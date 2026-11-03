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
    let staging = destination.with_extension("download");
    fs::create_dir_all(&staging)?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // A large model on a slow connection can legitimately take a long time.
        .timeout(Duration::from_secs(3600))
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
            let partial = target.with_extension("part");
            let mut response = client.get(&asset.url).send()?.error_for_status()?;
            let mut file = fs::File::create(&partial)?;
            let mut hash = Sha256::new();
            let mut received = 0u64;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                if !progress(&asset.name, done + received, total) {
                    drop(file);
                    let _ = fs::remove_file(&partial);
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
            ensure!(
                received == asset.bytes && hex::encode(hash.finalize()) == asset.sha256,
                "Speech asset integrity check failed: {}",
                asset.name
            );
            fs::rename(partial, &target)?;
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
    let backup = destination.with_extension("previous");
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if destination.exists() {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(&staging, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    progress("Ready", total, total);
    Ok(())
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
