//! Right Option tap to audio output, end to end, in the background and muted.
//!
//! Each trial posts a right Option press and release to this process only. The events
//! come through the WindowServer to the production local-monitor flag reduction, then the
//! production gesture classifier, capture-worker hop, reader, synthesis, PCM queue and
//! CoreAudio callback. Two things are substituted: the capture worker returns test text
//! instead of calling Accessibility (no real selection is read), and the samples written to
//! the device are zeros (`set_silent_output`), so nothing is heard and system volume is
//! untouched. No window opens and no app is activated.
//!
//! Excluded: the Accessibility selection read, GPUI's foreground-task hop, and physical
//! latency after CoreAudio's reported playback time.
use anyhow::{Context, Result, ensure};
use infotransfer::{
    activation::{
        Activation, ActivationKey, CaptureKind, CapturePlan, Effect, KeyEdge, ModifierGesture,
    },
    assets::AssetPaths,
    audio::clock_ns,
    native::Selection,
    reader::{Origin, Phase, Reader, RuntimeOptions},
    settings::Settings,
    sound::SpeechAudio,
    speech::Speech,
    text::Chunks,
};
use serde_json::json;
use std::{
    io::Write,
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn it_bench_now_ns() -> u64;
    fn it_bench_app_init();
    fn it_bench_monitor(callback: extern "C" fn(u16, u8, i32, i32, u64, u64)) -> i32;
    fn it_bench_post_right_option(down: i32) -> u64;
    fn it_bench_pump(seconds: f64);
}

#[derive(Clone, Copy)]
struct Key {
    code: u16,
    flags: u8,
    down: bool,
    pid: i32,
    event_ns: u64,
    received_ns: u64,
}
static KEYS: Mutex<Vec<Key>> = Mutex::new(Vec::new());
extern "C" fn on_key(code: u16, flags: u8, down: i32, pid: i32, event_ns: u64, received_ns: u64) {
    KEYS.lock().unwrap().push(Key {
        code,
        flags,
        down: down != 0,
        pid,
        event_ns,
        received_ns,
    });
}

fn now() -> u64 {
    unsafe { it_bench_now_ns() }
}
fn pump(seconds: f64) {
    unsafe { it_bench_pump(seconds) }
}
/// `clock_ns()` and CLOCK_UPTIME_RAW share a timebase; this is the constant between them.
fn clock_offset() -> i128 {
    (0..64)
        .map(|_| {
            let a = now();
            let c = clock_ns();
            let b = now();
            (b - a, (a / 2 + b / 2) as i128 - c as i128)
        })
        .min_by_key(|(width, _)| *width)
        .unwrap()
        .1
}

/// The production capture worker (one pending slot, newest wins), returning test text.
struct Captured {
    plan: CapturePlan,
    selection: Selection,
    start: u64,
    end: u64,
}
struct Worker {
    pending: Arc<Mutex<Option<CapturePlan>>>,
    wake: crossbeam_channel::Sender<()>,
    results: crossbeam_channel::Receiver<Captured>,
}
impl Worker {
    fn new(text: String) -> Self {
        let pending = Arc::new(Mutex::new(None::<CapturePlan>));
        let work = pending.clone();
        let (wake, rx) = crossbeam_channel::bounded(1);
        let (tx, results) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("selection-capture".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    if let Some(plan) = work.lock().unwrap().take() {
                        let start = now();
                        let selection = Selection {
                            text: text.clone(),
                            range: Some((0, text.encode_utf16().count() as i64)),
                        };
                        let end = now();
                        let _ = tx.send(Captured {
                            plan,
                            selection,
                            start,
                            end,
                        });
                    }
                }
            })
            .expect("start capture worker");
        Self {
            pending,
            wake,
            results,
        }
    }
    fn capture(&self, plan: CapturePlan) {
        *self.pending.lock().unwrap() = Some(plan);
        let _ = self.wake.try_send(());
    }
}

#[derive(Default)]
struct Marks {
    press_event: u64,
    press_received: u64,
    hid: u64,
    received: u64,
    gesture: u64,
    capture_start: u64,
    capture_end: u64,
    dispatched: u64,
}

