use crate::{
    assets::{AssetPaths, SpeechModel},
    audio::{AudioBus, AudioControl, AudioOutput, PlaybackRun, clock_ns},
    sound::{SoundSettings, SpeechAudio},
    speech::Speech,
    text::{self, Chunks},
    voices::VOICES,
};
use anyhow::{Result, ensure};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A notice keeps the popup and its Escape shortcut alive, so it must not linger.
#[cfg(not(test))]
const NOTICE_LIFETIME: Duration = Duration::from_secs(4);
#[cfg(test)]
const NOTICE_LIFETIME: Duration = Duration::from_millis(40);
/// Closing an idle output lets the device sleep; speech reopens it on demand.
const OUTPUT_IDLE_NS: u64 = 60_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing,
    Reading,
    Paused,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Selection,
    Clipboard,
    Preview,
    History,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Preferences {
    #[serde(with = "crate::voices::persisted")]
    pub voice: u32,
    pub speed: f32,
    pub memory_saver: bool,
    pub sound: SoundSettings,
    pub dictionary: Vec<crate::dictionary::WordReplacement>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            voice: 0,
            speed: 1.,
            memory_saver: false,
            sound: SoundSettings::default(),
            dictionary: crate::dictionary::defaults(),
        }
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub phase: Phase,
    pub model_ready: bool,
    pub loaded_model: Option<SpeechModel>,
    pub output_device: Option<String>,
    pub notice: Option<String>,
    pub run: Option<Arc<PlaybackRun>>,
    pub activation_ns: u64,
    pub first_pcm_ns: u64,
    pub feedback_ns: u64,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            model_ready: false,
            loaded_model: None,
            output_device: None,
            notice: None,
            run: None,
            activation_ns: 0,
            first_pcm_ns: 0,
            feedback_ns: 0,
        }
    }
}

struct Shared {
    snapshot: RwLock<Snapshot>,
    preferences: RwLock<Preferences>,
    wake: async_channel::Sender<()>,
    quitting: AtomicBool,
    synthesizing: AtomicBool,
    audio_wake: Sender<()>,
    output_wanted_ns: AtomicU64,
}
impl Shared {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.snapshot.write().unwrap());
        let _ = self.wake.try_send(());
    }
    fn touch_output(&self) {
        self.output_wanted_ns
            .store(clock_ns().max(1), Ordering::Release);
        let _ = self.audio_wake.try_send(());
    }
}

enum Command {
    Read {
        content: Arc<str>,
        origin: Origin,
        activation: u64,
        preview_voice: Option<u32>,
    },
    Pause,
    Resume,
    Stop,
    Preferences(Preferences),
    Notice(String),
    ModelReady(SpeechModel, bool, Option<String>),
    RetryLoad,
    Started {
        run: u64,
        time: u64,
    },
    GenerationFailed {
        run: u64,
        error: String,
    },
    OutputReady(String),
    OutputLost(String),
    Shutdown,
}
struct SpeechJob {
    run: Arc<PlaybackRun>,
    content: Arc<str>,
    origin: Origin,
    preview_voice: Option<u32>,
}

enum SpeechSource {
    Catalog { models: Vec<AssetPaths> },
    Fixed(AssetPaths),
}
impl SpeechSource {
    fn paths(&self, voice: u32) -> &AssetPaths {
        match self {
            Self::Fixed(paths) => paths,
            Self::Catalog { models } => models
                .iter()
                .find(|paths| paths.model == VOICES[voice as usize].model)
                .expect("catalog model"),
        }
    }
    fn speaker(&self, voice: u32) -> u32 {
        match self {
            Self::Fixed(_) => voice,
            Self::Catalog { .. } => VOICES[voice as usize].speaker,
        }
    }
}
struct SpeechMailbox {
    pending: Mutex<Option<SpeechJob>>,
    wake: Sender<()>,
    idle_since: AtomicU64,
}
impl SpeechMailbox {
    fn read(&self, job: SpeechJob) {
        *self.pending.lock().unwrap() = Some(job);
        self.wake();
    }
    fn wake(&self) {
        let _ = self.wake.try_send(());
    }
    fn idle(&self) {
        self.idle_since.store(clock_ns(), Ordering::Release);
        self.wake();
    }
}

#[derive(Clone)]
pub struct RuntimeOptions {
    pub threads: u32,
    pub warm_start: bool,
    pub output_device: Option<String>,
    pub first_chunk: usize,
    pub following_chunk: usize,
}
impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            threads: 2,
            warm_start: true,
            output_device: None,
            first_chunk: 180,
            following_chunk: 180,
        }
    }
}

