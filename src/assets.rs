use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Paradee v1.2 is the only engine for now. The enum keeps a single place to add models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechModel {
    Paradee,
}
impl SpeechModel {
    pub fn filename(self) -> &'static str {
        match self {
            // The user heard no difference from the 35 MB float32 file.
            Self::Paradee => "paradee_int8.onnx",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Paradee => "Paradee",
        }
    }
    /// Inference threads. On an M4, a 68-character sentence reached first audio in
    /// 100 ms with one thread, 63 ms with two and 49 ms with four.
    pub fn threads(self) -> u32 {
        match self {
            Self::Paradee => 2,
        }
    }
    /// Voices in the graph's order: af_heart, bf_isabella, am_puck, bm_fable.
    pub fn speakers(self) -> u32 {
        match self {
            Self::Paradee => 4,
        }
    }
    /// British voices read with British pronunciations, as in training.
    pub fn british(self, speaker: u32) -> bool {
        match self {
            Self::Paradee => matches!(speaker, 1 | 3),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AssetPaths {
    pub model: SpeechModel,
    pub speech: PathBuf,
    pub library: PathBuf,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn manifest(model: SpeechModel) -> Vec<Asset> {
    let data = match model {
        SpeechModel::Paradee => include_str!("../resources/paradee-manifest.json"),
    };
    serde_json::from_str(data).expect("embedded speech manifest is valid")
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("macOS home directory"))
        .join("Library/Application Support/InfoTransfer")
}

impl AssetPaths {
    pub fn discover() -> Result<Self> {
        Self::discover_model(SpeechModel::Paradee)
    }

    pub fn discover_model(model: SpeechModel) -> Result<Self> {
        let executable = std::env::current_exe()?;
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let development = executable
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|name| name != "MacOS");
        let resources = if development {
            project.join("resources")
        } else {
            executable
                .parent()
                .context("application executable directory")?
                .join("../Resources")
        };
        let speech = std::env::var_os("INFOTRANSFER_SPEECH_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if development {
                    resources.join(match model {
                        SpeechModel::Paradee => "speech-paradee-v1",
                    })
                } else {
                    data_dir().join(match model {
                        SpeechModel::Paradee => "speech-paradee-v1",
                    })
                }
            });
        if !development {
            restore_interrupted(&speech);
        }
        let runtime = resources.join("runtime");
        let library =
            find_library(&runtime).unwrap_or_else(|| runtime.join("libonnxruntime.dylib"));
        Ok(Self {
            model,
            speech,
            library,
        })
    }

    pub fn ready(&self) -> bool {
        self.library.is_file()
            && GENERATED
                .iter()
                .map(|(name, _)| *name)
                .chain([self.model.filename()])
                .all(|name| self.speech.join(name).is_file())
    }

    pub fn verify_active_assets(&self) -> Result<()> {
        let entries = manifest(self.model);
        let expected = entries
            .iter()
            .filter(|asset| asset.name == self.model.filename())
            .map(|asset| (asset.name.as_str(), asset.sha256.as_str()))
            .chain(GENERATED);
        for (name, hash) in expected {
            let (actual, _) = hash_file(&self.speech.join(name))?;
            ensure!(
                actual == hash,
                "Speech assets failed verification. Repair voices in settings."
            );
        }
        Ok(())
    }
}

/// Files built locally during setup from the pinned downloads, with their expected hashes.
const GENERATED: [(&str, &str); 3] = [
    (
        "tokens.txt",
        "6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425",
    ),
    (
        "lexicon-us-en.txt",
        "1a417ecd9d2044fc564fc30861ea6d5a4ce555147e34bfddc0440c492c617177",
    ),
    (
        "lexicon-gb-en.txt",
        "04a7621a5bd02915002e9847f2e9a291038cdabcef52d602e933e53422b065b4",
    ),
];

fn find_library(directory: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name == "libonnxruntime.dylib")
        {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_library(&path)
        {
            return Some(found);
        }
    }
    None
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        size += n as u64;
    }
    Ok((hex::encode(hash.finalize()), size))
}

