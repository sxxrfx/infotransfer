use anyhow::{Context, Result, bail};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Three seconds at 24 kHz. Synthesis of the next chunk starts once its audio fits,
/// so this must cover the slowest chunk's generation time (Paradee: about 0.2 s for
/// 180 characters on an M4) or playback gaps. Settings changes wait behind it.
const QUEUE_SAMPLES: usize = 72_000;
/// While the queue is full, wait for about this much audio to play before retrying.
const FULL_WAIT: Duration = Duration::from_millis(20);
const PAUSED_WAIT: Duration = Duration::from_millis(50);
/// Taps of the band-limited resampler; half of them, 1 ms at 24 kHz, is its delay.
const TAPS: usize = 48;
const PHASES: usize = 256;

const STOPPED: u8 = 0;
const PLAYING: u8 = 1;
const PAUSED: u8 = 2;
static CLOCK: OnceLock<Instant> = OnceLock::new();
pub fn clock_ns() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

pub struct PlaybackRun {
    pub id: u64,
    pub sample_rate: u32,
    transport: AtomicU8,
    finished: AtomicBool,
    drained: AtomicBool,
    pub first_output_ns: AtomicU64,
    pub first_signal_ns: AtomicU64,
    pub first_signal_playback_ns: AtomicU64,
    pub first_playback_ns: AtomicU64,
    pub silence_playback_ns: AtomicU64,
    pub rendered_frames: AtomicU64,
    pub underrun_frames: AtomicU64,
    level: AtomicU32,
    queue: ArrayQueue<f32>,
}

impl PlaybackRun {
    fn new(id: u64, sample_rate: u32, active: bool) -> Self {
        Self {
            id,
            sample_rate,
            transport: AtomicU8::new(if active { PLAYING } else { STOPPED }),
            finished: AtomicBool::new(!active),
            drained: AtomicBool::new(!active),
            first_output_ns: AtomicU64::new(0),
            first_signal_ns: AtomicU64::new(0),
            first_signal_playback_ns: AtomicU64::new(0),
            first_playback_ns: AtomicU64::new(0),
            silence_playback_ns: AtomicU64::new(0),
            rendered_frames: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            level: AtomicU32::new(0f32.to_bits()),
            queue: ArrayQueue::new(QUEUE_SAMPLES),
        }
    }
    pub fn active(&self) -> bool {
        self.transport.load(Ordering::Acquire) != STOPPED && !self.is_drained()
    }
    pub fn paused(&self) -> bool {
        self.transport.load(Ordering::Acquire) == PAUSED
    }
    pub fn is_drained(&self) -> bool {
        self.drained.load(Ordering::Acquire)
    }
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Acquire))
    }
}

pub struct AudioBus {
    current: ArcSwap<PlaybackRun>,
    /// Benchmarks render and measure normally but send silence to the device.
    pub silent: AtomicBool,
}

/// One owner serializes transport commands; producers and the callback never lock it.
pub struct AudioControl {
    bus: Arc<AudioBus>,
    next_id: u64,
    retired: Vec<Arc<PlaybackRun>>,
}