struct Handle {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    bus: Arc<AudioBus>,
    jobs: Arc<SpeechMailbox>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    source: Arc<SpeechSource>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}
impl Handle {
    fn shutdown(&self) {
        self.shared.quitting.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Shutdown);
        let _ = self.shared.audio_wake.try_send(());
        self.jobs.wake();
        for thread in self.threads.lock().unwrap().drain(..) {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
pub struct Reader {
    handle: Arc<Handle>,
    pub notifications: async_channel::Receiver<()>,
}

impl Reader {
    pub fn start(paths: AssetPaths) -> Self {
        Self::with_catalog(paths, RuntimeOptions::default())
    }
    pub fn with_catalog(paths: AssetPaths, options: RuntimeOptions) -> Self {
        let mut models = vec![paths];
        for model in [
            SpeechModel::KittenCpu,
            SpeechModel::KokoroHeart,
            SpeechModel::Qwen3,
            SpeechModel::VoxCpm2,
            SpeechModel::CosyVoice3,
            SpeechModel::Sopro,
            SpeechModel::Phonon,
        ] {
            if !models.iter().any(|paths| paths.model == model) {
                models
                    .push(AssetPaths::discover_model(model).expect("locate optional model assets"));
            }
        }
        Self::start_source(SpeechSource::Catalog { models }, options)
    }
    pub fn with_options(paths: AssetPaths, options: RuntimeOptions) -> Self {
        Self::start_source(SpeechSource::Fixed(paths), options)
    }
    fn start_source(source: SpeechSource, options: RuntimeOptions) -> Self {
        let source = Arc::new(source);
        Speech::quiet_backend_logs();
        let (wake, notifications) = async_channel::bounded(1);
        let (audio_wake, audio_rx) = bounded(1);
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(Snapshot::default()),
            preferences: RwLock::new(Preferences::default()),
            wake,
            quitting: AtomicBool::new(false),
            synthesizing: AtomicBool::new(false),
            audio_wake,
            output_wanted_ns: AtomicU64::new(0),
        });
        let (commands, rx) = unbounded();
        let (job_wake, job_rx) = bounded(1);
        let jobs = Arc::new(SpeechMailbox {
            pending: Mutex::new(None),
            wake: job_wake,
            idle_since: AtomicU64::new(clock_ns()),
        });
        let control = AudioControl::new();
        let bus = control.bus();
        let handle = Arc::new(Handle {
            commands: commands.clone(),
            shared: shared.clone(),
            bus: bus.clone(),
            jobs: jobs.clone(),
            threads: Mutex::new(Vec::new()),
            source: source.clone(),
        });
        let worker_shared = shared.clone();
        let worker_commands = commands.clone();
        let worker_bus = bus.clone();
        let worker_paths = source.clone();
        let worker_jobs = jobs.clone();
        let worker_options = options.clone();
        let speech_thread = std::thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                speech_worker(
                    worker_paths,
                    job_rx,
                    worker_jobs,
                    worker_options,
                    worker_commands,
                    worker_shared,
                    worker_bus,
                )
            })
            .expect("start speech worker");
        let audio_shared = shared.clone();
        let audio_commands = commands.clone();
        let audio_bus = bus.clone();
        let audio_thread = std::thread::Builder::new()
            .name("audio-devices".into())
            .spawn(move || {
                audio_worker(
                    audio_bus,
                    audio_commands,
                    audio_shared,
                    audio_rx,
                    options.output_device,
                )
            })
            .expect("start audio supervisor");
        let controller_shared = shared.clone();
        let control_thread = std::thread::Builder::new()
            .name("reading-control".into())
            .spawn(move || controller(source, control, rx, jobs, controller_shared))
            .expect("start reading controller");
        handle
            .threads
            .lock()
            .unwrap()
            .extend([speech_thread, audio_thread, control_thread]);
        Self {
            handle,
            notifications,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.handle.shared.snapshot.read().unwrap().clone()
    }
    pub fn preferences(&self) -> Preferences {
        self.handle.shared.preferences.read().unwrap().clone()
    }
    pub fn assets_for_voice(&self, voice: u32) -> AssetPaths {
        self.handle.source.paths(voice).clone()
    }
    pub fn read(&self, content: String, origin: Origin) -> Result<()> {
        self.read_at(content, origin, clock_ns())
    }
    pub fn read_at(&self, content: String, origin: Origin, activation: u64) -> Result<()> {
        if let Err(error) = text::validate(&content) {
            self.notice(error.to_string());
            return Err(error);
        }
        self.handle.commands.send(Command::Read {
            content: Arc::from(content),
            origin,
            activation,
            preview_voice: None,
        })?;
        Ok(())
    }
    pub fn preview(&self, voice: u32) {
        if voice < crate::voices::VOICE_COUNT {
            let _ = self.handle.commands.send(Command::Read {
                content: Arc::from("Reading at your own pace can make a long paragraph easier to follow. Press Escape to stop."),
                origin: Origin::Preview,
                activation: clock_ns(),
                preview_voice: Some(voice),
            });
        }
    }
    /// Opens audio output ahead of a likely reading, so it costs less first-speech time.
    pub fn prewarm(&self) {
        self.handle.shared.touch_output();
    }
    pub fn pause_or_resume(&self) {
        let phase = self.snapshot().phase;
        let _ = self.handle.commands.send(if phase == Phase::Paused {
            Command::Resume
        } else {
            Command::Pause
        });
    }
    pub fn pause(&self) {
        let _ = self.handle.commands.send(Command::Pause);
    }
    pub fn stop(&self) {
        let _ = self.handle.commands.send(Command::Stop);
    }
    pub fn notice(&self, message: String) {
        let _ = self.handle.commands.send(Command::Notice(message));
    }
    pub fn set_preferences(&self, prefs: Preferences) -> Result<()> {
        prefs.sound.validate()?;
        crate::dictionary::validate(&prefs.dictionary)?;
        ensure!(
            prefs.voice < crate::voices::VOICE_COUNT,
            "Choose an available voice"
        );
        ensure!(
            (0.75..=2.).contains(&prefs.speed),
            "Speed must be between 0.75 and 2"
        );
        self.handle.commands.send(Command::Preferences(prefs))?;
        Ok(())
    }
    pub fn level(&self) -> f32 {
        self.handle.bus.current().level()
    }
    pub fn is_synthesizing(&self) -> bool {
        self.handle.shared.synthesizing.load(Ordering::Acquire)
    }
    pub fn shutdown(&self) {
        self.handle.shutdown();
    }
    pub fn retry_load(&self) {
        let _ = self.handle.commands.send(Command::RetryLoad);
    }
}

