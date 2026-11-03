//! Producer-side EQ and optional reading gaps. The audio callback only consumes PCM.
use crate::text::Boundary;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tone {
    #[default]
    Natural,
    Clear,
    Soft,
}
impl Tone {
    pub const ALL: [Self; 3] = [Self::Natural, Self::Clear, Self::Soft];
    pub fn label(self) -> &'static str {
        match self {
            Self::Natural => "Natural",
            Self::Clear => "Clear",
            Self::Soft => "Soft",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub sentence_pause_ms: u32,
    pub paragraph_pause_ms: u32,
    pub tone: Tone,
    pub volume: f32,
}
impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            sentence_pause_ms: 0,
            paragraph_pause_ms: 0,
            tone: Tone::Natural,
            volume: 1.,
        }
    }
}
impl SoundSettings {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.sentence_pause_ms <= 1000 && self.paragraph_pause_ms <= 2000,
            "Pause is out of range"
        );
        ensure!(
            (0.25..=1.5).contains(&self.volume),
            "Volume must be between 25% and 150%"
        );
        Ok(())
    }
}

pub struct SpeechAudio {
    settings: SoundSettings,
    rate: u32,
    filters: [Biquad; 2],
    trailing_quiet: usize,
    tail: VecDeque<f32>,
    fade_frames: usize,
    position: usize,
    fade_in: bool,
}
impl SpeechAudio {
    pub fn new(rate: u32, settings: SoundSettings) -> Self {
        let filters = match settings.tone {
            Tone::Natural => [Biquad::identity(), Biquad::identity()],
            Tone::Clear => [
                Biquad::peak(rate, 250., -2., 0.7),
                Biquad::peak(rate, 2500., 1.5, 0.7),
            ],
            Tone::Soft => [Biquad::peak(rate, 4500., -3., 0.5), Biquad::identity()],
        };
        Self {
            settings,
            rate,
            filters,
            trailing_quiet: 0,
            tail: VecDeque::with_capacity((rate / 200 + 1) as usize),
            fade_frames: (rate / 200) as usize,
            position: 0,
            fade_in: true,
        }
    }
    pub fn configure(&mut self, settings: SoundSettings) {
        if settings.tone != self.settings.tone {
            // Reconfigure only between chunks; never discard a sentence's filter state.
            let next = Self::new(self.rate, settings);
            self.filters = next.filters;
        }
        self.settings = settings;
    }
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        output.clear();
        output.reserve(input.len());
        for &sample in input {
            let mut value = sample;
            for filter in &mut self.filters {
                value = filter.process(value);
            }
            value *= self.settings.volume;
            // Smooth over-range peaks without changing ordinary samples below -1.1 dBFS.
            let magnitude = value.abs();
            if magnitude > 0.88 {
                value = value.signum() * (0.88 + 0.10 * ((magnitude - 0.88) / 0.10).tanh());
            }
            if self.fade_in && self.position < self.fade_frames {
                value *= self.position as f32 / (self.fade_frames - 1) as f32;
            }
            self.position += 1;
            self.tail.push_back(value);
            if self.tail.len() > self.fade_frames {
                let value = self.tail.pop_front().unwrap();
                self.emit(value, output);
            }
        }
    }
    fn emit(&mut self, value: f32, output: &mut Vec<f32>) {
        if value.abs() < 0.001 {
            self.trailing_quiet += 1;
        } else {
            self.trailing_quiet = 0;
        }
        output.push(value);
    }
    pub fn finish_chunk(&mut self, boundary: Boundary, terminal: bool, output: &mut Vec<f32>) {
        output.clear();
        let fade = terminal || boundary != Boundary::Continuation;
        let count = self.tail.len();
        for i in 0..count {
            let mut value = self.tail.pop_front().unwrap();
            if fade {
                value *= (count - i - 1) as f32 / count.saturating_sub(1).max(1) as f32;
            }
            self.emit(value, output);
        }
        self.position = 0;
        self.fade_in = fade;
    }
    /// Minimum quiet time after punctuation, counting the model's existing tail.
    /// Called only when another chunk follows; never delays the first or final word.
    pub fn pause_samples(&mut self, boundary: Boundary) -> usize {
        let milliseconds = match boundary {
            Boundary::Continuation => 0,
            Boundary::Sentence => self.settings.sentence_pause_ms,
            Boundary::Paragraph => self
                .settings
                .paragraph_pause_ms
                .max(self.settings.sentence_pause_ms),
        };
        let needed =
            (self.rate as usize * milliseconds as usize / 1000).saturating_sub(self.trailing_quiet);
        self.trailing_quiet += needed;
        needed
    }
}

struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}
impl Biquad {
    fn identity() -> Self {
        Self {
            b: [1., 0., 0.],
            a: [0., 0.],
            z: [0., 0.],
        }
    }
    /// RBJ peaking EQ, https://www.w3.org/TR/audio-eq-cookbook/
    fn peak(rate: u32, frequency: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let w = std::f32::consts::TAU * frequency / rate as f32;
        let alpha = w.sin() / (2. * q);
        let a0 = 1. + alpha / a;
        Self {
            b: [
                (1. + alpha * a) / a0,
                -2. * w.cos() / a0,
                (1. - alpha * a) / a0,
            ],
            a: [-2. * w.cos() / a0, (1. - alpha / a) / a0],
            z: [0., 0.],
        }
    }
    fn process(&mut self, value: f32) -> f32 {
        let out = self.b[0] * value + self.z[0];
        self.z[0] = self.b[1] * value - self.a[0] * out + self.z[1];
        self.z[1] = self.b[2] * value - self.a[1] * out;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pauses_count_existing_quiet_and_do_not_break_continuations() {
        let mut audio = SpeechAudio::new(
            24_000,
            SoundSettings {
                sentence_pause_ms: 300,
                paragraph_pause_ms: 650,
                ..Default::default()
            },
        );
        let mut out = Vec::new();
        audio.process(&[0.2; 100], &mut out);
        assert_eq!(audio.pause_samples(Boundary::Continuation), 0);
        audio.process(&[0.; 2400], &mut out);
        audio.finish_chunk(Boundary::Continuation, false, &mut out);
        assert_eq!(audio.pause_samples(Boundary::Sentence), 4800);
        assert_eq!(audio.pause_samples(Boundary::Sentence), 0);
        audio.process(&[0.2; 100], &mut out);
        audio.finish_chunk(Boundary::Continuation, false, &mut out);
        assert_eq!(audio.pause_samples(Boundary::Paragraph), 15_600);
    }
    #[test]
    fn natural_pacing_never_inserts_silence() {
        let mut audio = SpeechAudio::new(24_000, SoundSettings::default());
        for boundary in [
            Boundary::Continuation,
            Boundary::Sentence,
            Boundary::Paragraph,
        ] {
            assert_eq!(audio.pause_samples(boundary), 0);
        }
    }
    #[test]
    fn natural_preserves_ordinary_pcm_and_eq_is_callback_size_independent() {
        let input: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.19).sin() * 0.3).collect();
        let mut out = Vec::new();
        let mut natural = SpeechAudio::new(24_000, SoundSettings::default());
        natural.process(&input, &mut out);
        let mut tail = Vec::new();
        natural.finish_chunk(Boundary::Sentence, true, &mut tail);
        out.extend_from_slice(&tail);
        assert_eq!(out.len(), input.len());
        assert_eq!(out[120..out.len() - 120], input[120..input.len() - 120]);
        assert_eq!(out[0], 0.);
        assert_eq!(*out.last().unwrap(), 0.);
        for tone in Tone::ALL {
            let settings = SoundSettings {
                tone,
                volume: 1.5,
                ..Default::default()
            };
            let mut whole = SpeechAudio::new(24_000, settings);
            whole.process(&input, &mut out);
            whole.finish_chunk(Boundary::Sentence, true, &mut tail);
            out.extend_from_slice(&tail);
            let mut stream = SpeechAudio::new(24_000, settings);
            let mut block = Vec::new();
            let mut joined = Vec::new();
            for part in input.chunks(317) {
                stream.process(part, &mut block);
                joined.extend_from_slice(&block);
            }
            stream.finish_chunk(Boundary::Sentence, true, &mut block);
            joined.extend_from_slice(&block);
            assert_eq!(out, joined);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 0.98));
        }
    }
}