impl Default for AudioControl {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioControl {
    pub fn new() -> Self {
        clock_ns();
        Self {
            bus: Arc::new(AudioBus {
                current: ArcSwap::from_pointee(PlaybackRun::new(0, 24_000, false)),
                silent: AtomicBool::new(false),
            }),
            next_id: 0,
            retired: Vec::new(),
        }
    }
    pub fn bus(&self) -> Arc<AudioBus> {
        self.bus.clone()
    }
    pub fn current(&self) -> Arc<PlaybackRun> {
        self.bus.current.load_full()
    }
    pub fn begin(&mut self, rate: u32) -> Arc<PlaybackRun> {
        self.stop();
        let previous = self.bus.current.load_full();
        self.next_id += 1;
        let run = Arc::new(PlaybackRun::new(self.next_id, rate, true));
        self.bus.current.store(run.clone());
        // store() pays outstanding ArcSwap reader debts. Retain a control-thread
        // owner until all producer/callback references are gone, so the callback
        // can never free the request's PCM allocation.
        self.retired.push(previous);
        self.retired.retain(|run| Arc::strong_count(run) > 1);
        run
    }
    pub fn pause(&mut self) {
        let run = self.bus.current.load();
        run.silence_playback_ns.store(0, Ordering::Release);
        let _ =
            run.transport
                .compare_exchange(PLAYING, PAUSED, Ordering::AcqRel, Ordering::Acquire);
    }
    pub fn resume(&mut self) {
        let run = self.bus.current.load();
        let _ =
            run.transport
                .compare_exchange(PAUSED, PLAYING, Ordering::AcqRel, Ordering::Acquire);
    }
    pub fn stop(&mut self) {
        let run = self.bus.current.load();
        run.silence_playback_ns.store(0, Ordering::Release);
        run.transport.store(STOPPED, Ordering::Release);
        run.level.store(0f32.to_bits(), Ordering::Release);
        run.drained.store(true, Ordering::Release);
    }
}

impl AudioBus {
    pub fn current(&self) -> Arc<PlaybackRun> {
        self.current.load_full()
    }
    pub fn push(&self, run: &PlaybackRun, samples: &[f32]) -> bool {
        for &value in samples {
            loop {
                if !run.active() {
                    return false;
                }
                match run.queue.push(value) {
                    Ok(()) => break,
                    Err(_) => {
                        std::thread::sleep(if run.paused() { PAUSED_WAIT } else { FULL_WAIT })
                    }
                }
            }
        }
        true
    }
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    pub device: String,
    pub sample_rate: u32,
    pub failed: Arc<AtomicBool>,
}

impl AudioOutput {
    pub fn start(bus: Arc<AudioBus>) -> Result<Self> {
        Self::start_on(bus, None)
    }
    pub fn start_on(bus: Arc<AudioBus>, name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let device = if let Some(name) = name {
            host.output_devices()?
                .find(|device| device.name().ok().as_deref() == Some(name))
        } else {
            host.default_output_device()
        }
        .context("No audio output is available")?;
        let name = device.name().unwrap_or_else(|_| "System output".into());
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config.into(), bus, failed.clone())?,
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config.into(), bus, failed.clone())?,
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config.into(), bus, failed.clone())?,
            _ => bail!("The current audio output format is unsupported"),
        };
        stream.play()?;
        Ok(Self {
            _stream: stream,
            device: name,
            sample_rate,
            failed,
        })
    }
    pub fn default_device_name() -> Option<String> {
        cpal::default_host().default_output_device()?.name().ok()
    }
}

/// Windowed-sinc interpolation. Linear interpolation mirrors speech energy above
/// 12 kHz when 24 kHz speech is played at 44.1 or 48 kHz, heard as a thin hiss.
struct Resampler {
    input_rate: u32,
    output_rate: u32,
    step: f64,
    /// (PHASES + 1) kernels of TAPS each; empty when the rates match.
    table: Vec<f32>,
    history: [f32; TAPS],
    position: f64,
    flushed: usize,
}
impl Resampler {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate,
            step: input_rate as f64 / output_rate as f64,
            table: if input_rate == output_rate {
                Vec::new()
            } else {
                kernel(input_rate, output_rate)
            },
            history: [0.; TAPS],
            position: 1.,
            flushed: 0,
        }
    }
    fn reset(&mut self) {
        self.history = [0.; TAPS];
        self.position = 1.;
        self.flushed = 0;
    }
    /// The next output sample, or None while no input is available. A finished
    /// run first plays out the filter's tail.
    fn next(&mut self, run: &PlaybackRun) -> Option<f32> {
        if self.table.is_empty() {
            return run.queue.pop();
        }
        while self.position >= 1. {
            let value = match run.queue.pop() {
                Some(value) => value,
                None if self.flushed < TAPS && run.finished.load(Ordering::Acquire) => {
                    self.flushed += 1;
                    0.
                }
                None => return None,
            };
            self.history.copy_within(1.., 0);
            self.history[TAPS - 1] = value;
            self.position -= 1.;
        }
        let scaled = self.position * PHASES as f64;
        let phase = (scaled as usize).min(PHASES - 1);
        let blend = (scaled - phase as f64) as f32;
        let a = &self.table[phase * TAPS..][..TAPS];
        let b = &self.table[(phase + 1) * TAPS..][..TAPS];
        let mut sum = 0.;
        for ((sample, a), b) in self.history.iter().zip(a).zip(b) {
            sum += sample * (a + (b - a) * blend);
        }
        self.position += self.step;
        Some(sum)
    }
}