fn controller(
    source: Arc<SpeechSource>,
    mut control: AudioControl,
    rx: Receiver<Command>,
    jobs: Arc<SpeechMailbox>,
    shared: Arc<Shared>,
) {
    jobs.wake();
    let mut manual_pause = false;
    let mut route_resume = false;
    let mut shown_notice = None::<String>;
    let mut notice_deadline = None::<Instant>;
    loop {
        let (reading, notice) = {
            let view = shared.snapshot.read().unwrap();
            (view.phase == Phase::Reading, view.notice.clone())
        };
        if notice != shown_notice {
            notice_deadline = notice.is_some().then(|| Instant::now() + NOTICE_LIFETIME);
            shown_notice = notice;
        }
        let wait = match (reading, notice_deadline) {
            (true, Some(deadline)) => Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(33)),
            ),
            (true, None) => Some(Duration::from_millis(33)),
            (false, Some(deadline)) => Some(deadline.saturating_duration_since(Instant::now())),
            (false, None) => None,
        };
        let command = if let Some(wait) = wait {
            match rx.recv_timeout(wait) {
                Ok(command) => Some(command),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                Err(_) => break,
            }
        } else {
            match rx.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };
        match command {
            Some(Command::Read {
                content,
                origin,
                activation,
                preview_voice,
            }) => {
                let voice = preview_voice.unwrap_or(shared.preferences.read().unwrap().voice);
                if !source.paths(voice).ready() {
                    shared.update(|view| {
                        view.notice = Some(format!(
                            "Download {} in Voices before reading",
                            VOICES[voice as usize].name
                        ))
                    });
                    continue;
                }
                let run = control.begin(24_000);
                shared.touch_output();
                manual_pause = false;
                route_resume = false;
                shared.update(|view| {
                    view.phase = Phase::Preparing;
                    view.notice = None;
                    view.run = Some(run.clone());
                    view.activation_ns = activation;
                    view.first_pcm_ns = 0;
                    view.feedback_ns = clock_ns();
                });
                jobs.read(SpeechJob {
                    run,
                    content,
                    origin,
                    preview_voice,
                });
            }
            Some(Command::Pause) => {
                manual_pause = true;
                control.pause();
                if control.current().active() {
                    shared.update(|view| view.phase = Phase::Paused);
                }
            }
            Some(Command::Resume) => {
                manual_pause = false;
                shared.touch_output();
                control.resume();
                if control.current().active() {
                    shared.update(|view| {
                        view.phase = if view.first_pcm_ns > 0 {
                            Phase::Reading
                        } else {
                            Phase::Preparing
                        }
                    });
                }
            }
            Some(Command::Stop) => {
                control.stop();
                jobs.idle();
                manual_pause = false;
                route_resume = false;
                shared.update(|view| {
                    view.phase = Phase::Idle;
                    view.notice = None;
                });
            }
            Some(Command::Preferences(prefs)) => {
                let model = source.paths(prefs.voice).model;
                *shared.preferences.write().unwrap() = prefs;
                jobs.wake();
                shared.update(|view| {
                    view.model_ready = view.loaded_model == Some(model);
                    if matches!(view.phase, Phase::Idle | Phase::Failed) {
                        view.notice = None;
                        view.phase = Phase::Idle;
                    }
                });
            }
            Some(Command::Notice(message)) => shared.update(|view| view.notice = Some(message)),
            Some(Command::RetryLoad) => jobs.wake(),
            Some(Command::ModelReady(model, ready, error)) => {
                if ready && shared.snapshot.read().unwrap().phase == Phase::Idle {
                    jobs.idle();
                }
                shared.update(|view| {
                    if ready {
                        view.loaded_model = Some(model);
                    } else if view.loaded_model == Some(model) {
                        view.loaded_model = None;
                    }
                    let selected_model =
                        source.paths(shared.preferences.read().unwrap().voice).model;
                    view.model_ready = view.loaded_model == Some(selected_model);
                    if model == selected_model
                        && let Some(error) = error
                    {
                        view.notice = Some(error);
                    }
                });
            }
            Some(Command::Started { run, time }) => {
                if control.current().id == run && control.current().active() {
                    shared.update(|view| {
                        view.first_pcm_ns = time;
                        view.phase = if control.current().paused() {
                            Phase::Paused
                        } else {
                            Phase::Reading
                        };
                    });
                }
            }
            Some(Command::GenerationFailed { run, error }) => {
                if control.current().id == run && control.current().active() {
                    control.stop();
                    shared.update(|view| {
                        view.phase = Phase::Failed;
                        view.notice = Some(error);
                    });
                }
            }
            Some(Command::OutputReady(device)) => {
                shared.update(|view| view.output_device = Some(device));
                if route_resume && !manual_pause && control.current().active() {
                    control.resume();
                    shared.update(|view| {
                        view.notice = None;
                        view.phase = if view.first_pcm_ns > 0 {
                            Phase::Reading
                        } else {
                            Phase::Preparing
                        };
                    });
                }
                route_resume = false;
            }
            Some(Command::OutputLost(error)) => {
                route_resume = control.current().active() && !manual_pause;
                let active = control.current().active();
                control.pause();
                shared.update(|view| {
                    view.output_device = None;
                    if active {
                        view.notice = Some(error);
                        view.phase = Phase::Paused;
                    }
                });
            }
            Some(Command::Shutdown) => {
                control.stop();
                shared.quitting.store(true, Ordering::Release);
                break;
            }
            None => {
                if notice_deadline.is_some_and(|deadline| deadline <= Instant::now()) {
                    notice_deadline = None;
                    shown_notice = None;
                    shared.update(|view| {
                        view.notice = None;
                        if view.phase == Phase::Failed {
                            view.phase = Phase::Idle;
                        }
                    });
                }
                if reading {
                    let run = control.current();
                    if run.is_drained() {
                        jobs.idle();
                        shared.update(|view| view.phase = Phase::Idle);
                    } else {
                        shared.update(|_| {});
                    }
                }
            }
        }
    }
}