fn wait_pumping(limit: Duration, mut done: impl FnMut() -> bool) -> Result<()> {
    let start = Instant::now();
    while !done() {
        ensure!(
            start.elapsed() < limit,
            "Timed out after {} ms",
            limit.as_millis()
        );
        pump(0.0005);
    }
    Ok(())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = ((p / 100. * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let value = |key: &str, default: &str| {
        args.iter()
            .position(|arg| arg == key)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.into())
    };
    let runs: usize = value("--runs", "30").parse()?;
    let warmup: usize = value("--warmup", "2").parse()?;
    let hold_ms: u64 = value("--hold-ms", "90").parse()?;
    let gap_ms: u64 = value("--gap-ms", "300").parse()?;
    let stage_reps: usize = value("--stage-reps", "30").parse()?;
    let sample_secs: u64 = value("--sample-secs", "0").parse()?;
    // Idle time before each stage repetition, as between real readings.
    let stage_gap_ms: u64 = value("--stage-gap-ms", "0").parse()?;
    let bucket = value("--bucket", "example");
    let out = std::path::PathBuf::from(value("--out", "e2e"));
    let text = match bucket.as_str() {
        "short" => "Hello there.".to_owned(),
        "typical" => "Reading at your own pace can make a long paragraph easier to follow. You can pause whenever you need.".into(),
        "technical" => "At 3:45 p.m., Dr. Lee checked the JSON API and read 42 measurements from v2.1.0.".into(),
        _ => "The API returned 3.5 GB in 120ms.".into(),
    };
    std::fs::create_dir_all(&out)?;

    // Reader exactly as the app builds it, with the user's saved speech preferences.
    let paths = AssetPaths::discover()?;
    ensure!(
        paths.ready(),
        "Speech assets are not ready at {}",
        paths.speech.display()
    );
    let options = RuntimeOptions::default();
    let reader = Reader::with_options(paths.clone(), options.clone());
    reader.set_silent_output(true);
    let prefs = Settings::load().map(|(s, _)| s.speech).unwrap_or_default();
    reader.set_preferences(prefs.clone())?;
    reader.prewarm();
    let start = Instant::now();
    while !(reader.snapshot().model_ready && reader.snapshot().output_device.is_some()) {
        ensure!(
            start.elapsed() < Duration::from_secs(20),
            "Reader did not become ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    unsafe { it_bench_app_init() };
    ensure!(
        unsafe { it_bench_monitor(on_key) } != 0,
        "Could not install the key monitor"
    );

    let offset = clock_offset();
    let up = |clock: u64| (clock as i128 + offset) as u64;
    let mut log = std::fs::File::create(out.join(format!("trials-{bucket}.jsonl")))?;
    writeln!(
        log,
        "{}",
        json!({"event":"configuration","bucket":bucket,"text":text,
            "key":"right Option (code 61), flags-changed posted to this process only","hold_ms":hold_ms,
            "threads":options.threads,"first_chunk":options.first_chunk,
            "speed":prefs.speed,"tone":format!("{:?}",prefs.sound.tone),"device":reader.snapshot().output_device,
            "silent_output":true,"selection":"test text through the capture-worker hop (no Accessibility read)",
            "sampled":sample_secs > 0,"pid":std::process::id(),
            "start":"event timestamp of the key release (a tap reads on release)",
            "end":"CoreAudio playback timestamp of the first sample at or above -50 dBFS"})
    )?;

    let mut sampler = (sample_secs > 0)
        .then(|| {
            std::process::Command::new("/usr/bin/sample")
                .arg(std::process::id().to_string())
                .arg(sample_secs.to_string())
                .arg("1")
                .arg("-mayDie")
                .arg("-file")
                .arg(out.join(format!("sample-{bucket}.txt")))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        })
        .transpose()?;
    if sampler.is_some() {
        std::thread::sleep(Duration::from_millis(500));
    }

    let worker = Worker::new(text.clone());
    let mut gesture = ModifierGesture::new(ActivationKey::RightOption);
    let mut activation = Activation::default();
    let mut revision = 0u64;
    let mut totals = Vec::new();
    let mut failures = 0;
    for trial in 0..warmup + runs {
        let idle = Instant::now();
        while reader.is_synthesizing() || reader.snapshot().phase != Phase::Idle {
            ensure!(
                idle.elapsed() < Duration::from_secs(10),
                "Reader stayed busy"
            );
            pump(0.002);
        }
        KEYS.lock().unwrap().clear();
        let mut marks = Marks::default();
        let mut tap: Option<CapturePlan> = None;
        // Production handle_native: press (20), release (21), cancel (22).
        let mut handle = |marks: &mut Marks,
                          activation: &mut Activation,
                          revision: &mut u64,
                          tap: &mut Option<CapturePlan>| {
            for key in std::mem::take(&mut *KEYS.lock().unwrap()) {
                let Some(edge) = gesture.event(key.code, key.flags, key.down) else {
                    continue;
                };
                let effect = match edge {
                    KeyEdge::Press => {
                        marks.press_event = key.event_ns;
                        marks.press_received = key.received_ns;
                        reader.prewarm();
                        if activation.active() {
                            None
                        } else {
                            *revision += 1;
                            activation.press(key.pid, clock_ns(), *revision)
                        }
                    }
                    KeyEdge::Release => {
                        marks.hid = key.event_ns;
                        marks.received = key.received_ns;
                        activation.release(key.pid, clock_ns())
                    }
                    KeyEdge::Cancel => {
                        activation.cancel();
                        None
                    }
                };
                if let Some(Effect::Capture(plan)) = effect {
                    if plan.kind == CaptureKind::Tap {
                        marks.gesture = now();
                        *tap = Some(plan.clone());
                    }
                    worker.capture(plan);
                }
            }
        };
        let result = (|| -> Result<()> {
            unsafe { it_bench_post_right_option(1) };
            wait_pumping(Duration::from_millis(500), || {
                handle(&mut marks, &mut activation, &mut revision, &mut tap);
                marks.press_received > 0
            })
            .context("Press was not observed")?;
            let held = Instant::now() + Duration::from_millis(hold_ms);
            while Instant::now() < held {
                pump(0.001);
                handle(&mut marks, &mut activation, &mut revision, &mut tap);
                while let Ok(c) = worker.results.try_recv() {
                    if c.plan.revision == revision && c.plan.kind == CaptureKind::Baseline {
                        let _ =
                            activation.baseline(c.plan.revision, Some(c.selection.fingerprint()));
                    }
                }
            }
            unsafe { it_bench_post_right_option(0) };
            let mut read = None;
            wait_pumping(Duration::from_secs(2), || {
                handle(&mut marks, &mut activation, &mut revision, &mut tap);
                while let Ok(c) = worker.results.try_recv() {
                    if tap.as_ref().is_some_and(|t| t.revision == c.plan.revision)
                        && c.plan.kind == CaptureKind::Tap
                    {
                        marks.capture_start = c.start;
                        marks.capture_end = c.end;
                        marks.dispatched = now();
                        read = Some(
                            reader
                                .read_at(c.selection.text, Origin::Selection, c.plan.activation)
                                .map(|_| c.plan.activation),
                        );
                        return true;
                    }
                }
                false
            })
            .with_context(|| {
                format!(
                    "Tap capture did not complete (release seen: {})",
                    marks.received > 0
                )
            })?;
            let activation_ns = read.context("Reading was not requested")??;
            wait_pumping(Duration::from_secs(5), || {
                reader.snapshot().activation_ns == activation_ns
            })?;
            let run = reader.snapshot().run.context("No playback run")?;
            wait_pumping(Duration::from_secs(10), || {
                run.first_signal_playback_ns.load(Ordering::Acquire) > 0
                    || reader.snapshot().phase == Phase::Failed
            })?;
            ensure!(
                reader.snapshot().phase != Phase::Failed,
                "{}",
                reader.snapshot().notice.unwrap_or_default()
            );
            let snapshot = reader.snapshot();
            let load = |v: &std::sync::atomic::AtomicU64| v.load(Ordering::Acquire);
            let ms = |t: u64| (t as f64 - marks.hid as f64) / 1e6;
            let row = json!({
                "event":"trial","trial":trial,"warmup":trial < warmup,
                "press_delivery_ms":(marks.press_received as f64 - marks.press_event as f64)/1e6,
                "key_received_ms":ms(marks.received),
                "gesture_ms":ms(marks.gesture),
                "capture_start_ms":ms(marks.capture_start),
                "capture_end_ms":ms(marks.capture_end),
                "dispatched_ms":ms(marks.dispatched),
                "controller_ms":ms(up(snapshot.feedback_ns)),
                "first_pcm_ms":ms(up(snapshot.first_pcm_ns)),
                "first_render_ms":ms(up(load(&run.first_output_ns))),
                "first_signal_render_ms":ms(up(load(&run.first_signal_ns))),
                "audible_estimate_ms":ms(up(load(&run.first_signal_playback_ns))),
                "underrun_frames":load(&run.underrun_frames),
            });
            if trial >= warmup {
                totals.push(row["audible_estimate_ms"].as_f64().unwrap_or(f64::NAN));
            }
            writeln!(log, "{row}")?;
            Ok(())
        })();
        if let Err(error) = result {
            failures += 1;
            writeln!(
                log,
                "{}",
                json!({"event":"trial","trial":trial,"success":false,"error":format!("{error:#}")})
            )?;
        }
        reader.stop();
        let pause = Instant::now() + Duration::from_millis(gap_ms);
        while Instant::now() < pause {
            pump(0.01);
        }
    }
    if let Some(sampler) = &mut sampler {
        let _ = sampler.wait();
    }

    // Stage breakdown of the first chunk, outside the reader so each step can be timed.
    let mut stages = std::fs::File::create(out.join(format!("stages-{bucket}.jsonl")))?;
    let first = Chunks::with_limits(&text, options.first_chunk, options.following_chunk)
        .next()
        .context("No first chunk")?
        .to_owned();
    let speaker = infotransfer::voices::VOICES[prefs.voice as usize].speaker;
    let load_start = Instant::now();
    let mut speech = Speech::load(&paths, options.threads)?;
    let load_ms = load_start.elapsed().as_secs_f64() * 1e3;
    speech.set_dictionary(&prefs.dictionary)?;
    let lead = |pcm: &[f32], level: f32| {
        pcm.iter()
            .position(|x| x.abs() >= level)
            .unwrap_or(pcm.len()) as f64
            / 24.
    };
    for rep in 0..=stage_reps {
        std::thread::sleep(Duration::from_millis(stage_gap_ms));
        let t = Instant::now();
        let spoken = infotransfer::spoken::prepare(&first, &prefs.dictionary)?;
        let normalize = t.elapsed();
        // pronunciation() normalizes again, now without one-time regex setup.
        let t = Instant::now();
        let _ = infotransfer::spoken::prepare(&first, &prefs.dictionary)?;
        let normalize_again = t.elapsed();
        let t = Instant::now();
        let phones = speech.pronunciation(&first, speaker)?;
        let pronunciation = t.elapsed();
        let mut audio = SpeechAudio::new(24_000, prefs.sound);
        let mut processed = Vec::new();
        let mut eq = Duration::ZERO;
        let mut samples = 0;
        let (mut raw_nonzero, mut raw_signal, mut eq_nonzero, mut eq_signal) = (0., 0., 0., 0.);
        let generation = speech.generate(&first, speaker, prefs.speed, |pcm| {
            if samples == 0 {
                raw_nonzero = lead(pcm, 0.00001);
                raw_signal = lead(pcm, 0.003_162_277_6);
            }
            let t = Instant::now();
            audio.process(pcm, &mut processed);
            eq += t.elapsed();
            if samples == 0 {
                eq_nonzero = lead(&processed, 0.00001);
                eq_signal = lead(&processed, 0.003_162_277_6);
            }
            samples += pcm.len();
            true
        })?;
        let first_audio = generation.first_audio.unwrap_or(generation.elapsed);
        // generate() repeats normalization and the (now cached) pronunciation lookup.
        let onnx = first_audio.saturating_sub(pronunciation);
        writeln!(
            stages,
            "{}",
            json!({"rep":rep,"cold":rep==0,"chunk":first,"spoken":spoken,"phones":phones,
                "load_ms":if rep==0 {Some(load_ms)} else {None},
                "normalize_ms":normalize.as_secs_f64()*1e3,
                "phonemes_ms":pronunciation.saturating_sub(normalize_again).as_secs_f64()*1e3,
                "onnx_ms":onnx.as_secs_f64()*1e3,
                "eq_ms":eq.as_secs_f64()*1e3,
                "generate_ms":generation.elapsed.as_secs_f64()*1e3,
                "audio_ms":samples as f64/24.,
                "lead_raw_nonzero_ms":raw_nonzero,"lead_raw_signal_ms":raw_signal,
                "lead_eq_nonzero_ms":eq_nonzero,"lead_eq_signal_ms":eq_signal,
                "tokens":phones.chars().count(),"gap_ms":stage_gap_ms})
        )?;
    }
    drop(speech);
    reader.shutdown();

    totals.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({"event":"summary","bucket":bucket,"trials":runs,"failures":failures,
            "p50_ms":percentile(&totals,50.),"p95_ms":percentile(&totals,95.),
            "min_ms":totals.first(),"max_ms":totals.last()})
    );
    if failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}