/// Kaiser-windowed sinc kernels, one per fractional position, each with unity DC gain.
fn kernel(input_rate: u32, output_rate: u32) -> Vec<f32> {
    // Cutoff in cycles per input sample, just below the lower Nyquist frequency.
    let cutoff = 0.46 * (output_rate as f64 / input_rate as f64).min(1.);
    let beta = 8.;
    let center = (TAPS / 2 - 1) as f64;
    let half = (TAPS / 2) as f64;
    let mut table = Vec::with_capacity((PHASES + 1) * TAPS);
    for phase in 0..=PHASES {
        let start = table.len();
        let offset = phase as f64 / PHASES as f64;
        for tap in 0..TAPS {
            let x = tap as f64 - center - offset;
            let edge = x / half;
            let window = if edge.abs() >= 1. {
                0.
            } else {
                bessel_i0(beta * (1. - edge * edge).sqrt()) / bessel_i0(beta)
            };
            let sinc = if x == 0. {
                2. * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            table.push((sinc * window) as f32);
        }
        let sum: f32 = table[start..].iter().sum();
        for value in &mut table[start..] {
            *value /= sum;
        }
    }
    table
}
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1., 1., 1.);
    while term > 1e-12 * sum {
        term *= (x / (2. * k)).powi(2);
        sum += term;
        k += 1.;
    }
    sum
}

