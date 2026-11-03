//! Explicitly registered local development runtimes; no network or shell invocation.
use crate::{assets::data_dir, speech::Generation};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize)]
pub struct LocalRuntime {
    pub python: PathBuf,
    pub script: PathBuf,
    pub model_dir: PathBuf,
    pub artifacts: Vec<RuntimeArtifact>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeArtifact {
    pub path: PathBuf,
    pub sha256: String,
}
impl LocalRuntime {
    pub fn discover(key: &str) -> Option<Self> {
        if !cfg!(feature = "experimental-voices") {
            return None;
        }
        let bytes = fs::read(data_dir().join("experimental-runtimes.json")).ok()?;
        let mut entries: BTreeMap<String, Self> = serde_json::from_slice(&bytes).ok()?;
        entries.remove(key)
    }
    pub fn ready(&self) -> bool {
        self.python.is_file()
            && self.script.is_file()
            && self.model_dir.is_dir()
            && !self.artifacts.is_empty()
            && self.artifacts.iter().all(|a| a.path.is_file())
    }
    pub fn verify(&self) -> Result<()> {
        for artifact in &self.artifacts {
            let mut file = fs::File::open(&artifact.path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            ensure!(
                hex::encode(hash.finalize()) == artifact.sha256,
                "Local model files changed. Run the model setup again."
            );
        }
        Ok(())
    }
}

pub struct LocalSpeech {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    pub sample_rate: u32,
    alive: bool,
}
#[derive(Deserialize)]
struct Frame {
    event: String,
    rate: Option<u32>,
    samples: Option<usize>,
    message: Option<String>,
}
impl LocalSpeech {
    pub fn load(runtime: &LocalRuntime, key: &str) -> Result<Self> {
        Self::load_cancellable(runtime, key, || false)
    }
    pub fn load_cancellable(
        runtime: &LocalRuntime,
        key: &str,
        cancel: impl Fn() -> bool,
    ) -> Result<Self> {
        runtime.verify()?;
        let mut child = Command::new(&runtime.python)
            .arg("-u")
            .arg(&runtime.script)
            .arg(key)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Cannot start the local model runtime")?;
        let input = child.stdin.take().context("Model input unavailable")?;
        let output = BufReader::new(child.stdout.take().context("Model output unavailable")?);
        let mut model = Self {
            child,
            input,
            output,
            sample_rate: 24000,
            alive: true,
        };
        let frame = model
            .frame(&cancel, Instant::now() + Duration::from_secs(120))?
            .context("Model loading canceled")?;
        ensure!(frame.event == "ready", "Local model did not become ready");
        model.sample_rate = frame.rate.context("Local model has no sample rate")?;
        ensure!(
            (16000..=48000).contains(&model.sample_rate),
            "Unsupported local model sample rate"
        );
        Ok(model)
    }
    pub fn alive(&self) -> bool {
        self.alive
    }
    fn canceled(&mut self) {
        self.alive = false;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn read(
        &mut self,
        bytes: &mut [u8],
        cancel: &impl Fn() -> bool,
        deadline: Instant,
    ) -> Result<bool> {
        let mut offset = 0;
        while offset < bytes.len() {
            if cancel() {
                self.canceled();
                return Ok(false);
            }
            ensure!(Instant::now() < deadline, "Local model timed out");
            if self.output.buffer().is_empty() {
                let mut fd = libc::pollfd {
                    fd: self.output.get_ref().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // Only this speech worker owns the pipe. Poll keeps Esc/replacement responsive during inference.
                let ready = unsafe { libc::poll(&mut fd, 1, 20) };
                if ready == 0 {
                    continue;
                }
                if ready < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            let n = self.output.read(&mut bytes[offset..])?;
            ensure!(n > 0, "Local model stopped unexpectedly");
            offset += n;
        }
        Ok(true)
    }
    fn frame(&mut self, cancel: &impl Fn() -> bool, deadline: Instant) -> Result<Option<Frame>> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0];
            if !self.read(&mut byte, cancel, deadline)? {
                return Ok(None);
            }
            if byte[0] == b'\n' {
                break;
            }
            ensure!(line.len() < 16384, "Invalid local model frame");
            line.push(byte[0]);
        }
        Ok(Some(
            serde_json::from_slice(&line).context("Invalid local model response")?,
        ))
    }
    pub fn generate(
        &mut self,
        text: &str,
        speed: f32,
        callback: impl FnMut(&[f32]) -> bool,
        cancel: impl Fn() -> bool,
    ) -> Result<Generation> {
        let result = self.generate_inner(text, speed, callback, cancel);
        if result.is_err() {
            self.canceled();
        }
        result
    }
    fn generate_inner(
        &mut self,
        text: &str,
        speed: f32,
        mut callback: impl FnMut(&[f32]) -> bool,
        cancel: impl Fn() -> bool,
    ) -> Result<Generation> {
        ensure!(self.alive, "Local model was canceled; retry reading");
        serde_json::to_writer(&mut self.input, &serde_json::json!({"text":text}))?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let start = Instant::now();
        let deadline = start + Duration::from_secs(120);
        let mut stats = Generation {
            first_audio: None,
            elapsed: Duration::ZERO,
            samples: 0,
            callbacks: 0,
            peak: 0.,
            canceled: false,
        };
        let mut tempo = Tempo::new(self.sample_rate, speed)?;
        let mut converted = Vec::new();
        loop {
            let Some(frame) = self.frame(&cancel, deadline)? else {
                stats.canceled = true;
                break;
            };
            match frame.event.as_str() {
                "pcm" => {
                    let count = frame.samples.context("Missing PCM size")?;
                    ensure!((1..=48_000 * 120).contains(&count), "Invalid PCM size");
                    let mut bytes = vec![0; count * 4];
                    if !self.read(&mut bytes, &cancel, deadline)? {
                        stats.canceled = true;
                        break;
                    }
                    let pcm: Vec<f32> = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|v| f32::from_le_bytes(*v))
                        .collect();
                    ensure!(pcm.iter().all(|s| s.is_finite()), "Non-finite model audio");
                    tempo.process(&pcm, false, &mut converted)?;
                    let peak = converted.iter().fold(0f32, |v, s| v.max(s.abs()));
                    stats.peak = stats.peak.max(peak);
                    stats.samples += converted.len();
                    stats.callbacks += 1;
                    if stats.first_audio.is_none() && peak > 0.00001 {
                        stats.first_audio = Some(start.elapsed());
                    }
                    if !converted.is_empty() && !callback(&converted) {
                        self.canceled();
                        stats.canceled = true;
                        break;
                    }
                }
                "done" => {
                    tempo.process(&[], true, &mut converted)?;
                    stats.samples += converted.len();
                    let peak = converted.iter().fold(0f32, |v, s| v.max(s.abs()));
                    stats.peak = stats.peak.max(peak);
                    if stats.first_audio.is_none() && peak > 0.00001 {
                        stats.first_audio = Some(start.elapsed());
                    }
                    if !converted.is_empty() && !callback(&converted) {
                        stats.canceled = true;
                    }
                    break;
                }
                "error" => bail!(
                    frame
                        .message
                        .unwrap_or_else(|| "Local speech failed".into())
                ),
                _ => bail!("Unexpected local model response"),
            }
        }
        stats.elapsed = start.elapsed();
        ensure!(
            stats.canceled || (stats.samples > 0 && stats.peak > 0.00001),
            "Local model produced no audible audio"
        );
        Ok(stats)
    }
}
impl Drop for LocalSpeech {
    fn drop(&mut self) {
        self.canceled();
    }
}

unsafe extern "C" {
    fn sonicCreateStream(sample_rate: i32, channels: i32) -> *mut std::ffi::c_void;
    fn sonicDestroyStream(stream: *mut std::ffi::c_void);
    fn sonicSetSpeed(stream: *mut std::ffi::c_void, speed: f32);
    fn sonicSetRate(stream: *mut std::ffi::c_void, rate: f32);
    fn sonicSetQuality(stream: *mut std::ffi::c_void, quality: i32);
    fn sonicWriteFloatToStream(
        stream: *mut std::ffi::c_void,
        samples: *const f32,
        count: i32,
    ) -> i32;
    fn sonicReadFloatFromStream(
        stream: *mut std::ffi::c_void,
        samples: *mut f32,
        count: i32,
    ) -> i32;
    fn sonicFlushStream(stream: *mut std::ffi::c_void) -> i32;
}
struct Tempo {
    stream: std::ptr::NonNull<std::ffi::c_void>,
}
impl Tempo {
    fn new(rate: u32, speed: f32) -> Result<Self> {
        // Each synchronous generation owns this stream; no state is shared with the audio callback.
        let stream = std::ptr::NonNull::new(unsafe { sonicCreateStream(rate as i32, 1) })
            .context("Cannot create speech tempo processor")?;
        unsafe {
            sonicSetQuality(stream.as_ptr(), 1);
            sonicSetSpeed(stream.as_ptr(), speed);
            sonicSetRate(stream.as_ptr(), rate as f32 / 24000.);
        }
        Ok(Self { stream })
    }
    fn process(&mut self, input: &[f32], finish: bool, output: &mut Vec<f32>) -> Result<()> {
        output.clear();
        let count = i32::try_from(input.len())?;
        let stream = self.stream.as_ptr();
        // Input and scratch buffers live through each synchronous Sonic call.
        let success = unsafe {
            if finish {
                sonicFlushStream(stream)
            } else {
                sonicWriteFloatToStream(stream, input.as_ptr(), count)
            }
        };
        ensure!(success != 0, "Speech tempo processing failed");
        let mut block = [0.; 4096];
        loop {
            let n =
                unsafe { sonicReadFloatFromStream(stream, block.as_mut_ptr(), block.len() as i32) };
            ensure!(n >= 0, "Invalid tempo output");
            if n == 0 {
                break;
            }
            output.extend_from_slice(&block[..n as usize]);
        }
        Ok(())
    }
}
impl Drop for Tempo {
    fn drop(&mut self) {
        unsafe { sonicDestroyStream(self.stream.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_tempo_preserves_pitch_and_converts_48k_to_24k() {
        let input: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 * std::f32::consts::TAU * 220. / 48_000.).sin() * 0.3)
            .collect();
        for speed in [0.75, 1., 1.5, 2.] {
            let mut tempo = Tempo::new(48_000, speed).unwrap();
            let mut part = Vec::new();
            let mut output = Vec::new();
            for block in input.chunks(997) {
                tempo.process(block, false, &mut part).unwrap();
                output.extend_from_slice(&part);
            }
            tempo.process(&[], true, &mut part).unwrap();
            output.extend_from_slice(&part);
            assert!(
                (output.len() as f32 - 24_000. / speed).abs() < 500.,
                "duration at {speed}: {}",
                output.len()
            );
            let mid = &output[1000..output.len() - 1000];
            let crossings = mid
                .windows(2)
                .filter(|pair| pair[0] <= 0. && pair[1] > 0.)
                .count();
            let frequency = crossings as f32 * 24_000. / mid.len() as f32;
            assert!(
                (frequency - 220.).abs() < 5.,
                "pitch at {speed}: {frequency}"
            );
        }
    }
}
