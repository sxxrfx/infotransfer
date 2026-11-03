use anyhow::{Context, Result};
use infotransfer::{
    assets::{self, AssetPaths, SpeechModel},
    speech::Speech,
    text::Chunks,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let value = |key: &str, default: &str| {
        args.iter()
            .position(|arg| arg == key)
            .and_then(|index| args.get(index + 1))
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let model = match value("--engine", "paradee").as_str() {
        "paradee" => SpeechModel::Paradee,
        other => anyhow::bail!("Unsupported engine: {other}"),
    };
    let paths = AssetPaths::discover_model(model)?;
    if args.iter().any(|arg| arg == "--prepare") {
        let mut last = 0;
        assets::prepare(model, &paths.speech, |name, done, total| {
            let percent = done * 100 / total;
            if percent >= last + 10 {
                eprintln!("Assets: {percent}% ({name})");
                last = percent;
            }
            true
        })?;
        println!("{}", json!({"event":"assets_ready","path":paths.speech}));
        return Ok(());
    }
    let threads = value("--threads", &model.threads().to_string()).parse::<u32>()?;
    if let Some(index) = args.iter().position(|arg| arg == "--phonemes") {
        let text = args.get(index + 1).context("--phonemes needs text")?;
        let voice = value("--voice", "0").parse::<u32>()?;
        let mut engine = Speech::load(&paths, threads)?;
        for chunk in Chunks::new(text) {
            println!("{}", engine.pronunciation(chunk, voice)?);
        }
        return Ok(());
    }
    let runs = value("--runs", "5").parse::<usize>()?;
    let speed = value("--speed", "1").parse::<f32>()?;
    let voice = value("--voice", "0").parse::<u32>()?;
    let bucket = value("--bucket", "typical");
    let text=match bucket.as_str(){
        "short"=>"Hello.".to_owned(),
        "long"=>"Reading at your own pace can make a long paragraph easier to follow. ".repeat(200),
        "unpunctuated"=>"Reading at your own pace can make a long paragraph easier to follow ".repeat(200),
        _=>"Reading at your own pace can make a long paragraph easier to follow. You can pause whenever you need.".to_owned(),
    };
    let started = Instant::now();
    let mut engine = Speech::load(&paths, threads).context("load local speech engine")?;
    println!(
        "{}",
        json!({"event":"loaded","load_ms":started.elapsed().as_secs_f64()*1000.,"threads":threads,"sample_rate":engine.sample_rate,"voices":engine.voices,"method":"model_callback_not_audible_output"})
    );
    for run in 0..runs {
        let default_input = Chunks::new(&text).next().context("benchmark input")?;
        let input = if let Some(index) = args.iter().position(|arg| arg == "--words") {
            let count = args
                .get(index + 1)
                .context("--words needs a count")?
                .parse::<usize>()?;
            let end = text
                .split_whitespace()
                .take(count)
                .collect::<Vec<_>>()
                .join(" ")
                .len();
            &text[..end.min(text.len())]
        } else {
            default_input
        };
        let checksum = args.iter().any(|arg| arg == "--audio-sha256");
        let mut audio_hash = Sha256::new();
        let result = engine.generate(input, voice, speed, |samples| {
            if checksum {
                for sample in samples {
                    audio_hash.update(sample.to_le_bytes());
                }
            }
            true
        });
        let audio_digest = checksum.then(|| hex::encode(audio_hash.finalize()));
        match result {
            Ok(g) => println!(
                "{}",
                json!({"event":"generation","engine":model.label(),"phase":if run==0 {"first"} else {"warm"},"run":run,"bucket":bucket,"characters":input.chars().count(),"threads":threads,"voice":voice,"speed":speed,"first_pcm_ms":g.first_audio.map(|t|t.as_secs_f64()*1000.),"audio_sha256":audio_digest,"total_ms":g.elapsed.as_secs_f64()*1000.,"samples":g.samples,"callbacks":g.callbacks,"peak":g.peak,"success":!g.canceled})
            ),
            Err(error) => println!(
                "{}",
                json!({"event":"generation","engine":model.label(),"phase":if run==0 {"first"} else {"warm"},"run":run,"success":false,"error":error.to_string()})
            ),
        }
    }
    Ok(())
}
