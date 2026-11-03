use crate::{
    assets::AssetPaths,
    dictionary::{self, WordReplacement},
    phonemes::EnglishFrontend,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    rc::Rc,
    time::{Duration, Instant},
};

type AudioCallback = unsafe extern "C" fn(*const f32, c_int, f32, *mut c_void) -> c_int;
unsafe extern "C" {
    fn it_speech_quiet_logs();
    fn it_speech_create(
        library: *const c_char,
        model: *const c_char,
        voices: *const c_char,
        tokens: *const c_char,
        threads: c_int,
        kind: c_int,
        error: *mut c_char,
        error_size: usize,
    ) -> *mut c_void;
    fn it_speech_destroy(engine: *mut c_void);
    fn it_speech_sample_rate(engine: *mut c_void) -> c_int;
    fn it_speech_speakers(engine: *mut c_void) -> c_int;
    fn it_speech_generate(
        engine: *mut c_void,
        text: *const c_char,
        phonemes: *const i32,
        phoneme_count: i32,
        voice: c_int,
        speed: f32,
        callback: AudioCallback,
        context: *mut c_void,
        sample_count: *mut c_int,
    ) -> c_int;
}

pub struct Speech {
    backend: Backend,
    pub sample_rate: u32,
    pub voices: u32,
    frontend: Option<EnglishFrontend>,
    dictionary: Vec<WordReplacement>,
    _thread_bound: PhantomData<Rc<()>>,
}
enum Backend {
    Native(NonNull<c_void>),
    Local(crate::advanced::LocalSpeech),
}

#[derive(Clone, Debug)]
pub struct Generation {
    pub first_audio: Option<Duration>,
    pub elapsed: Duration,
    pub samples: usize,
    pub callbacks: usize,
    pub peak: f32,
    pub canceled: bool,
}

struct CallbackState<F> {
    callback: F,
    start: Instant,
    first: Option<Duration>,
    samples: usize,
    callbacks: usize,
    peak: f32,
    error: Option<&'static str>,
    canceled: bool,
}

impl Speech {
    pub fn quiet_backend_logs() {
        // Applied once by the GUI runtime before private text reaches the frontend.
        unsafe { it_speech_quiet_logs() };
    }

