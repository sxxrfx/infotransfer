use crate::{
    assets::{AssetPaths, SpeechModel},
    dictionary::{self, WordReplacement},
    phonemes::EnglishFrontend,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    ffi::{CString, c_char, c_int, c_void},
    fs,
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
        threads: c_int,
        error: *mut c_char,
        error_size: usize,
    ) -> *mut c_void;
    fn it_speech_destroy(engine: *mut c_void);
    fn it_speech_generate(
        engine: *mut c_void,
        tokens: *const i64,
        token_count: i32,
        voice: c_int,
        speed: f32,
        callback: AudioCallback,
        context: *mut c_void,
        sample_count: *mut c_int,
    ) -> c_int;
}

/// The graph has 512 token positions, two of which hold the padding at each end.
const MAX_TOKENS: usize = 510;

pub struct Speech {
    engine: NonNull<c_void>,
    model: SpeechModel,
    pub sample_rate: u32,
    pub voices: u32,
    frontend: EnglishFrontend,
    /// Phone codepoint to model token id.
    tokens: HashMap<i32, i64>,
    dictionary: Vec<WordReplacement>,
    _thread_bound: PhantomData<Rc<()>>,
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
        paths.verify_active_assets()?;
        ensure!(!cancel(), "Speech loading canceled");
        let frontend = EnglishFrontend::load(paths)?;
        let mut tokens = HashMap::new();
        for line in fs::read_to_string(paths.speech.join("tokens.txt"))?.lines() {
            let (symbol, id) = line.rsplit_once(' ').context("Invalid token table")?;
            let mut chars = symbol.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                tokens.insert(ch as i32, id.parse()?);
            }
        }
        let library = CString::new(paths.library.as_os_str().as_encoded_bytes())?;
        let model = CString::new(
            paths
                .speech
                .join(paths.model.filename())
                .as_os_str()
                .as_encoded_bytes(),
        )?;
        let mut error = [0u8; 1024];
        // Both C strings and the writable error buffer live through synchronous creation.
        let raw = unsafe {
            it_speech_create(
                library.as_ptr(),
                model.as_ptr(),
                threads.clamp(1, 8) as i32,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        let engine = NonNull::new(raw).with_context(|| {
            String::from_utf8_lossy(&error)
                .trim_end_matches('\0')
                .to_owned()
        })?;
        Ok(Self {
            engine,
            model: paths.model,
            sample_rate: 24_000,
            voices: paths.model.speakers(),
            frontend,
            tokens,
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
    /// The phones sent to the model for this text, for pronunciation diagnostics.
    pub fn pronunciation(&mut self, text: &str, voice: u32) -> Result<String> {
        let spoken = crate::spoken::prepare(text, &self.dictionary)?;
        let phones = self.frontend.phonemes(&spoken, self.model.british(voice))?;
        Ok(phones
            .into_iter()
            .filter_map(|code| char::from_u32(code as u32))
            .collect())
    }
    pub fn set_dictionary(&mut self, entries: &[WordReplacement]) -> Result<()> {
        dictionary::validate(entries)?;
        if self.dictionary != entries {
            self.dictionary = entries.to_vec();
        }
        Ok(())
    }
    pub fn alive(&self) -> bool {
        true
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
        let phonemes = self
            .frontend
            .phonemes(&spoken, self.model.british(voice))?;
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
        let ids = phonemes
            .iter()
            .map(|code| self.tokens.get(code).copied())
            .collect::<Option<Vec<_>>>()
            .context("Pronunciation produced an unknown phone")?;
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
        let space = self.tokens.get(&(' ' as i32)).copied();
        let mut remaining = &ids[..];
        let mut success = 1;
        while !remaining.is_empty() && success != 0 && !state.canceled {
            // Rare expanded text can exceed one graph call: split at the last word break.
            let mut end = remaining.len().min(MAX_TOKENS);
            if end < remaining.len()
                && let Some(at) = remaining[..end].iter().rposition(|id| Some(*id) == space)
                && at > 0
            {
                end = at;
            }
            let mut input = Vec::with_capacity(end + 2);
            input.push(0);
            input.extend_from_slice(&remaining[..end]);
            input.push(0);
            remaining = &remaining[end..];
            while remaining.first().is_some_and(|id| Some(*id) == space) {
                remaining = &remaining[1..];
            }
            let mut sample_count = 0;
            // Generation is synchronous. The callback context and its borrowed data cannot
            // outlive this call; Speech is thread-bound and exclusively borrowed.
            success = unsafe {
                it_speech_generate(
                    self.engine.as_ptr(),
                    input.as_ptr(),
                    input.len() as i32,
                    voice as i32,
                    speed,
                    receive::<F>,
                    (&mut state as *mut CallbackState<F>).cast(),
                    &mut sample_count,
                )
            };
        }
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
        unsafe { it_speech_destroy(self.engine.as_ptr()) };
    }
}
