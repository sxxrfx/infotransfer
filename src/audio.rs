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
            queue: ArrayQueue::new(48_000),
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
                        std::thread::sleep(Duration::from_millis(2));
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

struct Renderer {
    request: u64,
    current: Option<f32>,
    next: Option<f32>,
    phase: f64,
}
impl Renderer {
    fn new() -> Self {
        Self {
            request: 0,
            current: None,
            next: None,
            phase: 0.,
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
            self.current = None;
            self.next = None;
            self.phase = 0.;
        }
        let ratio = run.sample_rate as f64 / output_rate as f64;
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
            if self.current.is_none() {
                self.current = run.queue.pop();
            }
            if self.next.is_none() {
                self.next = run.queue.pop();
            }
            let value = if let Some(current) = self.current {
                let next = self.next.unwrap_or(current);
                let sample = current + (next - current) * self.phase as f32;
                self.phase += ratio;
                while self.phase >= 1. {
                    self.phase -= 1.;
                    self.current = self.next.take();
                    self.next = run.queue.pop();
                }
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
            frame.fill(T::from_sample(value));
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
    let mut renderer = Renderer::new();
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
        let mut renderer = Renderer::new();
        let mut output = [0f32; 16];
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == -0.25));
        control.stop();
        renderer.render(&mut output, 1, 24_000, &bus, clock_ns());
        assert!(output.iter().all(|sample| *sample == 0.));
    }
    #[test]
    fn pause_preserves_audio_and_meter() {
        let mut control = AudioControl::new();
        let bus = control.bus();
        let run = control.begin(24_000);
        bus.push(&run, &[0.25; 32]);
        let mut renderer = Renderer::new();
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
