use anyhow::{Context, Result, bail};
use infotransfer::{
    assets::{AssetPaths, SpeechModel},
    audio::{PlaybackRun, clock_ns},
    reader::{Origin, Phase, Reader, RuntimeOptions},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn it_resident_bytes() -> u64;
}

fn wait(mut predicate: impl FnMut() -> bool, limit: Duration) -> Result<()> {
    let start = Instant::now();
    while !predicate() {
        if start.elapsed() > limit {
            bail!("Timed out after {} ms", limit.as_millis());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
fn elapsed_ns(end: u64, start: u64) -> Option<f64> {
    (end >= start && end > 0).then(|| (end - start) as f64 / 1_000_000.)
}
fn request(reader: &Reader, content: String) -> Result<(u64, Arc<PlaybackRun>)> {
    let activation = clock_ns();
    reader.read_at(content, Origin::Selection, activation)?;
    wait(
        || reader.snapshot().activation_ns == activation,
        Duration::from_secs(5),
    )?;
    Ok((
        activation,
        reader.snapshot().run.context("No playback request")?,
    ))
}
fn bench_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("INFOTRANSFER_BENCH_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8),
    )
}
fn first_output(reader: &Reader, run: &PlaybackRun) -> Result<()> {
    wait(
        || {
            run.first_signal_ns.load(Ordering::Acquire) > 0
                || reader.snapshot().phase == Phase::Failed
        },
        bench_timeout(),
    )?;
    if reader.snapshot().phase == Phase::Failed {
        bail!("{}", reader.snapshot().notice.unwrap_or_default());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let value = |key: &str, default: &str| {
        args.iter()
            .position(|arg| arg == key)
            .and_then(|index| args.get(index + 1))
            .cloned()
            .unwrap_or_else(|| default.into())
    };
    let runs = value("--runs", "5").parse::<usize>()?;
    let cold = args.iter().any(|arg| arg == "--cold");
    let replace = args.iter().any(|arg| arg == "--replace");
    let replacement_delay = value("--replace-delay", "120").parse::<u64>()?;
    let play_ms = value("--play-ms", "25").parse::<u64>()?;
    let device = args
        .iter()
        .position(|arg| arg == "--device")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let model = match value("--engine", "paradee").as_str() {
        "paradee" => SpeechModel::Paradee,
        other => anyhow::bail!("Unsupported engine: {other}"),
    };
    let defaults = RuntimeOptions::default();
    let options = RuntimeOptions {
        threads: value("--threads", &model.threads().to_string()).parse()?,
        first_chunk: value("--first-chunk", &defaults.first_chunk.to_string()).parse()?,
        following_chunk: value("--following-chunk", &defaults.following_chunk.to_string())
            .parse()?,
        output_device: device,
        warm_start: !cold,
    };
    let paths = AssetPaths::discover_model(model)?;
    let catalog_check = args.iter().any(|arg| arg == "--catalog-check");
    let catalog = catalog_check || args.iter().any(|arg| arg == "--catalog");
    let reader = if catalog {
        Reader::with_catalog(paths, options.clone())
    } else {
        Reader::with_options(paths, options.clone())
    };
    // --silent measures the same render path without anyone hearing it; the system
    // volume and mute setting are left alone.
    reader.set_silent_output(args.iter().any(|arg| arg == "--silent"));
    reader.prewarm();
    let mut prefs = reader.preferences();
    prefs.voice = value("--voice", "0").parse()?;
    prefs.speed = value("--speed", "1").parse()?;
    reader.set_preferences(prefs.clone())?;
    wait(
        || reader.snapshot().output_device.is_some(),
        Duration::from_secs(5),
    )?;
    if !cold {
        let expected = if catalog {
            infotransfer::voices::VOICES[prefs.voice as usize].model
        } else {
            model
        };
        wait(
            || reader.snapshot().model_ready && reader.snapshot().loaded_model == Some(expected),
            bench_timeout(),
        )?;
    }
    if catalog_check {
        catalog_behavior_check(&reader)?;
        reader.shutdown();
        return Ok(());
    }
    println!(
        "{}",
        json!({"event":"configuration","pid":std::process::id(),"engine":model.label(),
        "threads":options.threads,"first_chunk":options.first_chunk,"following_chunk":options.following_chunk,
        "voice":prefs.voice,"speed":prefs.speed,"device":reader.snapshot().output_device,
        "method":"request_to_real_render_callback; signal_threshold_minus50_dBFS; playback_and_silence_are_CPAL_double_buffer_estimates_not_acoustic",
        "ready_resident_bytes":unsafe {it_resident_bytes()},"cold":cold,"replacement":replace,"silent_output":args.iter().any(|arg| arg == "--silent"),
        "includes":"validation, controller, synthesis, bounded_PCM, resampling, real_audio_callback",
        "excludes":"selection_capture, GPUI, physical_device_latency"})
    );
    let bucket = value("--bucket", "typical");
    let content = match bucket.as_str() {
        "short" => "Hello.".to_owned(),
        "long" => "Reading at your own pace can make a long paragraph easier to follow. ".repeat(1600).chars().take(100_000).collect(),
        "unpunctuated" => "Reading at your own pace can make a long paragraph easier to follow ".repeat(1400),
        "technical" => "At 3:45 p.m., Dr. Lee checked example.com and read 42 measurements.".into(),
        _ => "Reading at your own pace can make a long paragraph easier to follow. You can pause whenever you need.".into(),
    };
    let mut failures = 0;
    for trial in 0..runs {
        let result = (|| -> Result<Value> {
            wait(|| !reader.is_synthesizing(), bench_timeout())?;
            if replace {
                let (_, prior) = request(
                    &reader,
                    "Reading at your own pace can make a long paragraph easier to follow. "
                        .repeat(100),
                )?;
                first_output(&reader, &prior)?;
                std::thread::sleep(Duration::from_millis(replacement_delay));
            }
            let (activation, run) = request(&reader, content.clone())?;
            first_output(&reader, &run)?;
            let snapshot = reader.snapshot();
            std::thread::sleep(Duration::from_millis(play_ms));
            let stop = clock_ns();
            reader.stop();
            wait(
                || run.silence_playback_ns.load(Ordering::Acquire) > stop,
                Duration::from_secs(2),
            )?;
            let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
            unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
            Ok(
                json!({"event":"trial","trial":trial,"success":true,"bucket":bucket,
                "phase":if cold && trial==0 {"cold"} else if trial==0 {"first_inference"} else {"warm"},
                "control_feedback_ms":elapsed_ns(snapshot.feedback_ns,activation),
                "first_pcm_ms":elapsed_ns(snapshot.first_pcm_ns,activation),
                "first_render_ms":elapsed_ns(run.first_output_ns.load(Ordering::Acquire),activation),
                "first_signal_render_ms":elapsed_ns(run.first_signal_ns.load(Ordering::Acquire),activation),
                "first_signal_playback_estimate_ms":elapsed_ns(run.first_signal_playback_ns.load(Ordering::Acquire),activation),
                "first_playback_estimate_ms":elapsed_ns(run.first_playback_ns.load(Ordering::Acquire),activation),
                "stop_silence_estimate_ms":elapsed_ns(run.silence_playback_ns.load(Ordering::Acquire),stop),
                "underrun_frames":run.underrun_frames.load(Ordering::Acquire),
                "resident_bytes":unsafe {it_resident_bytes()},"peak_resident_bytes":usage.ru_maxrss}),
            )
        })();
        match result {
            Ok(row) => println!("{row}"),
            Err(error) => {
                failures += 1;
                reader.stop();
                println!(
                    "{}",
                    json!({"event":"trial","trial":trial,"success":false,"error":error.to_string()})
                );
            }
        }
    }
    reader.shutdown();
    println!(
        "{}",
        json!({"event":"summary","trials":runs,"failures":failures})
    );
    if failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn catalog_behavior_check(reader: &Reader) -> Result<()> {
    let selected = reader.preferences().voice;
    reader.preview(0);
    wait(
        || reader.snapshot().phase == Phase::Preparing,
        Duration::from_secs(5),
    )?;
    let run = reader.snapshot().run.context("preview run")?;
    first_output(reader, &run)?;
    anyhow::ensure!(
        reader.preferences().voice == selected,
        "Preview changed saved selection"
    );
    anyhow::ensure!(
        reader.snapshot().loaded_model == Some(SpeechModel::Paradee),
        "Preview loaded wrong model"
    );
    reader.stop();
    wait(
        || reader.snapshot().phase == Phase::Idle,
        Duration::from_secs(2),
    )?;
    anyhow::ensure!(
        !run.active() && !run.paused(),
        "Stopped preview remains resumable"
    );
    reader.pause_or_resume();
    std::thread::sleep(Duration::from_millis(50));
    anyhow::ensure!(!run.active(), "Cancelled preview resumed");
    for voice in [0, 0] {
        reader.prewarm();
        let mut prefs = reader.preferences();
        prefs.voice = voice;
        reader.set_preferences(prefs)?;
    }
    wait(
        || {
            reader.preferences().voice == 0
                && reader.snapshot().model_ready
                && reader.snapshot().loaded_model == Some(SpeechModel::Paradee)
        },
        bench_timeout(),
    )?;
    let (_, run) = request(
        reader,
        "Reading at your own pace can make a long paragraph easier to follow.".into(),
    )?;
    first_output(reader, &run)?;
    reader.stop();
    println!(
        "{}",
        json!({"event":"catalog_check","success":true,
        "preview_preserved_voice":selected,"heart_model":true,"stop_cancelled":true,"repeated_selection_kept_heart":true})
    );
    Ok(())
}
