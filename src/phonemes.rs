//! Native pronunciation in the Misaki phoneme convention shared by Kokoro and Paradee.
use crate::assets::AssetPaths;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
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
        // A helper that hangs at launch is killed when `value` drops on this error.
        let mut ready = [0u8; 4];
        value
            .read_before(&mut ready, Instant::now() + PRONUNCIATION_TIMEOUT)
            .context("Pronunciation could not load")?;
        ensure!(ready == [0; 4], "Invalid pronunciation runtime");
        Ok(value)
    }
    fn pronounce(&mut self, word: &str, british: bool) -> Result<String> {
        ensure!(
            !word.is_empty() && word.len() <= 8192,
            "Word exceeds the pronunciation limit"
        );
        self.input
            .write_all(&(word.len() as u32 + 1).to_le_bytes())?;
        self.input.write_all(if british { b"b" } else { b"a" })?;
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
    /// American and British Misaki lexicons, indexed by `british`.
    lexicons: [HashMap<String, Vec<i32>>; 2],
    directory: PathBuf,
    symbols: HashSet<char>,
    fallback: Phonemizer,
    caches: [HashMap<String, Vec<i32>>; 2],
    british: bool,
}
impl EnglishFrontend {
    pub fn load(paths: &AssetPaths) -> Result<Self> {
        let symbols = fs::read_to_string(paths.speech.join("tokens.txt"))?
            .lines()
            .filter_map(|line| line.rsplit_once(' ').map(|(token, _)| token))
            .filter(|token| token.chars().count() == 1)
            .flat_map(str::chars)
            .collect();
        Ok(Self {
            lexicons: [load_lexicon(&paths.speech, "us")?, HashMap::new()],
            directory: paths.speech.clone(),
            symbols,
            fallback: Phonemizer::start()?,
            caches: [HashMap::new(), HashMap::new()],
            british: false,
        })
    }
    pub fn phonemes(&mut self, text: &str, british: bool) -> Result<Vec<i32>> {
        // No current voice is British, so that lexicon loads only when one asks for it.
        if british && self.lexicons[1].is_empty() {
            self.lexicons[1] = load_lexicon(&self.directory, "gb")?;
        }
        self.british = british;
        self.words(text)
    }
    /// A crashed or hung helper is replaced once, so one failure cannot end all reading.
    fn pronounce(&mut self, text: &str) -> Result<String> {
        let british = self.british;
        self.fallback.pronounce(text, british).or_else(|_| {
            self.fallback = Phonemizer::start()?;
            self.fallback
                .pronounce(text, british)
                .context("Local pronunciation failed. Try reading again.")
        })
    }
    fn words(&mut self, text: &str) -> Result<Vec<i32>> {
        enum Piece {
            Word(String, Vec<i32>),
            Symbol(char),
            Space,
        }
        let mut pieces = Vec::new();
        let mut token = String::new();
        for ch in text.chars().chain(std::iter::once(' ')) {
            if ch.is_alphabetic() || ch == '\'' {
                token.push(ch);
                continue;
            }
            if !token.is_empty() {
                let raw = token.trim_matches('\'').to_owned();
                let phones = self.word(&raw)?;
                pieces.push(Piece::Word(raw, phones));
                token.clear();
            }
            if self.symbols.contains(&ch) && !ch.is_whitespace() {
                pieces.push(Piece::Symbol(ch));
            } else if ch.is_whitespace() {
                pieces.push(Piece::Space);
            }
        }
        // A capital "A" is the article only where a sentence starts; elsewhere it is a letter.
        let mut initial = true;
        let starts: Vec<bool> = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Word(..) => std::mem::replace(&mut initial, false),
                Piece::Symbol(ch) => {
                    initial |= ".!?…".contains(*ch);
                    false
                }
                Piece::Space => false,
            })
            .collect();
        // Function words depend on what follows, as in Misaki: walk right to left,
        // tracking whether the next sound is a vowel (None at punctuation or the end).
        let mut next_vowel = None;
        for (piece, initial) in pieces.iter_mut().zip(starts).rev() {
            match piece {
                Piece::Symbol(ch) if PAUSES.contains(*ch) => next_vowel = None,
                Piece::Word(raw, phones) => {
                    if let Some(reduced) = function_word(raw, next_vowel, initial)
                        .filter(|reduced| self.supported(reduced))
                    {
                        *phones = reduced;
                    }
                    if let Some(vowel) = starts_with_vowel(phones) {
                        next_vowel = vowel;
                    }
                }
                _ => {}
            }
        }
        let mut result = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Word(_, phones) => {
                    if !result.is_empty() && result.last() != Some(&(' ' as i32)) {
                        result.push(' ' as i32);
                    }
                    result.extend_from_slice(&phones);
                }
                Piece::Symbol(ch) => result.push(ch as i32),
                Piece::Space => {
                    if !result.is_empty() && result.last() != Some(&(' ' as i32)) {
                        result.push(' ' as i32);
                    }
                }
            }
        }
        while result.last() == Some(&(' ' as i32)) {
            result.pop();
        }
        // Misaki's final step for v1.0 models: flaps become T and glottal stops t. The
        // models were trained on that output, so ɾ is a nearly unseen symbol to them.
        for code in &mut result {
            if *code == 'ɾ' as i32 {
                *code = 'T' as i32;
            } else if *code == 'ʔ' as i32 {
                *code = 't' as i32;
            }
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
        let dialect = self.british as usize;
        if let Some(phones) = self.lexicons[dialect]
            .get(&key)
            .filter(|phones| self.supported(phones))
        {
            return Ok(phones.clone());
        }
        if let Some(phones) = self.derived(&key) {
            return Ok(phones);
        }
        if let Some(phones) = self.caches[dialect].get(&key) {
            return Ok(phones.clone());
        }
        let ipa = self.pronounce(&key)?;
        let converted = compatible(&ipa, self.british);
        let mut phones: Vec<i32> = converted.trim().chars().map(|ch| ch as i32).collect();
        if !self.supported(&phones) {
            if let Some(known) = self.lexicons[dialect]
                .get(&key)
                .filter(|p| self.supported(p))
            {
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
                    let item = self.lexicons[dialect]
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
        if self.caches[dialect].len() >= 512 {
            self.caches[dialect].clear();
        }
        self.caches[dialect].insert(key, phones.clone());
        Ok(phones)
    }
    fn known(&self, word: &str) -> Option<String> {
        self.lexicons[self.british as usize]
            .get(word)
            .filter(|phones| self.supported(phones))
            .map(|phones| {
                phones
                    .iter()
                    .filter_map(|code| char::from_u32(*code as u32))
                    .collect()
            })
    }
    /// Misaki's -s, -ed and -ing rules: a word missing from the lexicon is built from its
    /// stem, so it keeps the stem's phones instead of eSpeak's.
    fn derived(&self, word: &str) -> Option<Vec<i32>> {
        let british = self.british;
        let reduced = if british { 'ɪ' } else { 'ᵻ' };
        let stem = |cut: usize, extra: &str| {
            let base = format!("{}{extra}", &word[..word.len() - cut]);
            self.known(&base)
        };
        let phones = if word.len() >= 3 && word.ends_with('s') {
            let stem = (!word.ends_with("ss"))
                .then(|| stem(1, ""))
                .flatten()
                .or_else(|| {
                    (word.ends_with("'s")
                        || (word.len() > 4 && word.ends_with("es") && !word.ends_with("ies")))
                    .then(|| stem(2, ""))
                    .flatten()
                })
                .or_else(|| {
                    (word.len() > 4 && word.ends_with("ies"))
                        .then(|| stem(3, "y"))
                        .flatten()
                })?;
            let last = stem.chars().last()?;
            if "ptkfθ".contains(last) {
                format!("{stem}s")
            } else if "szʃʒʧʤ".contains(last) {
                format!("{stem}{reduced}z")
            } else {
                format!("{stem}z")
            }
        } else if word.len() >= 4 && word.ends_with('d') {
            let stem = (!word.ends_with("dd"))
                .then(|| stem(1, ""))
                .flatten()
                .or_else(|| {
                    (word.len() > 4 && word.ends_with("ed") && !word.ends_with("eed"))
                        .then(|| stem(2, ""))
                        .flatten()
                })?;
            let mut chars: Vec<char> = stem.chars().collect();
            let last = *chars.last()?;
            if "pkfθʃsʧ".contains(last) {
                format!("{stem}t")
            } else if last == 'd' {
                format!("{stem}{reduced}d")
            } else if last != 't' {
                format!("{stem}d")
            } else if british || chars.len() < 2 {
                format!("{stem}ɪd")
            } else if US_TAUS.contains(chars[chars.len() - 2]) {
                chars.pop();
                format!("{}ɾᵻd", chars.into_iter().collect::<String>())
            } else {
                format!("{stem}ᵻd")
            }
        } else if word.len() >= 5 && word.ends_with("ing") {
            let doubled = word.ends_with("cking") || {
                let bytes = word.as_bytes();
                let n = bytes.len();
                n > 5 && bytes[n - 4] == bytes[n - 5] && b"bcdgklmnprstvxz".contains(&bytes[n - 4])
            };
            let stem = (word.len() > 5)
                .then(|| stem(3, ""))
                .flatten()
                .or_else(|| stem(3, "e"))
                .or_else(|| (word.len() > 5 && doubled).then(|| stem(4, "")).flatten())?;
            let mut chars: Vec<char> = stem.chars().collect();
            let last = *chars.last()?;
            if british && "əː".contains(last) {
                return None;
            } else if !british
                && last == 't'
                && chars.len() > 1
                && US_TAUS.contains(chars[chars.len() - 2])
            {
                chars.pop();
                format!("{}ɾɪŋ", chars.into_iter().collect::<String>())
            } else {
                format!("{stem}ɪŋ")
            }
        } else {
            return None;
        };
        let phones: Vec<i32> = phones.chars().map(|ch| ch as i32).collect();
        self.supported(&phones).then_some(phones)
    }
    fn supported(&self, phones: &[i32]) -> bool {
        !phones.is_empty()
            && phones.iter().all(|code| {
                char::from_u32(*code as u32).is_some_and(|ch| self.symbols.contains(&ch))
            })
    }
}

fn load_lexicon(directory: &Path, dialect: &str) -> Result<HashMap<String, Vec<i32>>> {
    let mut lexicon = HashMap::new();
    for line in fs::read_to_string(directory.join(format!("lexicon-{dialect}-en.txt")))?.lines() {
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
    Ok(lexicon)
}

/// Punctuation that ends a phrase for function-word context; quotes do not.
const PAUSES: &str = ";:,.!?—…";
const VOWELS: &str = "AIOQWYaiuæɑɒɔəɛɜɪʊʌᵻ";
const CONSONANTS: &str = "bdfhjklmnpstvwzðŋɡɹɾʃʒʤʧθ";
/// Vowels after which American English flaps a following t.
const US_TAUS: &str = "AIOWYiuæɑəɛɪɹʊʌ";

/// Some(true) when the phones begin with a vowel sound, Some(false) for a consonant,
/// Some(None) when a pause comes first, and None when nothing tells either way.
fn starts_with_vowel(phones: &[i32]) -> Option<Option<bool>> {
    phones
        .iter()
        .filter_map(|code| char::from_u32(*code as u32))
        .find_map(|ch| {
            if PAUSES.contains(ch) {
                Some(None)
            } else if VOWELS.contains(ch) {
                Some(Some(true))
            } else if CONSONANTS.contains(ch) {
                Some(Some(false))
            } else {
                None
            }
        })
}

/// Misaki's reduced forms for unstressed function words. `next_vowel` describes the
/// following word, or is None before a pause. Words such as "a" read as letters before
/// a pause, and a capital "A" after the start of a sentence is a letter, as in "Vitamin A is".
fn function_word(raw: &str, next_vowel: Option<bool>, initial: bool) -> Option<Vec<i32>> {
    let phones = match (raw, next_vowel) {
        ("a", Some(_)) => "ɐ",
        ("A", Some(_)) if initial => "ɐ",
        ("an" | "An" | "AN", Some(_)) => "ɐn",
        ("am" | "Am", Some(_)) => "ɐm",
        ("I", _) => "ˌI",
        ("to" | "To" | "TO", Some(false)) => "tə",
        ("to" | "To" | "TO", Some(true)) => "tʊ",
        ("in" | "In" | "IN", Some(_)) => "ɪn",
        ("in" | "In" | "IN", None) => "ˈɪn",
        ("the" | "The" | "THE", Some(true)) => "ði",
        ("the" | "The" | "THE", _) => "ðə",
        _ => return None,
    };
    Some(phones.chars().map(|ch| ch as i32).collect())
}

/// eSpeak IPA to Misaki phones, following Misaki's EspeakFallback for each dialect. The
/// helper emits no tie bars, so only unambiguous two-letter sequences are merged.
fn compatible(ipa: &str, british: bool) -> String {
    let mut output = ipa
        .replace('ɫ', "l")
        .replace(['\n', '\r', '\t'], " ")
        .replace(['\u{361}', '\u{35c}'], "");
    let dialect: &[(&str, &str)] = if british {
        &[("eə", "ɛː"), ("iə", "ɪə"), ("əʊ", "Q")]
    } else {
        &[("oʊ", "O"), ("ɜːɹ", "ɜɹ"), ("ɜː", "ɜɹ"), ("ɪə", "iə")]
    };
    for (from, to) in dialect.iter().copied().chain([
        ("ʔˌn\u{329}", "ʔn"),
        ("ʔn\u{329}", "ʔn"),
        ("aɪ", "I"),
        ("aʊ", "W"),
        ("dʒ", "ʤ"),
        ("eɪ", "A"),
        ("tʃ", "ʧ"),
        ("ɔɪ", "Y"),
        ("ʲo", "jo"),
        ("ʲə", "jə"),
        ("ʲ", ""),
        ("ɚ", "əɹ"),
        ("r", "ɹ"),
        ("x", "k"),
        ("ç", "k"),
        ("ɐ", "ə"),
        ("ɬ", "l"),
        ("\u{303}", ""),
        ("e", "A"),
        ("o", "ɔ"),
    ]) {
        output = output.replace(from, to);
    }
    // A syllabic consonant is written as ᵊ before it.
    let mut syllabic = String::with_capacity(output.len());
    for ch in output.chars() {
        if ch == '\u{329}' {
            if let Some(previous) = syllabic.pop() {
                syllabic.push('ᵊ');
                syllabic.push(previous);
            }
        } else {
            syllabic.push(ch);
        }
    }
    if !british {
        syllabic = syllabic.replace('ː', "");
    }
    syllabic.replace('ɾ', "T").replace('ʔ', "t")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn phones(text: &str) -> Vec<i32> {
        text.chars().map(|ch| ch as i32).collect()
    }
    #[test]
    fn function_words_reduce_like_misaki() {
        assert_eq!(function_word("a", Some(false), false), Some(phones("ɐ")));
        assert_eq!(function_word("A", None, true), None);
        assert_eq!(function_word("A", Some(false), true), Some(phones("ɐ")));
        assert_eq!(function_word("A", Some(true), false), None);
        assert_eq!(function_word("the", Some(true), false), Some(phones("ði")));
        assert_eq!(function_word("The", Some(false), false), Some(phones("ðə")));
        assert_eq!(function_word("the", None, false), Some(phones("ðə")));
        assert_eq!(function_word("to", Some(false), false), Some(phones("tə")));
        assert_eq!(function_word("to", Some(true), false), Some(phones("tʊ")));
        assert_eq!(function_word("to", None, false), None);
        assert_eq!(function_word("in", None, false), Some(phones("ˈɪn")));
        assert_eq!(function_word("I", None, false), Some(phones("ˌI")));
        assert_eq!(function_word("cat", Some(true), false), None);
    }
    #[test]
    fn next_sound_skips_stress_and_stops_at_pauses() {
        assert_eq!(starts_with_vowel(&phones("ˈæpəl")), Some(Some(true)));
        assert_eq!(starts_with_vowel(&phones("ˌʌndəɹ")), Some(Some(true)));
        assert_eq!(starts_with_vowel(&phones("klˈɪɹ")), Some(Some(false)));
        assert_eq!(starts_with_vowel(&phones(",")), Some(None));
        assert_eq!(starts_with_vowel(&phones("ˈ")), None);
    }
    #[test]
    fn model_phone_alphabet_preserves_diphthongs() {
        assert_eq!(compatible("kˈeɪt tʃaɪld ɫoʊ\n", false), "kˈAt ʧIld lO ");
        assert_eq!(compatible("bˈɜːd hˈeə", false), "bˈɜɹd hˈAə");
        assert_eq!(compatible("bˈɜːd hˈeə ɡˈəʊ", true), "bˈɜːd hˈɛː ɡˈQ");
        assert_eq!(compatible("bˈʌtn\u{329}", false), "bˈʌtᵊn");
    }
}