    pub fn load(paths: &AssetPaths, threads: u32) -> Result<Self> {
        Self::load_cancellable(paths, threads, || false)
    }
    pub fn load_cancellable(
        paths: &AssetPaths,
        threads: u32,
        cancel: impl Fn() -> bool,
    ) -> Result<Self> {
        ensure!(paths.ready(), "Download speech assets before reading");
        if let Some(key) = paths.model.local_key() {
            let runtime = paths
                .local
                .as_ref()
                .context("Set up this local model first")?;
            let engine = crate::advanced::LocalSpeech::load_cancellable(runtime, key, cancel)?;
            return Ok(Self {
                backend: Backend::Local(engine),
                sample_rate: 24_000,
                voices: 1,
                frontend: None,
                dictionary: dictionary::defaults(),
                _thread_bound: PhantomData,
            });
        }
        paths.verify_active_assets()?;
        let frontend = EnglishFrontend::load(paths)?;
        let library = CString::new(paths.library.as_os_str().as_encoded_bytes())?;
        let names = [paths.model.filename(), "voices.bin", "tokens.txt"];
        let strings = names
            .iter()
            .map(|name| CString::new(paths.speech.join(name).as_os_str().as_encoded_bytes()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut error = [0u8; 1024];
        // All C strings and the writable error buffer live through synchronous creation.
        let raw = unsafe {
            it_speech_create(
                library.as_ptr(),
                strings[0].as_ptr(),
                strings[1].as_ptr(),
                strings[2].as_ptr(),
                threads.clamp(1, 8) as i32,
                paths.model.native_kind(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        let engine = NonNull::new(raw).with_context(|| {
            String::from_utf8_lossy(&error)
                .trim_end_matches('\0')
                .to_owned()
        })?;
        // A successfully created engine owns both queries; the handle remains alive.
        let (sample_rate, voices) = unsafe {
            (
                it_speech_sample_rate(engine.as_ptr()),
                it_speech_speakers(engine.as_ptr()),
            )
        };
        let required_speakers = match paths.model {
            crate::assets::SpeechModel::Kitten | crate::assets::SpeechModel::KittenCpu => 8,
            crate::assets::SpeechModel::KokoroHeart => 4,
            crate::assets::SpeechModel::Kokoro => 1,
            _ => unreachable!(),
        };
        if sample_rate != 24_000 || voices < required_speakers {
            unsafe { it_speech_destroy(engine.as_ptr()) };
            bail!("Speech runtime has invalid output configuration");
        }
        Ok(Self {
            backend: Backend::Native(engine),
            sample_rate: sample_rate as u32,
            voices: voices as u32,
            frontend: Some(frontend),
            dictionary: dictionary::defaults(),
            _thread_bound: PhantomData,
        })
    }

    pub fn generate<F: FnMut(&[f32]) -> bool>(
        &mut self,
        text: &str,
        voice: u32,
        speed: f32,
        callback: F,
    ) -> Result<Generation> {
        self.generate_cancellable(text, voice, speed, callback, || false)
    }
    pub fn set_dictionary(&mut self, entries: &[WordReplacement]) -> Result<()> {
        dictionary::validate(entries)?;
        if self.dictionary != entries {
            self.dictionary = entries.to_vec();
        }
        Ok(())
    }
    pub fn alive(&self) -> bool {
        match &self.backend {
            Backend::Native(_) => true,
            Backend::Local(model) => model.alive(),
        }
    }
    pub fn generate_cancellable<F: FnMut(&[f32]) -> bool>(
        &mut self,
        text: &str,
        voice: u32,
        speed: f32,
        callback: F,
        cancel: impl Fn() -> bool,
    ) -> Result<Generation> {
        ensure!(voice < self.voices, "Unknown voice");
        ensure!(
            (0.75..=2.).contains(&speed),
            "Speed must be between 0.75 and 2"
        );
        let started = Instant::now();
        let spoken = crate::spoken::prepare(text, &self.dictionary)?;
        let engine = match &mut self.backend {
            Backend::Local(model) => {
                let preparation = started.elapsed();
                let mut result = model.generate(&spoken, speed, callback, cancel)?;
                result.first_audio = result.first_audio.map(|time| time + preparation);
                result.elapsed = started.elapsed();
                return Ok(result);
            }
            Backend::Native(engine) => *engine,
        };
        let phonemes = self
            .frontend
            .as_mut()
            .context("Missing pronunciation frontend")?
            .phonemes(&spoken)?;
        if cancel() {
            return Ok(Generation {
                first_audio: None,
                elapsed: started.elapsed(),
                samples: 0,
                callbacks: 0,
                peak: 0.,
                canceled: true,
            });
        }
        let input = CString::new(spoken).context("Text contains a null character")?;
        let mut state = CallbackState {
            callback,
            start: started,
            first: None,
            samples: 0,
            callbacks: 0,
            peak: 0.,
            error: None,
            canceled: false,
        };
        let mut sample_count = 0;
        // Generation is synchronous. The callback context and its borrowed data cannot
        // outlive this call; Speech is thread-bound and exclusively borrowed.
        let success = unsafe {
            it_speech_generate(
                engine.as_ptr(),
                input.as_ptr(),
                phonemes.as_ptr(),
                phonemes.len() as i32,
                voice as i32,
                speed,
                receive::<F>,
                (&mut state as *mut CallbackState<F>).cast(),
                &mut sample_count,
            )
        };
        if let Some(error) = state.error {
            bail!(error);
        }
        ensure!(success != 0, "Speech generation failed");
        ensure!(
            state.canceled || (state.samples > 0 && state.peak > 0.00001),
            "Speech engine produced no audible samples"
        );
        Ok(Generation {
            first_audio: state.first,
            elapsed: state.start.elapsed(),
            samples: state.samples,
            callbacks: state.callbacks,
            peak: state.peak,
            canceled: state.canceled,
        })
    }
}

unsafe extern "C" fn receive<F: FnMut(&[f32]) -> bool>(
    samples: *const f32,
    n: c_int,
    _progress: f32,
    context: *mut c_void,
) -> c_int {
    // The C shim invokes this callback serially during generate(), with its live context.
    let state = unsafe { &mut *context.cast::<CallbackState<F>>() };
    if samples.is_null() || n <= 0 {
        state.error = Some("Speech engine returned invalid samples");
        return 0;
    }
    let audio = unsafe { std::slice::from_raw_parts(samples, n as usize) };
    if audio.iter().any(|sample| !sample.is_finite()) {
        state.error = Some("Speech engine returned non-finite samples");
        return 0;
    }
    let peak = audio
        .iter()
        .fold(0f32, |peak, sample| peak.max(sample.abs()));
    state.peak = state.peak.max(peak);
    if state.first.is_none() && peak > 0.00001 {
        state.first = Some(state.start.elapsed());
    }
    state.samples += audio.len();
    state.callbacks += 1;
    match catch_unwind(AssertUnwindSafe(|| (state.callback)(audio))) {
        Ok(true) => 1,
        Ok(false) => {
            state.canceled = true;
            0
        }
        Err(_) => {
            state.error = Some("Speech audio consumer failed");
            0
        }
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        // This uniquely owned thread-bound handle was created by the matching C shim.
        if let Backend::Native(engine) = self.backend {
            unsafe { it_speech_destroy(engine.as_ptr()) };
        }
    }
}