struct Renderer {
    request: u64,
    resampler: Resampler,
}
impl Renderer {
    fn new(output_rate: u32) -> Self {
        Self {
            request: 0,
            resampler: Resampler::new(24_000, output_rate),
        }
    }
    fn render<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        data: &mut [T],
        channels: usize,
        output_rate: u32,
        bus: &AudioBus,
        playback_ns: u64,
    ) {
        let run = bus.current.load();
        if run.transport.load(Ordering::Acquire) != PLAYING || run.is_drained() {
            data.fill(T::from_sample(0.));
            let _ = run.silence_playback_ns.compare_exchange(
                0,
                playback_ns,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
            return;
        }
        if self.request != run.id {
            self.request = run.id;
            if (self.resampler.input_rate, self.resampler.output_rate)
                != (run.sample_rate, output_rate)
            {
                // All speech is 24 kHz, so the stream's prebuilt kernel normally applies.
                self.resampler = Resampler::new(run.sample_rate, output_rate);
            }
            self.resampler.reset();
        }
        let mut energy = 0f32;
        let mut count = 0u64;
        for (frame_index, frame) in data.chunks_mut(channels).enumerate() {
            if run.transport.load(Ordering::Acquire) != PLAYING {
                frame.fill(T::from_sample(0.));
                let _ = run.silence_playback_ns.compare_exchange(
                    0,
                    playback_ns + frame_index as u64 * 1_000_000_000 / output_rate as u64,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
                continue;
            }
            let value = if let Some(sample) = self.resampler.next(&run) {
                sample.clamp(-1., 1.)
            } else {
                if run.finished.load(Ordering::Acquire) {
                    run.drained.store(true, Ordering::Release);
                } else if run.first_output_ns.load(Ordering::Relaxed) > 0 {
                    run.underrun_frames.fetch_add(1, Ordering::Relaxed);
                }
                0.
            };
            if value.abs() > 0.00001 && run.first_output_ns.load(Ordering::Relaxed) == 0 {
                let _ = run.first_output_ns.compare_exchange(
                    0,
                    clock_ns(),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
                run.first_playback_ns.store(
                    playback_ns + frame_index as u64 * 1_000_000_000 / output_rate as u64,
                    Ordering::Release,
                );
            }
            if value.abs() >= 0.003_162_277_6 && run.first_signal_ns.load(Ordering::Relaxed) == 0 {
                run.first_signal_playback_ns.store(
                    playback_ns + frame_index as u64 * 1_000_000_000 / output_rate as u64,
                    Ordering::Release,
                );
                let _ = run.first_signal_ns.compare_exchange(
                    0,
                    clock_ns(),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
            }
            frame.fill(T::from_sample(if bus.silent.load(Ordering::Relaxed) {
                0.
            } else {
                value
            }));
            energy += value * value;
            count += 1;
        }
        run.rendered_frames.fetch_add(count, Ordering::Relaxed);
        if count > 0 {
            run.level
                .store((energy / count as f32).sqrt().to_bits(), Ordering::Release);
        }
    }
}

fn stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    bus: Arc<AudioBus>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let rate = config.sample_rate.0;
    let mut renderer = Renderer::new(rate);
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], info| {
            let stamp = info.timestamp();
            let latency = stamp
                .playback
                .duration_since(&stamp.callback)
                .unwrap_or_default();
            renderer.render(
                data,
                channels,
                rate,
                &bus,
                clock_ns() + latency.as_nanos() as u64,
            )
        },
        move |_| {
            failed.store(true, Ordering::Release);
        },
        None,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_and_replacement_never_render_old_audio() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        let old = control.begin(24_000);
        assert!(bus.push(&old, &[0.5; 32]));
        let next = control.begin(24_000);
        assert!(!bus.push(&old, &[0.9]));
        assert!(bus.push(&next, &[-0.25; 32]));
        let mut renderer = Renderer::new(24_000);
        let mut output = [0f32; 16];
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == -0.25));
        control.stop();
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == 0.));
    }
    #[test]
    fn a_producer_waiting_on_a_full_queue_returns_promptly_on_stop() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        let run = control.begin(24_000);
        control.pause();
        let producer = {
            let (bus, run) = (bus.clone(), run.clone());
            std::thread::spawn(move || bus.push(&run, &vec![0.1; QUEUE_SAMPLES + 1]))
        };
        std::thread::sleep(Duration::from_millis(120));
        let stopped = Instant::now();
        control.stop();
        assert!(!producer.join().unwrap());
        assert!(stopped.elapsed() < Duration::from_millis(200));
    }
    #[test]
    fn pause_preserves_audio_and_meter() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        let run = control.begin(24_000);
        bus.push(&run, &[0.25; 32]);
        let mut renderer = Renderer::new(24_000);
        let mut output = [0f32; 8];
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        let level = run.level();
        control.pause();
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == 0.));
        assert_eq!(level, run.level());
        control.resume();
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == 0.25));
    }
    #[test]
    fn upsampling_keeps_level_and_rejects_mirrored_hiss() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        let run = control.begin(24_000);
        let tone = |frequency: f64, rate: f64, i: usize| {
            (std::f64::consts::TAU * frequency * i as f64 / rate).sin() as f32 * 0.5
        };
        let input: Vec<f32> = (0..4_800).map(|i| tone(9_000., 24_000., i)).collect();
        assert!(bus.push(&run, &input));
        run.finish();
        let mut renderer = Renderer::new(48_000);
        let mut output = vec![0f32; 9_800];
        for block in output.chunks_mut(512) {
            renderer.render(block, 1, 48_000, &bus, clock_ns());
        }
        assert!(run.is_drained());
        // Correlate a steady middle section with the tone and with its 15 kHz image.
        let section = &output[2_000..8_000];
        let energy = |frequency: f64| {
            let (mut re, mut im) = (0f64, 0f64);
            for (i, sample) in section.iter().enumerate() {
                let angle = std::f64::consts::TAU * frequency * i as f64 / 48_000.;
                re += *sample as f64 * angle.cos();
                im += *sample as f64 * angle.sin();
            }
            (re * re + im * im).sqrt() / section.len() as f64
        };
        let (signal, image) = (energy(9_000.), energy(15_000.));
        assert!((signal - 0.25).abs() < 0.03, "tone level {signal}");
        assert!(20. * (image / signal).log10() < -60., "image {image}");
    }
    #[test]
    fn silent_output_still_measures_the_render_path() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        bus.silent.store(true, Ordering::Relaxed);
        let run = control.begin(24_000);
        bus.push(&run, &[0.5; 32]);
        let mut renderer = Renderer::new(24_000);
        let mut output = [1f32; 16];
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == 0.));
        assert!(run.first_signal_ns.load(Ordering::Acquire) > 0);
        assert_eq!(run.level(), 0.5);
    }
    #[test]
    fn a_late_completion_cannot_finish_the_next_reading() {
        let mut control = AudioControl::new();
        let old = control.begin(24_000);
        let next = control.begin(24_000);
        old.finish();
        assert!(!control.current().is_drained());
        assert!(next.active());
        assert!(!old.active());
    }
}