pub fn prepare(
    model: SpeechModel,
    destination: &Path,
    mut progress: impl FnMut(&str, u64, u64) -> bool,
) -> Result<()> {
    let entries = manifest(model);
    let total = entries.iter().map(|asset| asset.bytes).sum();
    ensure!(
        total <= 200_000_000,
        "Speech manifest exceeds the accepted download budget"
    );
    restore_interrupted(destination);
    let staging = destination.with_extension("download");
    fs::create_dir_all(&staging)?;
    // Staging holds a copy of every file plus the generated lexicons.
    let needed = total * 3;
    ensure!(
        free_space(&staging).is_none_or(|free| free >= needed),
        "Not enough disk space. Free at least {} MB, then try again.",
        needed.div_ceil(1_000_000)
    );
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // Applies to each read: a stalled connection fails, a slow one continues.
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut done = 0;
    for asset in &entries {
        if !progress(&asset.name, done, total) {
            bail!("Download canceled");
        }
        let target = staging.join(&asset.name);
        let existing = destination.join(&asset.name);
        if !target.is_file() && existing.is_file() {
            let (hash, size) = hash_file(&existing)?;
            if hash == asset.sha256 && size == asset.bytes {
                fs::copy(&existing, &target)?;
            }
        }
        let verified = target.is_file()
            && hash_file(&target)
                .is_ok_and(|(hash, size)| hash == asset.sha256 && size == asset.bytes);
        if !verified {
            download(&client, asset, &target, |received| {
                progress(&asset.name, done + received, total)
            })?;
        }
        done += asset.bytes;
    }
    for dialect in ["us", "gb"] {
        generate_lexicon(&staging, dialect)?;
    }
    generate_tokens(&staging)?;
    ensure!(progress("Finishing", total, total), "Download canceled");
    fs::write(
        staging.join("PROVENANCE.txt"),
        include_str!("../resources/SPEECH-NOTICES.txt"),
    )?;
    fs::write(
        staging.join("LICENSE-APACHE-2.0"),
        include_str!("../vendor/sherpa-onnx/LICENSE"),
    )?;
    if destination.exists() {
        // One atomic exchange: the new files are active, and the old ones are in staging.
        swap(&staging, destination)?;
    } else {
        fs::rename(&staging, destination)?;
    }
    // The new voice is active; a leftover old copy is harmless and replaced next time.
    let _ = fs::remove_dir_all(&staging);
    progress("Ready", total, total);
    Ok(())
}

/// Downloads one asset to `target`, resuming a partial file when the server allows it.
fn download(
    client: &reqwest::blocking::Client,
    asset: &Asset,
    target: &Path,
    mut progress: impl FnMut(u64) -> bool,
) -> Result<()> {
    let partial = target.with_extension("part");
    let mut hash = Sha256::new();
    let mut received = 0u64;
    if let Ok(mut existing) = fs::File::open(&partial) {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = existing.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
            received += n as u64;
        }
    }
    if received > asset.bytes {
        (hash, received) = (Sha256::new(), 0);
    }
    if received < asset.bytes {
        let mut request = client.get(&asset.url);
        if received > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={received}-"));
        }
        let mut response = request.send()?.error_for_status()?;
        if received > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            (hash, received) = (Sha256::new(), 0);
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(received > 0)
            .truncate(received == 0)
            .open(&partial)?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if !progress(received) {
                // The partial file stays, so the next attempt resumes from here.
                file.sync_all()?;
                bail!("Download canceled");
            }
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            received += n as u64;
            ensure!(received <= asset.bytes, "Unexpected speech asset size");
            hash.update(&buffer[..n]);
            file.write_all(&buffer[..n])?;
        }
        file.sync_all()?;
    }
    if received != asset.bytes || hex::encode(hash.finalize()) != asset.sha256 {
        let _ = fs::remove_file(&partial);
        bail!("Speech asset integrity check failed: {}", asset.name);
    }
    fs::rename(partial, target)?;
    Ok(())
}

