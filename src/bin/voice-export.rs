//! Full recordings through the production frontend and chunk policy; no audio device.
use anyhow::{Context, Result, ensure};
use infotransfer::{
    assets::{AssetPaths, SpeechModel},
    sound::{SoundSettings, SpeechAudio},
    speech::Speech,
    text::Chunks,
};
use serde::Deserialize;
use serde_json::json;
use std::{fs, io::Write, path::Path, time::Instant};

#[derive(Deserialize)]
struct Passage {
    id: String,
    text: String,
}

fn wav(path: &Path, samples: &[f32]) -> Result<()> {
    let bytes = u32::try_from(samples.len() * 2)?;
    let mut file = fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&24_000u32.to_le_bytes())?;
    file.write_all(&48_000u32.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for sample in samples {
        file.write_all(&((sample.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes())?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let value = |key: &str, default: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
            .map_or(default.to_owned(), Clone::clone)
    };
    let mut model = match value("--engine", "kitten-cpu").as_str() {
        "kitten-cpu" => SpeechModel::KittenCpu,
        "kokoro-heart" => SpeechModel::KokoroHeart,
        "qwen3" => SpeechModel::Qwen3,
        "voxcpm2" => SpeechModel::VoxCpm2,
        "cosyvoice3" => SpeechModel::CosyVoice3,
        "sopro" => SpeechModel::Sopro,
        "phonon" => SpeechModel::Phonon,
        other => anyhow::bail!("Unsupported engine: {other}"),
    };
    let mut speaker = value("--voice", "0").parse()?;
    let catalog_voice = if let Some(index) = args.iter().position(|arg| arg == "--voice-id") {
        let id: usize = args
            .get(index + 1)
            .context("Voice ID is missing")?
            .parse()?;
        let voice = infotransfer::voices::VOICES
            .get(id)
            .context("Unknown catalog voice")?;
        model = voice.model;
        speaker = voice.speaker;
        Some(voice.name)
    } else {
        None
    };
    let speed = value("--speed", "1").parse()?;
    let output = value("--output", ".cache/voice-eval/audio");
    let prefix = value("--prefix", "voice");
    ensure!(
        prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "Invalid prefix"
    );
    fs::create_dir_all(&output)?;
    let corpus: Vec<Passage> = serde_json::from_slice(&fs::read(value(
        "--corpus",
        "docs/performance/voice-corpus.json",
    ))?)?;
    let load = Instant::now();
    let mut engine = Speech::load(&AssetPaths::discover_model(model)?, 2)?;
    let dictionary = if let Some(index) = args.iter().position(|arg| arg == "--dictionary") {
        let path = args.get(index + 1).context("Dictionary file is missing")?;
        serde_json::from_slice::<Vec<infotransfer::dictionary::WordReplacement>>(&fs::read(path)?)?
    } else {
        infotransfer::dictionary::defaults()
    };
    engine.set_dictionary(&dictionary)?;
    println!(
        "{}",
        json!({"event":"loaded","engine":model.label(),"voice_name":catalog_voice,"load_ms":load.elapsed().as_secs_f64()*1000.})
    );
    for passage in corpus {
        ensure!(
            passage
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Invalid passage id"
        );
        for control in [false, true] {
            if control && !["prose", "paragraph"].contains(&passage.id.as_str()) {
                continue;
            }
            let id = format!(
                "{prefix}-{}{}",
                passage.id,
                if control { "-whole" } else { "" }
            );
            let started = Instant::now();
            let mut samples = Vec::new();
            let mut times = Vec::new();
            let mut processed = Vec::new();
            let mut audio = SpeechAudio::new(24_000, SoundSettings::default());
            let mut pauses = Vec::new();
            let chunks = if control {
                vec![passage.text.as_str()]
            } else {
                Chunks::new(&passage.text).collect()
            };
            let result: Result<()> = (|| {
                let mut policy = Chunks::new(&passage.text);
                for (index, chunk) in chunks.iter().enumerate() {
                    if !control {
                        policy.next();
                    }
                    let generated = engine.generate(chunk, speaker, speed, |pcm| {
                        audio.process(pcm, &mut processed);
                        samples.extend_from_slice(&processed);
                        true
                    })?;
                    ensure!(!generated.canceled, "Generation canceled");
                    times.push(generated.elapsed.as_secs_f64() * 1000.);
                    audio.finish_chunk(
                        policy.boundary(),
                        index + 1 == chunks.len(),
                        &mut processed,
                    );
                    samples.extend_from_slice(&processed);
                    if index + 1 < chunks.len() {
                        let count = audio.pause_samples(policy.boundary());
                        pauses.push(json!({"after_chunk": index, "start_sample": samples.len(), "inserted_samples": count, "boundary": format!("{:?}", policy.boundary())}));
                        samples.resize(samples.len() + count, 0.);
                    }
                }
                ensure!(
                    !samples.is_empty() && samples.iter().all(|s| s.is_finite()),
                    "Invalid audio"
                );
                Ok(())
            })();
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            let spoken: Vec<String> = chunks
                .iter()
                .map(|chunk| infotransfer::spoken::prepare(chunk, &dictionary))
                .collect::<Result<_>>()?;
            let clipped = samples.iter().filter(|s| s.abs() >= 1.).count();
            let peak = samples.iter().map(|s| s.abs()).fold(0f32, f32::max);
            let file = Path::new(&output).join(format!("{id}.wav"));
            if result.is_ok() {
                wav(&file, &samples).context("Write evaluation WAV")?;
            }
            println!(
                "{}",
                json!({"event":"recording","id":id,"passage":passage.id,"text":passage.text,"spoken":spoken,"whole_control":control,"success":result.is_ok(),"error":result.err().map(|e|e.to_string()),"wav":file,"engine":model.label(),"speaker":speaker,"speed":speed,"chunks":chunks,"pauses":pauses,"sound":SoundSettings::default(),"chunk_ms":times,"total_ms":elapsed,"samples":samples.len(),"duration_s":samples.len() as f64/24000.,"peak":peak,"clipped_samples":clipped})
            );
        }
    }
    Ok(())
}
