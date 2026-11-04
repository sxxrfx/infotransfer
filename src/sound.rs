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
    cleanup: Cleanup,
    filters: [Biquad; 2],
    trailing_quiet: usize,
    tail: VecDeque<f32>,
    fade_frames: usize,
    position: usize,
}
impl SpeechAudio {
    pub fn new(rate: u32, settings: SoundSettings) -> Self {
        Self {
            settings,
            rate,
            cleanup: Cleanup::new(rate),
            filters: Self::tone(rate, settings.tone),
            trailing_quiet: 0,
            tail: VecDeque::with_capacity((rate / 200 + 1) as usize),
            fade_frames: (rate / 200) as usize,
            position: 0,
        }
    }
    fn tone(rate: u32, tone: Tone) -> [Biquad; 2] {
        match tone {
            Tone::Natural => [Biquad::identity(), Biquad::identity()],
            Tone::Clear => [
                Biquad::peak(rate, 250., -2., 0.7),
                Biquad::peak(rate, 2500., 1.5, 0.7),
            ],
            Tone::Soft => [Biquad::peak(rate, 4500., -3., 0.5), Biquad::identity()],
        }
    }
    pub fn configure(&mut self, settings: SoundSettings) {
        if settings.tone != self.settings.tone {
            // Reconfigure only between chunks; never discard a sentence's filter state.
            self.filters = Self::tone(self.rate, settings.tone);
        }
        self.settings = settings;
    }
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        output.clear();
        output.reserve(input.len());
        for &sample in input {
            let mut value = self.cleanup.filter(sample);
            for filter in &mut self.filters {
                value = filter.process(value);
            }
            value = self.cleanup.expand(value);
            value *= self.settings.volume;
            // Smooth over-range peaks without changing ordinary samples below -1.1 dBFS.
            let magnitude = value.abs();
            if magnitude > 0.88 {
                value = value.signum() * (0.88 + 0.10 * ((magnitude - 0.88) / 0.10).tanh());
            }
            if self.position < self.fade_frames {
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
    /// Every chunk fades in and out over 5 ms, so a sentence split mid-way cannot click.
    pub fn finish_chunk(&mut self, output: &mut Vec<f32>) {
        output.clear();
        let count = self.tail.len();
        for i in 0..count {
            let value = self.tail.pop_front().unwrap() * (count - i - 1) as f32
                / count.saturating_sub(1).max(1) as f32;
            self.emit(value, output);
        }
        self.position = 0;
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

/// Always-on cleanup for Heart's output. Every stage is a causal IIR filter or
/// envelope, so none of them delays speech.
struct Cleanup {
    rumble: Biquad,
    hiss: Biquad,
    fizz: Biquad,
    sibilance: Biquad,
    sibilant_level: Envelope,
    speech_level: Envelope,
    quiet_level: Envelope,
    quiet_gain: f32,
    opening: f32,
    closing: f32,
}
impl Cleanup {
    fn new(rate: u32) -> Self {
        Self {
            rumble: Biquad::highpass(rate, 70., std::f32::consts::FRAC_1_SQRT_2),
            hiss: Biquad::high_shelf(rate, 7500., -3.),
            fizz: Biquad::lowpass(rate, 11_000., std::f32::consts::FRAC_1_SQRT_2),
            sibilance: Biquad::bandpass(rate, 6500., 1.2),
            sibilant_level: Envelope::new(rate, 1., 40.),
            speech_level: Envelope::new(rate, 1., 40.),
            quiet_level: Envelope::new(rate, 2., 60.),
            quiet_gain: 1.,
            // Opens within about a millisecond so word onsets keep their attack.
            opening: 1. - (-1. / (0.001 * rate as f32)).exp(),
            closing: 1. - (-1. / (0.020 * rate as f32)).exp(),
        }
    }
    fn filter(&mut self, sample: f32) -> f32 {
        let value = self.rumble.process(sample);
        // Split-band de-esser: when the 5–8 kHz band dominates, reduce only that band,
        // by at most 6 dB.
        let band = self.sibilance.process(value);
        let sibilant = self.sibilant_level.follow(band);
        let speech = self.speech_level.follow(value);
        let keep = if sibilant > 1e-6 {
            (0.6 * speech / sibilant).clamp(0.5, 1.)
        } else {
            1.
        };
        let value = value - (1. - keep) * band;
        self.fizz.process(self.hiss.process(value))
    }
    /// 2:1 downward expansion below -50 dBFS, limited to 24 dB: quieter hiss in pauses.
    fn expand(&mut self, value: f32) -> f32 {
        const THRESHOLD: f32 = 0.003_162_277_6;
        let level = self.quiet_level.follow(value);
        let target = (level / THRESHOLD).clamp(0.063, 1.);
        let step = if target > self.quiet_gain {
            self.opening
        } else {
            self.closing
        };
        self.quiet_gain += (target - self.quiet_gain) * step;
        value * self.quiet_gain
    }
}

/// Peak follower with separate attack and release times in milliseconds.
struct Envelope {
    attack: f32,
    release: f32,
    level: f32,
}
impl Envelope {
    fn new(rate: u32, attack_ms: f32, release_ms: f32) -> Self {
        let coefficient = |ms: f32| 1. - (-1. / (ms * 0.001 * rate as f32)).exp();
        Self {
            attack: coefficient(attack_ms),
            release: coefficient(release_ms),
            level: 0.,
        }
    }
    fn follow(&mut self, value: f32) -> f32 {
        let magnitude = value.abs();
        let step = if magnitude > self.level {
            self.attack
        } else {
            self.release
        };
        self.level += (magnitude - self.level) * step;
        self.level
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
    /// Normalizes RBJ cookbook coefficients, https://www.w3.org/TR/audio-eq-cookbook/
    fn new(b: [f32; 3], a: [f32; 3]) -> Self {
        Self {
            b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]],
            a: [a[1] / a[0], a[2] / a[0]],
            z: [0., 0.],
        }
    }
    fn angle(rate: u32, frequency: f32, q: f32) -> (f32, f32) {
        let w = std::f32::consts::TAU * frequency / rate as f32;
        (w.cos(), w.sin() / (2. * q))
    }
    fn peak(rate: u32, frequency: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let (cos, alpha) = Self::angle(rate, frequency, q);
        Self::new(
            [1. + alpha * a, -2. * cos, 1. - alpha * a],
            [1. + alpha / a, -2. * cos, 1. - alpha / a],
        )
    }
    fn lowpass(rate: u32, frequency: f32, q: f32) -> Self {
        let (cos, alpha) = Self::angle(rate, frequency, q);
        Self::new(
            [(1. - cos) / 2., 1. - cos, (1. - cos) / 2.],
            [1. + alpha, -2. * cos, 1. - alpha],
        )
    }
    fn highpass(rate: u32, frequency: f32, q: f32) -> Self {
        let (cos, alpha) = Self::angle(rate, frequency, q);
        Self::new(
            [(1. + cos) / 2., -(1. + cos), (1. + cos) / 2.],
            [1. + alpha, -2. * cos, 1. - alpha],
        )
    }
    /// Constant 0 dB peak gain.
    fn bandpass(rate: u32, frequency: f32, q: f32) -> Self {
        let (cos, alpha) = Self::angle(rate, frequency, q);
        Self::new([alpha, 0., -alpha], [1. + alpha, -2. * cos, 1. - alpha])
    }
    /// Shelf slope 1.
    fn high_shelf(rate: u32, frequency: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.);
        let (cos, alpha) = Self::angle(rate, frequency, std::f32::consts::FRAC_1_SQRT_2);
        let root = 2. * a.sqrt() * alpha;
        Self::new(
            [
                a * ((a + 1.) + (a - 1.) * cos + root),
                -2. * a * ((a - 1.) + (a + 1.) * cos),
                a * ((a + 1.) + (a - 1.) * cos - root),
            ],
            [
                (a + 1.) - (a - 1.) * cos + root,
                2. * ((a - 1.) - (a + 1.) * cos),
                (a + 1.) - (a - 1.) * cos - root,
            ],
        )
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
    fn tone(frequency: f32, amplitude: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (std::f32::consts::TAU * frequency * i as f32 / 24_000.).sin() * amplitude)
            .collect()
    }
    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }
    /// Steady-state gain in dB, skipping the filters' start-up and the fades.
    fn gain_db(frequency: f32, amplitude: f32) -> f32 {
        let input = tone(frequency, amplitude, 24_000);
        let mut audio = SpeechAudio::new(24_000, SoundSettings::default());
        let mut out = Vec::new();
        audio.process(&input, &mut out);
        20. * (rms(&out[4_800..20_000]) / rms(&input[4_800..20_000])).log10()
    }
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
        audio.process(&tone(500., 0.2, 480), &mut out);
        assert_eq!(audio.pause_samples(Boundary::Continuation), 0);
        audio.process(&[0.; 2400], &mut out);
        audio.finish_chunk(&mut out);
        // 300 ms less the existing quiet tail; the filters ring out for about 12 ms.
        let sentence = audio.pause_samples(Boundary::Sentence);
        assert!((4_800..=5_200).contains(&sentence), "{sentence}");
        assert_eq!(audio.pause_samples(Boundary::Sentence), 0);
        audio.process(&tone(500., 0.2, 480), &mut out);
        audio.finish_chunk(&mut out);
        let paragraph = audio.pause_samples(Boundary::Paragraph);
        assert!((15_000..=15_600).contains(&paragraph), "{paragraph}");
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
    fn cleanup_keeps_the_voice_band_and_removes_rumble_hiss_and_fizz() {
        assert!(gain_db(1_000., 0.3).abs() < 0.5);
        assert!(gain_db(3_000., 0.3).abs() < 0.8);
        assert!(gain_db(30., 0.3) < -12.);
        assert!(gain_db(9_000., 0.3) < -2.);
        assert!(gain_db(11_500., 0.3) < -6.);
    }
    #[test]
    fn de_esser_softens_a_dominant_sibilant_band_by_at_most_six_decibels() {
        let shelf_only = {
            let mut shelf = Biquad::high_shelf(24_000, 7500., -3.);
            let mut fizz = Biquad::lowpass(24_000, 11_000., std::f32::consts::FRAC_1_SQRT_2);
            let input = tone(6_500., 0.3, 24_000);
            let out: Vec<f32> = input
                .iter()
                .map(|s| fizz.process(shelf.process(*s)))
                .collect();
            20. * (rms(&out[4_800..20_000]) / rms(&input[4_800..20_000])).log10()
        };
        let reduction = shelf_only - gain_db(6_500., 0.3);
        assert!((2. ..=6.5).contains(&reduction), "{reduction}");
    }
    #[test]
    fn pauses_are_quieter_but_speech_level_is_not_expanded() {
        assert!(gain_db(1_000., 0.0008) < -6.);
        assert!(gain_db(1_000., 0.05).abs() < 0.5);
    }
    #[test]
    fn chunks_fade_and_eq_is_callback_size_independent() {
        let input = tone(440., 0.3, 24_000);
        let mut out = Vec::new();
        let mut tail = Vec::new();
        for tone in Tone::ALL {
            let settings = SoundSettings {
                tone,
                volume: 1.5,
                ..Default::default()
            };
            let mut whole = SpeechAudio::new(24_000, settings);
            whole.process(&input, &mut out);
            whole.finish_chunk(&mut tail);
            out.extend_from_slice(&tail);
            assert_eq!(out.len(), input.len());
            assert_eq!(out[0], 0.);
            assert_eq!(*out.last().unwrap(), 0.);
            let mut stream = SpeechAudio::new(24_000, settings);
            let mut block = Vec::new();
            let mut joined = Vec::new();
            for part in input.chunks(317) {
                stream.process(part, &mut block);
                joined.extend_from_slice(&block);
            }
            stream.finish_chunk(&mut block);
            joined.extend_from_slice(&block);
            assert_eq!(out, joined);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 0.98));
        }
    }
}