fn swap(new: &Path, old: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(new.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(old.as_os_str().as_bytes())?;
    // Both paths are valid C strings in the same folder; RENAME_SWAP is atomic.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
    ensure!(
        status == 0,
        "Voices could not be replaced: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

/// Earlier versions moved the active voice aside before installing the new one. If that
/// was interrupted, only the moved-aside copy exists; put it back.
fn restore_interrupted(destination: &Path) {
    let backup = destination.with_extension("previous");
    if !destination.exists() && backup.is_dir() {
        let _ = fs::rename(&backup, destination);
    } else if backup.is_dir() {
        let _ = fs::remove_dir_all(&backup);
    }
}

fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // statvfs fills the struct on success and leaves it untouched on failure.
    (unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } == 0).then(|| {
        let stats = unsafe { stats.assume_init() };
        stats.f_bavail as u64 * stats.f_frsize
    })
}

/// The phone alphabet from the model's config, one "symbol id" pair per line.
fn generate_tokens(directory: &Path) -> Result<()> {
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("config.json"))?)?;
    let vocab = config["vocab"]
        .as_object()
        .context("Speech model config has no vocabulary")?;
    let mut entries = vocab
        .iter()
        .map(|(symbol, id)| Ok((id.as_u64().context("Invalid token id")?, symbol)))
        .collect::<Result<Vec<_>>>()?;
    entries.sort();
    let mut output = String::new();
    for (id, symbol) in entries {
        output.push_str(&format!("{symbol} {id}\n"));
    }
    fs::write(directory.join("tokens.txt"), output)?;
    Ok(())
}

fn generate_lexicon(directory: &Path, dialect: &str) -> Result<()> {
    let mut words: BTreeMap<String, serde_json::Value> =
        serde_json::from_slice(&fs::read(directory.join(format!("{dialect}_silver.json")))?)?;
    let gold: BTreeMap<String, serde_json::Value> =
        serde_json::from_slice(&fs::read(directory.join(format!("{dialect}_gold.json")))?)?;
    words.extend(gold);
    let mut output = std::io::BufWriter::new(fs::File::create(
        directory.join(format!("lexicon-{dialect}-en.txt")),
    )?);
    let mut normalized = BTreeMap::<String, String>::new();
    for (word, value) in words {
        let phones = value
            .as_str()
            .or_else(|| value.get("DEFAULT").and_then(|value| value.as_str()))
            .context("Invalid pronunciation entry")?;
        normalized.insert(word.to_lowercase(), phones.to_owned());
    }
    normalized.insert("kokoro".into(), "kˈOkəɹO".into());
    normalized.insert("misaki".into(), "misˈɑki".into());
    for (word, phones) in normalized {
        write!(output, "{word}")?;
        for phone in phones.chars() {
            write!(output, " {phone}")?;
        }
        writeln!(output)?;
    }
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupted_install_restores_the_previous_voice() {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("speech-paradee-v1");
        let backup = active.with_extension("previous");
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join("model.onnx"), b"voice").unwrap();
        restore_interrupted(&active);
        assert_eq!(fs::read(active.join("model.onnx")).unwrap(), b"voice");
        assert!(!backup.exists());
    }
    #[test]
    fn replacing_voices_swaps_both_folders_at_once() {
        let root = tempfile::tempdir().unwrap();
        let (new, old) = (root.path().join("new"), root.path().join("old"));
        fs::create_dir(&new).unwrap();
        fs::create_dir(&old).unwrap();
        fs::write(new.join("version"), b"2").unwrap();
        fs::write(old.join("version"), b"1").unwrap();
        swap(&new, &old).unwrap();
        assert_eq!(fs::read(old.join("version")).unwrap(), b"2");
        assert_eq!(fs::read(new.join("version")).unwrap(), b"1");
        assert!(free_space(root.path()).is_some_and(|free| free > 0));
    }
}
