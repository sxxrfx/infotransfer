//! Native pronunciation with each model's phoneme convention.
use crate::assets::{AssetPaths, SpeechModel};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

const PRONUNCIATION_TIMEOUT: Duration = Duration::from_secs(5);

struct Phonemizer {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Phonemizer {
    fn start() -> Result<Self> {
        let executable = std::env::current_exe()?;
        let bundled = executable
            .parent()
            .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "MacOS"));
        let directory: PathBuf = if bundled {
            executable.parent().unwrap().join("../Resources/phonemizer")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/phonemizer")
        };
        ensure!(
            directory.join("phonemize").is_file(),
            "Pronunciation assets are missing. Reinstall InfoTransfer."
        );
        let mut child = Command::new(directory.join("phonemize"))
            .arg(&directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not start local pronunciation")?;
        let input = child.stdin.take().context("Pronunciation input")?;
        let output = child.stdout.take().context("Pronunciation output")?;
        let mut value = Self {
            child,
            input,
            output,
        };
        let mut ready = [0u8; 4];
        value
            .output
            .read_exact(&mut ready)
            .context("Pronunciation could not load")?;
        ensure!(ready == [0; 4], "Invalid pronunciation runtime");
        Ok(value)
    }
    fn pronounce(&mut self, word: &str) -> Result<String> {
        ensure!(
            !word.is_empty() && word.len() <= 8192,
            "Word exceeds the pronunciation limit"
        );
        self.input.write_all(&(word.len() as u32).to_le_bytes())?;
        self.input.write_all(word.as_bytes())?;
        self.input.flush()?;
        let deadline = Instant::now() + PRONUNCIATION_TIMEOUT;
        let mut length = [0u8; 4];
        self.read_before(&mut length, deadline)?;
        let length = u32::from_le_bytes(length) as usize;
        ensure!(
            (1..=65536).contains(&length),
            "Invalid pronunciation response"
        );
        let mut output = vec![0u8; length];
        self.read_before(&mut output, deadline)?;
        String::from_utf8(output).context("Invalid pronunciation encoding")
    }
    fn read_before(&mut self, bytes: &mut [u8], deadline: Instant) -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "Pronunciation timed out");
            let mut fd = libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // The descriptor stays open through this call; only this worker reads it.
            let ready = unsafe { libc::poll(&mut fd, 1, remaining.as_millis().min(1000) as i32) };
            if ready < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if ready > 0 {
                let count = self.output.read(&mut bytes[offset..])?;
                ensure!(count > 0, "Pronunciation stopped");
                offset += count;
            }
        }
        Ok(())
    }
}
impl Drop for Phonemizer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct EnglishFrontend {
    lexicon: HashMap<String, Vec<i32>>,
    symbols: HashSet<char>,
    fallback: Phonemizer,
    cache: HashMap<String, Vec<i32>>,
    compact: bool,
}
impl EnglishFrontend {
    pub fn load(paths: &AssetPaths) -> Result<Self> {
        let symbols = fs::read_to_string(paths.speech.join("tokens.txt"))?
            .lines()
            .filter_map(|line| line.rsplit_once(' ').map(|(token, _)| token))
            .filter(|token| token.chars().count() == 1)
            .flat_map(str::chars)
            .collect();
        let mut lexicon = HashMap::new();
        for line in fs::read_to_string(paths.speech.join("lexicon-us-en.txt"))?.lines() {
            if let Some((word, phones)) = line.split_once(' ') {
                lexicon.insert(
                    word.to_owned(),
                    phones
                        .split_whitespace()
                        .flat_map(str::chars)
                        .map(|ch| ch as i32)
                        .collect(),
                );
            }
        }
        Ok(Self {
            lexicon,
            symbols,
            fallback: Phonemizer::start()?,
            cache: HashMap::new(),
            compact: !matches!(paths.model, SpeechModel::Kitten | SpeechModel::KittenCpu),
        })
    }
    pub fn phonemes(&mut self, text: &str) -> Result<Vec<i32>> {
        if !self.compact {
            let mut result = Vec::new();
            for clause in text.split_inclusive(['.', ',', '?', '!', '…']) {
                let mark = clause
                    .chars()
                    .last()
                    .filter(|ch| matches!(ch, '.' | ',' | '?' | '!' | '…'));
                let body = clause.trim_end_matches(['.', ',', '?', '!', '…']).trim();
                if !body.is_empty() {
                    let ipa = self.pronounce(body)?;
                    let phones = compatible(&ipa, false)
                        .trim()
                        .chars()
                        .map(|ch| ch as i32)
                        .collect::<Vec<_>>();
                    if !self.supported(&phones) {
                        return self.words(text);
                    }
                    if !result.is_empty() {
                        result.push(' ' as i32);
                    }
                    result.extend(phones);
                }
                if let Some(mark) = mark {
                    result.push(mark as i32);
                }
            }
            ensure!(
                !result.is_empty() && result.len() <= 32768,
                "No pronounceable text, or spoken replacement is too long"
            );
            return Ok(result);
        }
        self.words(text)
    }
    /// A crashed or hung helper is replaced once, so one failure cannot end all reading.
    fn pronounce(&mut self, text: &str) -> Result<String> {
        self.fallback.pronounce(text).or_else(|_| {
            self.fallback = Phonemizer::start()?;
            self.fallback
                .pronounce(text)
                .context("Local pronunciation failed. Try reading again.")
        })
    }
    fn words(&mut self, text: &str) -> Result<Vec<i32>> {
        let mut result = Vec::new();
        let mut token = String::new();
        for ch in text.chars().chain(std::iter::once(' ')) {
            if ch.is_alphabetic() || ch == '\'' {
                token.push(ch);
                continue;
            }
            if !token.is_empty() {
                if !result.is_empty() && result.last() != Some(&(' ' as i32)) {
                    result.push(' ' as i32);
                }
                result.extend_from_slice(&self.word(token.trim_matches('\''))?);
                token.clear();
            }
            if self.symbols.contains(&ch) && !ch.is_whitespace() {
                result.push(ch as i32);
            } else if ch.is_whitespace()
                && !result.is_empty()
                && result.last() != Some(&(' ' as i32))
            {
                result.push(' ' as i32);
            }
        }
        while result.last() == Some(&(' ' as i32)) {
            result.pop();
        }
        ensure!(
            !result.is_empty() && result.len() <= 32768,
            "No pronounceable text, or spoken replacement is too long"
        );
        Ok(result)
    }
    fn word(&mut self, raw: &str) -> Result<Vec<i32>> {
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let key = raw.to_lowercase();
        if self.compact
            && let Some(phones) = self
                .lexicon
                .get(&key)
                .filter(|phones| self.supported(phones))
        {
            return Ok(phones.clone());
        }
        if let Some(phones) = self.cache.get(&key) {
            return Ok(phones.clone());
        }
        let ipa = self.pronounce(&key)?;
        let converted = compatible(&ipa, self.compact);
        let mut phones: Vec<i32> = converted.trim().chars().map(|ch| ch as i32).collect();
        if !self.supported(&phones) {
            if let Some(known) = self.lexicon.get(&key).filter(|p| self.supported(p)) {
                return Ok(known.clone());
            }
            // A model may lack a rare phone. Spell the complete word instead of dropping it.
            phones.clear();
            for ch in key.chars() {
                let name = match ch {
                    'a'..='z' => ch.to_string(),
                    _ => format!("unicode {}", ch as u32),
                };
                for part in name.split_whitespace() {
                    let item = self
                        .lexicon
                        .get(part)
                        .filter(|p| self.supported(p))
                        .context("Add a word replacement for this spelling")?;
                    if !phones.is_empty() {
                        phones.push(' ' as i32);
                    }
                    phones.extend_from_slice(item);
                }
            }
        }
        ensure!(
            !phones.is_empty(),
            "Add a word replacement for this spelling"
        );
        if self.cache.len() >= 512 {
            self.cache.clear();
        }
        self.cache.insert(key, phones.clone());
        Ok(phones)
    }
    fn supported(&self, phones: &[i32]) -> bool {
        !phones.is_empty()
            && phones.iter().all(|code| {
                char::from_u32(*code as u32).is_some_and(|ch| self.symbols.contains(&ch))
            })
    }
}

fn compatible(ipa: &str, compact: bool) -> String {
    let mut output = ipa
        .replace('ɫ', "l")
        .replace(['\n', '\r', '\t'], " ")
        .replace(['\u{361}', '\u{35c}'], "");
    if compact {
        for (from, to) in [
            ("eɪ", "A"),
            ("aɪ", "I"),
            ("oʊ", "O"),
            ("əʊ", "Q"),
            ("aʊ", "W"),
            ("ɔɪ", "Y"),
            ("tʃ", "ʧ"),
            ("dʒ", "ʤ"),
        ] {
            output = output.replace(from, to);
        }
        output = output.replace('ː', "").replace('ɾ', "T");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_phone_alphabets_preserve_diphthongs() {
        assert_eq!(compatible("kˈeɪt tʃaɪld ɫoʊ\n", true), "kˈAt ʧIld lO ");
        assert_eq!(compatible("kˈeɪt tʃaɪld ɫoʊ", false), "kˈeɪt tʃaɪld loʊ");
    }
}
