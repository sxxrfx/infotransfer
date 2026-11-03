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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechModel {
    Kokoro,
    KokoroHeart,
    Kitten,
    KittenCpu,
    Qwen3,
    VoxCpm2,
    CosyVoice3,
    Sopro,
    Phonon,
}
impl SpeechModel {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Kokoro | Self::KokoroHeart => "model.int8.onnx",
            Self::Kitten => "model.fp16.onnx",
            Self::KittenCpu => "model.cpu.onnx",
            Self::Phonon => "model.q8.gguf",
            Self::Qwen3 | Self::VoxCpm2 | Self::CosyVoice3 | Self::Sopro => "model.safetensors",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Kokoro => "Kokoro",
            Self::KokoroHeart => "Kokoro Heart",
            Self::Kitten => "Kitten",
            Self::KittenCpu => "Kitten CPU",
            Self::Qwen3 => "Qwen3 Ryan",
            Self::VoxCpm2 => "VoxCPM2",
            Self::CosyVoice3 => "CosyVoice3",
            Self::Sopro => "Sopro v2 Turbo",
            Self::Phonon => "Phonon 31M",
        }
    }
    pub fn native_kind(self) -> i32 {
        match self {
            Self::Kokoro | Self::KokoroHeart => 0,
            Self::Kitten | Self::KittenCpu => 1,
            _ => unreachable!("Local models use their registered runtime"),
        }
    }
    pub fn local_key(self) -> Option<&'static str> {
        match self {
            Self::Qwen3 => Some("qwen3"),
            Self::VoxCpm2 => Some("voxcpm2"),
            Self::CosyVoice3 => Some("cosyvoice3"),
            Self::Sopro => Some("sopro"),
            Self::Phonon => Some("phonon"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AssetPaths {
    pub model: SpeechModel,
    pub speech: PathBuf,
    pub library: PathBuf,
    pub local: Option<crate::advanced::LocalRuntime>,
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
        SpeechModel::Kokoro => include_str!("../resources/speech-manifest.json"),
        SpeechModel::KokoroHeart => include_str!("../resources/kokoro-heart-manifest.json"),
        SpeechModel::Kitten | SpeechModel::KittenCpu => {
            include_str!("../resources/kitten-manifest.json")
        }
        _ => return Vec::new(),
    };
    serde_json::from_str(data).expect("embedded speech manifest is valid")
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("macOS home directory"))
        .join("Library/Application Support/InfoTransfer")
}

impl AssetPaths {
    pub fn discover() -> Result<Self> {
        Self::discover_model(SpeechModel::KittenCpu)
    }

    pub fn discover_model(model: SpeechModel) -> Result<Self> {
        if let Some(key) = model.local_key() {
            let local = crate::advanced::LocalRuntime::discover(key);
            return Ok(Self {
                model,
                speech: local
                    .as_ref()
                    .map(|r| r.model_dir.clone())
                    .unwrap_or_else(|| data_dir().join(key)),
                library: local.as_ref().map(|r| r.python.clone()).unwrap_or_default(),
                local,
            });
        }
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
                        SpeechModel::Kokoro => "speech",
                        SpeechModel::KokoroHeart => "speech-heart",
                        SpeechModel::Kitten => "speech-kitten",
                        SpeechModel::KittenCpu => "speech-kitten-cpu",
                        _ => unreachable!(),
                    })
                } else {
                    data_dir().join(match model {
                        SpeechModel::Kokoro => "speech-kokoro-v1",
                        SpeechModel::KokoroHeart => "speech-heart-v1",
                        SpeechModel::Kitten => "speech-kitten-v1",
                        SpeechModel::KittenCpu => "speech-kitten-cpu-v1",
                        _ => unreachable!(),
                    })
                }
            });
        let runtime = resources.join("runtime");
        let library =
            find_library(&runtime).unwrap_or_else(|| runtime.join("libsherpa-onnx-c-api.dylib"));
        Ok(Self {
            model,
            speech,
            library,
            local: None,
        })
    }

    pub fn ready(&self) -> bool {
        if self.model.local_key().is_some() {
            return self.local.as_ref().is_some_and(|r| r.ready());
        }
        self.library.is_file()
            && [
                self.model.filename(),
                "voices.bin",
                "tokens.txt",
                "lexicon-us-en.txt",
            ]
            .iter()
            .all(|name| self.speech.join(name).is_file())
    }

    pub fn verify_active_assets(&self) -> Result<()> {
        if let Some(runtime) = &self.local {
            return runtime.verify();
        }
        let entries = manifest(self.model);
        for name in [
            self.model.filename(),
            "voices.bin",
            "tokens.txt",
            "lexicon-us-en.txt",
        ] {
            let expected = if self.model == SpeechModel::KittenCpu {
                match name {
                    "model.cpu.onnx" => {
                        Some("264b48c18257cc172960c9321b233b249e215879b5d295766baeace33f5eb5b1")
                    }
                    "lexicon-us-en.txt" => {
                        Some("f2c7716d24cd2be2b27694083148b454c14c2d6b65e811898ecf1085c5b2ad06")
                    }
                    _ => entries
                        .iter()
                        .find(|asset| asset.name == name)
                        .map(|asset| asset.sha256.as_str()),
                }
            } else if self.model == SpeechModel::KokoroHeart {
                match name {
                    "lexicon-us-en.txt" => {
                        Some("1a417ecd9d2044fc564fc30861ea6d5a4ce555147e34bfddc0440c492c617177")
                    }
                    _ => entries
                        .iter()
                        .find(|asset| asset.name == name)
                        .map(|asset| asset.sha256.as_str()),
                }
            } else {
                // Experimental legacy engines are only exposed by the benchmark CLI.
                None
            };
            if let Some(expected) = expected {
                let (actual, _) = hash_file(&self.speech.join(name))?;
                ensure!(
                    actual == expected,
                    "Speech assets failed verification. Repair voices in settings."
                );
            }
        }
        Ok(())
    }
}

fn find_library(directory: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name == "libsherpa-onnx-c-api.dylib")
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
    ensure!(
        model.local_key().is_none(),
        "Use local model setup for experimental voices"
    );
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
        generate_lexicon(&staging, dialect, model)?;
    }
    if model == SpeechModel::KittenCpu {
        ensure!(
            progress("Preparing voices", total, total),
            "Download canceled"
        );
        crate::model::prepare_cpu_model(
            &staging.join("model.fp16.onnx"),
            &staging.join(model.filename()),
        )?;
    }
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

fn generate_lexicon(directory: &Path, dialect: &str, model: SpeechModel) -> Result<()> {
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
            let expanded = if matches!(model, SpeechModel::Kitten | SpeechModel::KittenCpu) {
                match phone {
                    'A' => "eɪ",
                    'I' => "aɪ",
                    'O' => "oʊ",
                    'Q' => "əʊ",
                    'W' => "aʊ",
                    'Y' => "ɔɪ",
                    'T' => "ɾ",
                    'ᵊ' => "ə",
                    _ => "",
                }
            } else {
                ""
            };
            if expanded.is_empty() {
                write!(output, " {phone}")?;
            } else {
                for component in expanded.chars() {
                    write!(output, " {component}")?;
                }
            }
        }
        writeln!(output)?;
    }
    output.flush()?;
    Ok(())
}