fn speech_worker(
    source: Arc<SpeechSource>,
    rx: Receiver<()>,
    jobs: Arc<SpeechMailbox>,
    options: RuntimeOptions,
    commands: Sender<Command>,
    shared: Arc<Shared>,
    bus: Arc<AudioBus>,
) {
    let mut engine = None;
    let mut warmed = None;
    loop {
        let saver = shared.preferences.read().unwrap().memory_saver;
        let timeout = if saver && engine.is_some() && bus.current().is_drained() {
            Duration::from_secs(300).saturating_sub(Duration::from_nanos(
                clock_ns().saturating_sub(jobs.idle_since.load(Ordering::Acquire)),
            ))
        } else if saver && engine.is_some() {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(timeout) {
            Ok(()) => (),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if saver
                    && bus.current().is_drained()
                    && clock_ns().saturating_sub(jobs.idle_since.load(Ordering::Acquire))
                        >= 300_000_000_000
                    && let Some((model, _)) = engine.take()
                {
                    let _ = commands.send(Command::ModelReady(model, false, None));
                }
                continue;
            }
            Err(_) => break,
        };
        if shared.quitting.load(Ordering::Acquire) {
            break;
        }
        let job = jobs.pending.lock().unwrap().take();
        let voice = job
            .as_ref()
            .and_then(|job| job.preview_voice)
            .unwrap_or(shared.preferences.read().unwrap().voice);
        let paths = source.paths(voice);
        if engine.is_none() {
            warmed = None;
        }
        if paths.ready() && options.warm_start && job.is_none() {
            if let Ok(speech) = load_engine(&mut engine, paths, &options, &commands, || {
                shared.quitting.load(Ordering::Acquire)
            }) && paths.model.local_key().is_none()
                && warmed != Some(paths.model)
            {
                // ONNX Runtime allocates during its first inference. Pay for that here,
                // silently, rather than on the first reading after launch.
                let _ = speech.generate("Ready.", source.speaker(voice), 1., |_| true);
                warmed = Some(paths.model);
            }
        } else if !paths.ready()
            && engine
                .as_ref()
                .is_some_and(|(model, _)| *model != paths.model)
            && let Some((model, _)) = engine.take()
        {
            let _ = commands.send(Command::ModelReady(model, false, None));
        }
        if let Some(SpeechJob {
            run,
            content,
            origin,
            preview_voice,
        }) = job
        {
            let _ = origin;
            if !run.active() {
                continue;
            }
            let mut started = false;
            let mut failed = false;
            let mut audio = SpeechAudio::new(24_000, shared.preferences.read().unwrap().sound);
            let mut processed = Vec::with_capacity(4096);
            let mut chunks =
                Chunks::with_limits(&content, options.first_chunk, options.following_chunk);
            shared.synthesizing.store(true, Ordering::Release);
            while let Some(chunk) = chunks.next() {
                if !run.active() {
                    break;
                }
                let prefs = shared.preferences.read().unwrap().clone();
                let voice = preview_voice.unwrap_or(prefs.voice);
                audio.configure(prefs.sound);
                let result = if !crate::spoken::speakable(chunk, &prefs.dictionary) {
                    Ok(())
                } else {
                    load_engine(
                        &mut engine,
                        source.paths(voice),
                        &options,
                        &commands,
                        || !run.active() || shared.quitting.load(Ordering::Acquire),
                    )
                    .and_then(|engine| {
                        engine.set_dictionary(&prefs.dictionary)?;
                        engine.generate_cancellable(
                            chunk,
                            source.speaker(voice),
                            prefs.speed,
                            |samples| {
                                audio.process(samples, &mut processed);
                                for part in processed.chunks(1024) {
                                    if !bus.push(&run, part) {
                                        return false;
                                    }
                                    if !started {
                                        started = true;
                                        let _ = commands.send(Command::Started {
                                            run: run.id,
                                            time: clock_ns(),
                                        });
                                    }
                                }
                                true
                            },
                            || !run.active(),
                        )
                    })
                    .map(|_| ())
                };
                if let Err(error) = result {
                    failed = true;
                    let _ = commands.send(Command::GenerationFailed {
                        run: run.id,
                        error: error.to_string(),
                    });
                    break;
                }
                audio.finish_chunk(chunks.boundary(), !chunks.has_more(), &mut processed);
                for part in processed.chunks(1024) {
                    if !bus.push(&run, part) {
                        break;
                    }
                    if !started {
                        started = true;
                        let _ = commands.send(Command::Started {
                            run: run.id,
                            time: clock_ns(),
                        });
                    }
                }
                if chunks.has_more() {
                    let mut remaining = audio.pause_samples(chunks.boundary());
                    let silence = [0.; 1024];
                    while remaining > 0 && run.active() {
                        let count = remaining.min(silence.len());
                        if !bus.push(&run, &silence[..count]) {
                            break;
                        }
                        remaining -= count;
                    }
                }
            }
            if !failed && run.active() {
                if started {
                    run.finish();
                } else {
                    let _ = commands.send(Command::GenerationFailed {
                        run: run.id,
                        error: "Nothing to read in this selection".into(),
                    });
                }
            }
            shared.synthesizing.store(false, Ordering::Release);
            if engine.as_ref().is_some_and(|(_, speech)| !speech.alive()) {
                let (model, _) = engine.take().unwrap();
                let _ = commands.send(Command::ModelReady(model, false, None));
                jobs.wake();
            }
        }
    }
}

fn load_engine<'a>(
    engine: &'a mut Option<(SpeechModel, Speech)>,
    paths: &AssetPaths,
    options: &RuntimeOptions,
    commands: &Sender<Command>,
    cancel: impl Fn() -> bool,
) -> Result<&'a mut Speech> {
    if engine
        .as_ref()
        .is_none_or(|(model, speech)| *model != paths.model || !speech.alive())
    {
        if let Some((model, _)) = engine.take() {
            let _ = commands.send(Command::ModelReady(model, false, None));
        }
        match Speech::load_cancellable(paths, options.threads, &cancel) {
            Ok(loaded) => {
                *engine = Some((paths.model, loaded));
                let _ = commands.send(Command::ModelReady(paths.model, true, None));
            }
            Err(error) => {
                if cancel() {
                    return Err(error);
                }
                let _ = commands.send(Command::ModelReady(
                    paths.model,
                    false,
                    Some(error.to_string()),
                ));
                return Err(error);
            }
        }
    }
    Ok(&mut engine.as_mut().expect("loaded speech engine").1)
}

fn audio_worker(
    bus: Arc<AudioBus>,
    commands: Sender<Command>,
    shared: Arc<Shared>,
    wake: Receiver<()>,
    device: Option<String>,
) {
    let mut output: Option<AudioOutput> = None;
    let mut last_error = None;
    while !shared.quitting.load(Ordering::Acquire) {
        let run = bus.current();
        if run.active() && !run.paused() {
            shared
                .output_wanted_ns
                .store(clock_ns().max(1), Ordering::Release);
        }
        let wanted_at = shared.output_wanted_ns.load(Ordering::Acquire);
        if wanted_at == 0 || clock_ns().saturating_sub(wanted_at) >= OUTPUT_IDLE_NS {
            output = None;
            last_error = None;
            if matches!(
                wake.recv_timeout(Duration::from_secs(5)),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected)
            ) {
                break;
            }
            continue;
        }
        let desired = device.clone().or_else(AudioOutput::default_device_name);
        let changed = output.as_ref().is_some_and(|stream| {
            Some(&stream.device) != desired.as_ref() || stream.failed.load(Ordering::Acquire)
        });
        if changed {
            output = None;
            let _ = commands.send(Command::OutputLost("Switching audio output…".into()));
        }
        if output.is_none() {
            match AudioOutput::start_on(bus.clone(), device.as_deref()) {
                Ok(stream) => {
                    let _ = commands.send(Command::OutputReady(stream.device.clone()));
                    output = Some(stream);
                    last_error = None;
                }
                Err(error) => {
                    let error = error.to_string();
                    if last_error.as_ref() != Some(&error) {
                        let _ = commands.send(Command::OutputLost(error.clone()));
                        last_error = Some(error);
                    }
                }
            }
        }
        if matches!(
            wake.recv_timeout(Duration::from_millis(500)),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected)
        ) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader() -> Reader {
        let paths = AssetPaths {
            model: SpeechModel::KittenCpu,
            speech: "/nonexistent".into(),
            library: "/nonexistent".into(),
            local: None,
        };
        Reader::with_options(paths, RuntimeOptions::default())
    }
    fn wait_until(check: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn notices_expire_so_the_popup_cannot_stay_stuck() {
        let reader = reader();
        reader.notice("No text selected".into());
        assert!(wait_until(|| reader.snapshot().notice.is_some()));
        assert!(wait_until(|| reader.snapshot().notice.is_none()));
    }
    #[test]
    fn a_missing_voice_explains_itself_and_stays_idle() {
        let reader = reader();
        reader.read("Hello".into(), Origin::Selection).unwrap();
        assert!(wait_until(|| reader
            .snapshot()
            .notice
            .is_some_and(|notice| notice.contains("Download"))));
        assert_eq!(reader.snapshot().phase, Phase::Idle);
    }
    #[test]
    fn idle_app_keeps_audio_output_closed() {
        let reader = reader();
        std::thread::sleep(Duration::from_millis(100));
        assert!(reader.snapshot().output_device.is_none());
    }
}
