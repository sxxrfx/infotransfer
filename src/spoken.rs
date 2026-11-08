//! English developer-text rendering. Selected text is never modified in place.
use crate::dictionary::{self, WordReplacement};
use anyhow::{Result, ensure};
use regex::Regex;
use std::sync::LazyLock;

static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"^(?:0[xX][0-9a-fA-F]+|0[bB][01]+|0[oO][0-7]+|(?:[0-9]{1,3}(?:,[0-9]{3})+|[0-9]+)(?:\.[0-9]+)*(?:[eE][+-]?[0-9]+)?(?:st|nd|rd|th)?)"
).unwrap()
});
static CLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{1,2}):([0-9]{2})(?::([0-9]{2}))?").unwrap());
static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{4})-([0-9]{2})-([0-9]{2})").unwrap());
static SLASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{1,4})/([0-9]{1,4})(?:/([0-9]{4}|[0-9]{2}))?").unwrap());
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:[0-9]{1,3}-)?[0-9]{3}-[0-9]{3}-[0-9]{4}|[0-9]{3}-[0-9]{4})").unwrap()
});
static INITIALS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]\b").unwrap());
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const LETTER_NAMES: [&str; 26] = [
    "eigh",
    "bee",
    "see",
    "dee",
    "ee",
    "ef",
    "gee",
    "aitch",
    "eye",
    "jay",
    "kay",
    "el",
    "em",
    "en",
    "oh",
    "pee",
    "cue",
    "are",
    "ess",
    "tee",
    "you",
    "vee",
    "double you",
    "ex",
    "why",
    "zee",
];

const SMALL: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn integer(n: u64) -> String {
    if n < 20 {
        return SMALL[n as usize].into();
    }
    if n < 100 {
        return if n.is_multiple_of(10) {
            TENS[n as usize / 10].into()
        } else {
            format!("{} {}", TENS[n as usize / 10], SMALL[n as usize % 10])
        };
    }
    if n < 1000 {
        return if n.is_multiple_of(100) {
            format!("{} hundred", SMALL[n as usize / 100])
        } else {
            format!("{} hundred {}", SMALL[n as usize / 100], integer(n % 100))
        };
    }
    for (scale, name) in [
        (1_000_000_000_000_000_000, "quintillion"),
        (1_000_000_000_000_000, "quadrillion"),
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1000, "thousand"),
    ] {
        if n >= scale {
            return if n.is_multiple_of(scale) {
                format!("{} {name}", integer(n / scale))
            } else {
                format!("{} {name} {}", integer(n / scale), integer(n % scale))
            };
        }
    }
    unreachable!()
}
/// Years from 1100 to 2099 as people say them; other numbers as quantities.
fn year(n: u64) -> String {
    if !(1100..2100).contains(&n) || (2000..2010).contains(&n) {
        return integer(n);
    }
    match n % 100 {
        0 => format!("{} hundred", integer(n / 100)),
        low @ 1..=9 => format!("{} oh {}", integer(n / 100), integer(low)),
        low => format!("{} {}", integer(n / 100), integer(low)),
    }
}
/// A bare four-digit number in prose. 2048 is far more often a size than a year.
fn year_like(raw: &str) -> bool {
    raw.len() == 4
        && raw.bytes().all(|byte| byte.is_ascii_digit())
        && raw
            .parse::<u64>()
            .is_ok_and(|n| (1100..2100).contains(&n) && n != 2048)
}
/// Identifiers such as `port 1234` or `PID 1999` are numbers, not years.
fn identifier_before(text: &str) -> bool {
    text.trim_end()
        .rsplit([' ', '\t', '\n'])
        .next()
        .is_some_and(|word| {
            [
                "port", "pid", "id", "code", "error", "errno", "line", "lines", "issue", "pr",
                "page", "row", "column", "col", "status", "step", "item", "exit", "build",
                "ticket", "number", "no", "uid", "gid", "offset", "index", "byte", "bytes",
            ]
            .iter()
            .any(|known| {
                word.trim_end_matches([':', '='])
                    .eq_ignore_ascii_case(known)
            })
        })
}
fn plural_number(spoken: &str) -> String {
    spoken
        .strip_suffix('y')
        .map_or_else(|| format!("{spoken}s"), |stem| format!("{stem}ies"))
}
fn fraction(numerator: u64, denominator: u64) -> Option<String> {
    let name = [
        "half", "third", "quarter", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    ]
    .get(usize::try_from(denominator).ok()?.checked_sub(2)?)?;
    (1..denominator).contains(&numerator).then(|| {
        format!(
            "{} {name}{}",
            integer(numerator),
            if numerator == 1 { "" } else { "s" }
        )
    })
}
fn date(year: u64, month: usize, day: u64, spoken_year: &str) -> Option<String> {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&month) || day == 0 || day > days[month - 1] {
        return None;
    }
    Some(format!(
        "{} {}, {spoken_year}",
        MONTHS[month - 1],
        ordinal(&day.to_string())
    ))
}
/// Unicode decimal digits other than ASCII, such as full-width or Arabic-Indic ones.
fn decimal_digit(ch: char) -> Option<char> {
    let code = ch as u32;
    [
        0xFF10, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66,
        0x0CE6, 0x0D66, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x17E0, 0x1810, 0x1D7CE, 0x1D7D8, 0x1D7E2,
        0x1D7EC, 0x1D7F6,
    ]
    .into_iter()
    .find(|zero| (*zero..*zero + 10).contains(&code))
    .and_then(|zero| char::from_digit(code - zero, 10))
}
fn superscript(ch: char) -> Option<char> {
    match ch {
        '⁰' => Some('0'),
        '¹' => Some('1'),
        '²' => Some('2'),
        '³' => Some('3'),
        '⁴'..='⁹' => char::from_digit(ch as u32 - '⁴' as u32 + 4, 10),
        '⁻' => Some('-'),
        _ => None,
    }
}
/// Precomposes common accented letters and drops other combining marks after a letter,
/// so a decomposed word such as "cafe\u{301}" stays one word.
fn normalize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(digit) = decimal_digit(ch) {
            output.push(digit);
            continue;
        }
        if ('\u{300}'..='\u{36f}').contains(&ch)
            && let Some(base) = output
                .chars()
                .next_back()
                .filter(|base| base.is_alphabetic())
        {
            let composed = [
                ('\u{301}', "aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
                ('\u{300}', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
                ('\u{302}', "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
                ('\u{308}', "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
                ('\u{303}', "anoANO", "ãñõÃÑÕ"),
                ('\u{327}', "cC", "çÇ"),
            ]
            .into_iter()
            .find(|(mark, _, _)| *mark == ch)
            .and_then(|(_, plain, accented)| {
                plain
                    .chars()
                    .position(|letter| letter == base)
                    .and_then(|index| accented.chars().nth(index))
            });
            if let Some(composed) = composed {
                output.pop();
                output.push(composed);
            }
            continue;
        }
        output.push(ch);
    }
    output
}
fn digits(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '0'..='9' => SMALL[ch as usize - '0' as usize].to_owned(),
            'a'..='z' | 'A'..='Z' => {
                LETTER_NAMES[ch.to_ascii_uppercase() as usize - b'A' as usize].to_owned()
            }
            _ => ch.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn cardinal(text: &str) -> String {
    let clean = text.replace(',', "");
    if clean.len() > 1 && clean.starts_with('0') {
        return digits(&clean);
    }
    clean
        .parse::<u64>()
        .map(integer)
        .unwrap_or_else(|_| digits(&clean))
}
fn ordinal(text: &str) -> String {
    let value = cardinal(text);
    let (prefix, last) = value
        .rsplit_once(' ')
        .map_or(("", value.as_str()), |(a, b)| (a, b));
    let ending = match last {
        "zero" => "zeroth".into(),
        "one" => "first".into(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        word if word.ends_with('y') => format!("{}ieth", &word[..word.len() - 1]),
        word => format!("{word}th"),
    };
    if prefix.is_empty() {
        ending
    } else {
        format!("{prefix} {ending}")
    }
}
fn numeric(raw: &str, version: bool) -> String {
    if raw.starts_with("0x") || raw.starts_with("0X") {
        return format!("hex {}", digits(&raw[2..]));
    }
    if raw.starts_with("0b") || raw.starts_with("0B") {
        return format!("binary {}", digits(&raw[2..]));
    }
    if raw.starts_with("0o") || raw.starts_with("0O") {
        return format!("octal {}", digits(&raw[2..]));
    }
    if let Some(index) = raw.find(['e', 'E']) {
        let exponent = &raw[index + 1..];
        let sign = if exponent.starts_with('-') {
            "minus "
        } else {
            ""
        };
        return format!(
            "{} times ten to the power of {sign}{}",
            numeric(&raw[..index], false),
            exponent
                .trim_start_matches(['+', '-'])
                .parse::<u64>()
                .map(integer)
                .unwrap_or_else(|_| digits(exponent.trim_start_matches(['+', '-'])))
        );
    }
    if ["st", "nd", "rd", "th"]
        .iter()
        .any(|suffix| raw.ends_with(suffix))
    {
        return ordinal(&raw[..raw.len() - 2]);
    }
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() == 1 {
        return cardinal(raw);
    }
    if version || parts.len() > 2 {
        return parts
            .iter()
            .map(|part| cardinal(part))
            .collect::<Vec<_>>()
            .join(" point ");
    }
    format!("{} point {}", cardinal(parts[0]), digits(parts[1]))
}
fn number_at(text: &str) -> Option<&str> {
    NUMBER.find(text).map(|found| found.as_str())
}
fn boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
}

const ALIASES: &[(&str, &str)] = &[
    ("NVIDIA", "nvidia"),
    ("CUDA", "cuda"),
    ("ASCII", "ask ee"),
    ("GNU", "guh new"),
    ("GUI", "gooey"),
    ("AJAX", "ajax"),
    ("TODO", "to do"),
    ("FIXME", "fix me"),
    ("JSONL", "jason lines"),
    ("JSON5", "jason five"),
    ("YML", "yammel"),
    ("TOML", "tom el"),
    ("PostgreSQL", "postgres"),
    ("GraphQL", "graph Q L"),
    ("TypeScript", "type script"),
    ("JavaScript", "java script"),
    ("Node.js", "node J S"),
    ("Next.js", "next J S"),
    ("Vue.js", "view J S"),
    (".NET", "dot net"),
    ("UTF-32", "U T F thirty two"),
    ("UTF-16", "U T F sixteen"),
    ("UTF-8", "U T F eight"),
    ("OAuth2", "O auth two"),
    ("OAuth", "O auth"),
    ("gRPC", "G R P C"),
    ("C++", "C plus plus"),
    ("C#", "C sharp"),
    ("F#", "F sharp"),
    ("JSON", "jason"),
    ("YAML", "yammel"),
    ("SQL", "S Q L"),
    ("README", "read me"),
    ("stdout", "standard output"),
    ("stderr", "standard error"),
    ("stdin", "standard input"),
    ("localhost", "local host"),
    ("NaN", "not a number"),
    ("i18n", "internationalization"),
    ("l10n", "localization"),
    ("base64", "base sixty four"),
    ("sha256", "S H A two five six"),
    ("e.g.", "for example"),
    ("i.e.", "that is"),
    ("etc.", "et cetera"),
    ("vs.", "versus"),
];
const ACRONYMS: &[&str] = &[
    "api", "cpu", "gpu", "cli", "sdk", "http", "https", "html", "css", "xml", "ui", "ux", "url",
    "uri", "uuid", "id", "ip", "tcp", "udp", "dns", "ssh", "ssl", "tls", "jwt", "jsx", "tsx", "js",
    "ts", "rs", "rb", "py", "npm", "pnpm", "npx", "ci", "cd", "orm", "oop", "ssd", "hdd", "ascii",
    "utf", "tts", "llm", "ai", "sha", "gpt", "aws",
];
/// Also ordinary words, so spelled only when written in capitals.
const CASED_ACRONYMS: &[&str] = &["ram", "rom", "crud", "ide"];
/// Aliases that are also ordinary words in other casings, such as "my nan".
const EXACT_ALIASES: &[&str] = &["NaN", "GNU"];
/// Short words read as words, not letters, even in capitals. Ambiguous ones such as
/// IT, US, AM and OK stay spelled.
const COMMON: &[&str] = &[
    "an", "as", "at", "be", "by", "do", "go", "he", "hi", "if", "in", "is", "me", "my", "no", "of",
    "oh", "on", "or", "so", "to", "up", "we", "the", "and", "not", "use", "all", "any", "are",
    "but", "can", "did", "for", "get", "got", "had", "has", "her", "him", "his", "how", "let",
    "may", "new", "now", "off", "old", "one", "our", "out", "own", "put", "run", "say", "see",
    "set", "she", "too", "try", "two", "was", "way", "who", "why", "yes", "yet", "you", "add",
    "big", "bug", "end", "fix", "key", "few", "ask", "bad", "hot", "low", "top", "win", "ten",
    "six", "its", "via", "per", "max", "min", "day",
];
const ONSETS: &[&str] = &[
    "bl", "br", "ch", "cl", "cr", "dr", "fl", "fr", "gl", "gr", "kn", "ph", "pl", "pr", "sc", "sh",
    "sk", "sl", "sm", "sn", "sp", "st", "sw", "th", "tr", "tw", "wh", "wr", "chr", "sch", "scr",
    "shr", "spl", "spr", "str", "thr",
];

/// Whether a lowercase word written in capitals can be read as a word, as in HELLO or
/// NASA, rather than spelled, as in HTML or RGBA.
fn pronounceable(word: &str) -> bool {
    let vowel = |ch: char| "aeiouy".contains(ch);
    let onset = &word[..word.find(vowel).unwrap_or(word.len())];
    let coda = &word[word.rfind(vowel).map_or(0, |index| index + 1)..];
    onset.len() < word.len()
        && (onset.len() <= 1 || ONSETS.contains(&onset))
        && word.split(vowel).all(|run| run.len() <= 3)
        && word.split(|ch| !vowel(ch)).all(|run| run.len() <= 2)
        && (word.len() > 3
            || coda.len() <= 1
            || ["ck", "sh", "ch", "th", "ng", "st", "nd", "nt", "ll", "ss"].contains(&coda))
}
fn spell(text: &str) -> String {
    text.chars()
        .map(|ch| LETTER_NAMES[ch.to_ascii_uppercase() as usize - b'A' as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// `shouted` is set when a neighbouring word is also in capitals, as in "DO NOT USE".
fn word(text: &str, shouted: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for index in 1..chars.len() {
        // A plural acronym such as APIs stays whole.
        let plural = chars.get(index + 1) == Some(&'s')
            && chars.get(index + 2).is_none_or(|ch| !ch.is_lowercase());
        if chars[index].is_uppercase()
            && (chars[index - 1].is_lowercase()
                || (chars[index - 1].is_uppercase()
                    && !plural
                    && chars.get(index + 1).is_some_and(|ch| ch.is_lowercase())))
        {
            parts.push(chars[start..index].iter().collect::<String>());
            start = index;
        }
    }
    parts.push(chars[start..].iter().collect::<String>());
    let single = parts.len() == 1;
    parts
        .into_iter()
        .map(|part| {
            let ordinary_word = [
                "error", "warn", "warning", "success", "failed", "fail", "pass", "passed", "true",
                "false", "null", "none", "void", "info", "debug", "trace", "note", "test", "rust",
                "swift", "java", "ruby", "dart", "go", "react", "linux", "macos", "windows",
                "docker",
            ]
            .iter()
            .any(|word| part.eq_ignore_ascii_case(word));
            if part.len() == 1
                && part.as_bytes()[0].is_ascii_alphabetic()
                && !part.eq_ignore_ascii_case("a")
                && !part.eq_ignore_ascii_case("i")
            {
                return LETTER_NAMES
                    [part.as_bytes()[0].to_ascii_uppercase() as usize - b'A' as usize]
                    .to_owned();
            }
            let capitals =
                |text: &str| text.len() > 1 && text.chars().all(|ch| ch.is_ascii_uppercase());
            let lower = part.to_ascii_lowercase();
            let acronym = ACRONYMS.contains(&lower.as_str())
                || (capitals(&part) && CASED_ACRONYMS.contains(&lower.as_str()));
            let spoken = ordinary_word
                || COMMON.contains(&lower.as_str())
                || (single && (part.len() > 3 || shouted) && pronounceable(&lower));
            if acronym || (capitals(&part) && part.len() <= 5 && !spoken) {
                return spell(&part);
            }
            // Plural and possessive acronyms: APIs and PR's become "… eye's".
            if let Some(stem) = part
                .strip_suffix("'s")
                .or_else(|| part.strip_suffix('s'))
                .filter(|stem| capitals(stem) && stem.len() <= 5)
            {
                return format!("{}'s", spell(stem));
            }
            part
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn contraction(text: &str) -> Option<String> {
    let text = text.trim_matches('\'');
    let lower = text.to_ascii_lowercase();
    let special = match lower.as_str() {
        "i'am" | "i'm" => Some("I am"),
        "can't" => Some("cannot"),
        "won't" => Some("will not"),
        "shan't" => Some("shall not"),
        "ain't" => Some("is not"),
        "let's" => Some("let us"),
        "it's" => Some("it is"),
        "that's" => Some("that is"),
        "there's" => Some("there is"),
        "here's" => Some("here is"),
        "what's" => Some("what is"),
        "who's" => Some("who is"),
        "where's" => Some("where is"),
        "how's" => Some("how is"),
        "he's" => Some("he is"),
        "she's" => Some("she is"),
        _ => None,
    };
    if let Some(value) = special {
        return Some(value.into());
    }
    if let Some(base) = lower.strip_suffix("n't") {
        return Some(format!("{base} not"));
    }
    for (suffix, expanded) in [
        ("'ll", "will"),
        ("'re", "are"),
        ("'ve", "have"),
        ("'d", "would"),
    ] {
        if let Some(base) = lower.strip_suffix(suffix)
            && !base.is_empty()
        {
            return Some(format!(
                "{} {expanded}",
                if base == "i" { "I" } else { base }
            ));
        }
    }
    None
}
fn unit(text: &str, single: bool) -> Option<(&str, String)> {
    for (short, singular, plural) in [
        ("GiB", "gibibyte", "gibibytes"),
        ("MiB", "mebibyte", "mebibytes"),
        ("KiB", "kibibyte", "kibibytes"),
        ("GB/s", "gigabyte per second", "gigabytes per second"),
        ("MB/s", "megabyte per second", "megabytes per second"),
        ("Gbps", "gigabit per second", "gigabits per second"),
        ("Mbps", "megabit per second", "megabits per second"),
        ("GHz", "gigahertz", "gigahertz"),
        ("MHz", "megahertz", "megahertz"),
        ("kHz", "kilohertz", "kilohertz"),
        ("TB", "terabyte", "terabytes"),
        ("GB", "gigabyte", "gigabytes"),
        ("MB", "megabyte", "megabytes"),
        ("KB", "kilobyte", "kilobytes"),
        ("kB", "kilobyte", "kilobytes"),
        ("Hz", "hertz", "hertz"),
        ("ms", "millisecond", "milliseconds"),
        ("ns", "nanosecond", "nanoseconds"),
        ("µs", "microsecond", "microseconds"),
        ("us", "microsecond", "microseconds"),
        ("px", "pixel", "pixels"),
        ("fps", "frame per second", "frames per second"),
        ("min", "minute", "minutes"),
        ("s", "second", "seconds"),
    ] {
        if text.starts_with(short) && boundary(text[short.len()..].chars().next()) {
            return Some((
                &text[..short.len()],
                if single { singular } else { plural }.into(),
            ));
        }
    }
    None
}
fn calendar(text: &str) -> Option<(usize, String)> {
    let captures = DATE.captures(text)?;
    let value: u64 = captures[1].parse().ok()?;
    Some((
        captures[0].len(),
        date(
            value,
            captures[2].parse().ok()?,
            captures[3].parse().ok()?,
            &year(value),
        )?,
    ))
}
/// Month/day/year dates, common fractions and other `a/b` numbers.
fn slash(text: &str) -> Option<(usize, String)> {
    let captures = SLASH.captures(text)?;
    let end = captures[0].len();
    let mut after = text[end..].chars();
    let joined = match after.next() {
        Some(ch) if ch.is_alphanumeric() || ch == '_' => true,
        Some('/' | '.' | ':' | '-' | ',') => after.next().is_some_and(char::is_alphanumeric),
        _ => false,
    };
    if joined {
        return None;
    }
    let (first, second): (u64, u64) = (captures[1].parse().ok()?, captures[2].parse().ok()?);
    if let Some(full) = captures.get(3) {
        let value: u64 = full.as_str().parse().ok()?;
        let spoken = match full.len() {
            4 => year(value),
            _ if value < 10 => format!("oh {}", integer(value)),
            _ => integer(value),
        };
        let value = if full.len() == 2 { 2000 + value } else { value };
        return date(value, first as usize, second, &spoken).map(|spoken| (end, spoken));
    }
    if (first, second) == (24, 7) {
        return Some((end, "twenty four seven".into()));
    }
    Some((
        end,
        fraction(first, second)
            .unwrap_or_else(|| format!("{} over {}", integer(first), integer(second))),
    ))
}
/// Phone-like digit groups, read group by group.
fn phone(text: &str) -> Option<(usize, String)> {
    let found = PHONE.find(text)?.as_str();
    let mut after = text[found.len()..].chars();
    let groups: Vec<_> = found.split('-').collect();
    let joined = match after.next() {
        Some(ch) if ch.is_alphanumeric() => true,
        Some('-' | '.' | '/' | ':') => after.next().is_some_and(|ch| ch.is_ascii_digit()),
        _ => false,
    };
    // A range such as 100-1000 is not a phone number.
    if joined || (groups.len() == 2 && groups[0].ends_with("00")) {
        return None;
    }
    Some((
        found.len(),
        groups
            .iter()
            .map(|group| digits(group))
            .collect::<Vec<_>>()
            .join(", "),
    ))
}
/// `10am`, `10 pm`, `10 a.m.`: the letters and how much text they cover. A final period
/// stays when it may end the sentence.
fn meridiem(text: &str) -> Option<(usize, &'static str)> {
    let spaces = text.len() - text.trim_start_matches(' ').len();
    let rest = &text[spaces..];
    let (form, name) = [
        ("a.m", "eigh em"),
        ("p.m", "pee em"),
        ("am", "eigh em"),
        ("pm", "pee em"),
    ]
    .into_iter()
    .find(|(form, _)| {
        rest.get(..form.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(form))
            && boundary(rest[form.len()..].chars().next())
    })?;
    let mut length = spaces + form.len();
    if form.contains('.')
        && text[length..].starts_with('.')
        && text[length + 1..]
            .trim_start()
            .chars()
            .next()
            .is_some_and(|ch| ch.is_lowercase() || ch.is_ascii_digit() || ch == ',')
    {
        length += 1;
    }
    Some((length, name))
}
/// Whether text before a colon ends with a file name, as in `main.rs:12`.
fn file_name(before: &str) -> bool {
    before
        .rsplit(|ch: char| !(ch.is_alphanumeric() || "._-/\\".contains(ch)))
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, extension)| {
            !stem.is_empty()
                && (1..=5).contains(&extension.len())
                && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
                && extension.chars().any(|ch| ch.is_ascii_alphabetic())
        })
}
/// A currency written after an amount: `5€`, `5 £`, `5 USD`.
fn currency(text: &str) -> Option<(usize, char)> {
    let spaces = text.len() - text.trim_start_matches(' ').len();
    let rest = &text[spaces..];
    [
        ("€", '€'),
        ("£", '£'),
        ("₹", '₹'),
        ("USD", '$'),
        ("EUR", '€'),
        ("GBP", '£'),
        ("INR", '₹'),
    ]
    .into_iter()
    .find(|(form, _)| rest.starts_with(form) && boundary(rest[form.len()..].chars().next()))
    .map(|(form, symbol)| (spaces + form.len(), symbol))
}
fn money(raw: &str, symbol: char) -> String {
    let (singular, plural, cent, cents) = match symbol {
        '$' => ("dollar", "dollars", "cent", "cents"),
        '£' => ("pound", "pounds", "penny", "pence"),
        '₹' => ("rupee", "rupees", "paisa", "paise"),
        _ => ("euro", "euros", "cent", "cents"),
    };
    let parts: Vec<_> = raw.split('.').collect();
    let mut value = if parts.len() <= 2 && parts.get(1).is_none_or(|part| part.len() <= 2) {
        format!(
            "{} {}",
            cardinal(parts[0]),
            if parts[0] == "1" { singular } else { plural }
        )
    } else {
        format!("{} {plural}", numeric(raw, false))
    };
    if parts.len() == 2 && parts[1].len() <= 2 {
        let count: u64 = format!("{0:0<2}", parts[1]).parse().unwrap_or(0);
        if count > 0 {
            value.push_str(&format!(
                " and {} {}",
                integer(count),
                if count == 1 { cent } else { cents }
            ));
        }
    }
    value
}
fn clock(text: &str) -> Option<(usize, String)> {
    let captures = CLOCK.captures(text)?;
    let hour: u64 = captures[1].parse().ok()?;
    let minute: u64 = captures[2].parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    let mut value = if minute == 0 {
        format!("{} o'clock", integer(hour))
    } else {
        format!(
            "{} {}{}",
            integer(hour),
            if minute < 10 { "oh " } else { "" },
            integer(minute)
        )
    };
    if let Some(seconds) = captures.get(3) {
        let second: u64 = seconds.as_str().parse().ok()?;
        if second > 59 {
            return None;
        }
        value.push_str(&format!(" and {} seconds", integer(second)));
    }
    Some((captures[0].len(), value))
}

/// Whether a chunk has anything to say: `Ok(false)` for dividers, emoji-only lines and
/// other text with nothing to read, and an error worded for the user when the text
/// cannot be prepared. Neither may stop the rest of a reading silently.
pub fn speech_check(text: &str, replacements: &[WordReplacement]) -> Result<bool> {
    render(text, replacements).map(|spoken| !spoken.is_empty())
}

/// Markdown rules and table separators, such as `---` or `| --- | :-: |`.
fn separator_line(line: &str) -> bool {
    line.chars().all(|ch| {
        matches!(
            ch,
            '-' | '=' | '_' | '*' | '~' | '|' | ':' | '+' | ' ' | '\t'
        )
    }) && line
        .chars()
        .filter(|ch| matches!(ch, '-' | '=' | '_' | '*' | '~'))
        .count()
        >= 3
}

pub fn prepare(text: &str, replacements: &[WordReplacement]) -> Result<String> {
    let value = render(text, replacements)?;
    ensure!(!value.is_empty(), "No words or numbers to read");
    Ok(value)
}

/// Integers written as single symbols: circled, parenthesised and Roman numerals.
fn numeral(ch: char) -> Option<u64> {
    let code = ch as u32;
    let value = match code {
        0x24EA => 0,
        0x2460..=0x2473 => code - 0x2460 + 1,
        0x2474..=0x2487 => code - 0x2474 + 1,
        0x2488..=0x249B => code - 0x2488 + 1,
        0x24EB..=0x24F4 => code - 0x24EB + 11,
        0x2776..=0x277F => code - 0x2776 + 1,
        0x2780..=0x2789 => code - 0x2780 + 1,
        0x278A..=0x2793 => code - 0x278A + 1,
        0x2160..=0x216B => code - 0x2160 + 1,
        0x2170..=0x217B => code - 0x2170 + 1,
        _ => return None,
    };
    Some(value.into())
}

/// The spoken text, or an empty string when there is nothing to read.
fn render(text: &str, replacements: &[WordReplacement]) -> Result<String> {
    // Built-in aliases leave the user's own replacement text alone.
    let (text, replaced) = dictionary::replace_marked(&normalize(text), replacements);
    let mut output: Vec<String> = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        let ch = rest.chars().next().unwrap();
        let previous = text[..offset].chars().next_back();
        if ch.is_whitespace() {
            offset += ch.len_utf8();
            continue;
        }
        let line_start = text[..offset]
            .rsplit('\n')
            .next()
            .is_none_or(|line| line.trim().is_empty());
        if line_start {
            let line = rest.split('\n').next().unwrap_or(rest);
            if separator_line(line) {
                offset += line.len();
                continue;
            }
        }
        if ch == '#' && line_start {
            let count = rest.chars().take_while(|ch| *ch == '#').count();
            if rest[count..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                offset += count;
                continue;
            }
        }
        if ch == '*' {
            let count = rest.chars().take_while(|ch| *ch == '*').count();
            let after = rest[count..].chars().next();
            // A spaced power, as in `2 ** 10`, is not bold.
            let operator = count == 2
                && previous.is_some_and(char::is_whitespace)
                && after.is_some_and(char::is_whitespace)
                && text[..offset]
                    .trim_end()
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric)
                && rest[count..]
                    .trim_start()
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric);
            if !operator
                && ((line_start && after.is_some_and(char::is_whitespace))
                    || (count >= 2 && (boundary(previous) || boundary(after))))
            {
                offset += count;
                continue;
            }
        }
        if boundary(previous)
            && !replaced.iter().any(|range| range.contains(&offset))
            && let Some((key, value)) = ALIASES
                .iter()
                .filter(|(key, _)| {
                    rest.get(..key.len()).is_some_and(|prefix| {
                        if EXACT_ALIASES.contains(key) {
                            prefix == *key
                        } else {
                            prefix.eq_ignore_ascii_case(key)
                        }
                    }) && boundary(rest[key.len()..].chars().next())
                })
                .max_by_key(|(key, _)| key.len())
        {
            output.push(
                INITIALS
                    .replace_all(value, |captures: &regex::Captures<'_>| {
                        LETTER_NAMES[captures[0].as_bytes()[0] as usize - b'A' as usize]
                    })
                    .into_owned(),
            );
            offset += key.len();
            continue;
        }
        if ch.is_ascii_digit() {
            // Compiler locations such as `main.rs:12:30`.
            if previous == Some(':') && file_name(&text[..offset - 1]) {
                let raw = number_at(rest).unwrap();
                output.push(format!("line {}", numeric(raw, false)));
                offset += raw.len();
                if let Some(column) = text[offset..].strip_prefix(':').and_then(number_at) {
                    output.push(format!("column {}", numeric(column, false)));
                    offset += 1 + column.len();
                }
                continue;
            }
            if let Some((count, value)) = calendar(rest)
                .or_else(|| phone(rest))
                .or_else(|| slash(rest).filter(|_| previous != Some('/')))
            {
                output.push(value);
                offset += count;
                continue;
            }
            if let Some((count, mut value)) = clock(rest) {
                offset += count;
                if let Some((length, name)) = meridiem(&text[offset..]) {
                    value = format!("{} {name}", value.trim_end_matches(" o'clock"));
                    offset += length;
                }
                output.push(value);
                continue;
            }
            let raw = number_at(rest).unwrap();
            offset += raw.len();
            if let Some((length, symbol)) = currency(&text[offset..]) {
                output.push(money(raw, symbol));
                offset += length;
                continue;
            }
            let plain = raw.bytes().all(|byte| byte.is_ascii_digit());
            // Decades: 1990s, '90s, the 80s. A bare 30s may be seconds.
            let era = previous == Some('\'')
                || text[..offset - raw.len()]
                    .trim_end()
                    .rsplit(' ')
                    .next()
                    .is_some_and(|word| {
                        ["the", "early", "mid", "late", "my", "his", "her", "their"]
                            .iter()
                            .any(|known| word.eq_ignore_ascii_case(known))
                    });
            let spaces =
                text[offset..].len() - text[offset..].trim_start_matches([' ', '\t']).len();
            // 1990s and the 80s are decades, not seconds.
            let decade = plain
                && raw.ends_with('0')
                && (year_like(raw) || (raw.len() == 2 && raw != "00" && era));
            if let Some((suffix, expanded)) = unit(&text[offset + spaces..], raw == "1")
                && !(decade && spaces == 0 && suffix == "s")
            {
                output.push(numeric(raw, false));
                output.push(expanded);
                offset += spaces + suffix.len();
                continue;
            }
            if plain
                && raw.ends_with('0')
                && (year_like(raw) || (raw.len() == 2 && raw != "00" && era))
                && let Some(suffix) = ["s", "'s"].into_iter().find(|suffix| {
                    text[offset..].starts_with(suffix)
                        && boundary(text[offset + suffix.len()..].chars().next())
                })
            {
                output.push(plural_number(&if raw.len() == 4 {
                    year(raw.parse().unwrap())
                } else {
                    cardinal(raw)
                }));
                offset += suffix.len();
                continue;
            }
            let spoken = if year_like(raw)
                && previous != Some('#')
                && !identifier_before(&text[..offset - raw.len()])
            {
                year(raw.parse().unwrap())
            } else {
                numeric(raw, false)
            };
            if plain
                && raw.parse().is_ok_and(|hour: u64| (1..=12).contains(&hour))
                && let Some((length, name)) = meridiem(&text[offset..])
            {
                output.push(format!("{spoken} {name}"));
                offset += length;
                continue;
            }
            output.push(spoken);
            continue;
        }
        // Powers written as superscripts: x², m³, 10⁻³.
        if superscript(ch).is_some_and(|digit| digit != '-')
            || (ch == '⁻'
                && rest['⁻'.len_utf8()..]
                    .chars()
                    .next()
                    .and_then(superscript)
                    .is_some())
        {
            let length: usize = rest
                .chars()
                .take_while(|ch| superscript(*ch).is_some())
                .map(char::len_utf8)
                .sum();
            let run: String = rest[..length].chars().filter_map(superscript).collect();
            let (sign, power) = run
                .strip_prefix('-')
                .map_or(("", run.as_str()), |power| ("minus ", power));
            let power = power.replace('-', "");
            output.push(match (sign, power.as_str()) {
                ("", "2") => "squared".into(),
                ("", "3") => "cubed".into(),
                _ => format!("to the power of {sign}{}", cardinal(&power)),
            });
            offset += length;
            continue;
        }
        // Subscripts, as in CO₂, read as the number.
        if ('₀'..='₉').contains(&ch) {
            let run: String = rest
                .chars()
                .take_while(|ch| ('₀'..='₉').contains(ch))
                .filter_map(|ch| char::from_digit(ch as u32 - '₀' as u32, 10))
                .collect();
            output.push(cardinal(&run));
            offset += run.len() * '₀'.len_utf8();
            continue;
        }
        if let Some((_, numerator, denominator)) = [
            ('½', 1, 2),
            ('⅓', 1, 3),
            ('⅔', 2, 3),
            ('¼', 1, 4),
            ('¾', 3, 4),
            ('⅕', 1, 5),
            ('⅖', 2, 5),
            ('⅗', 3, 5),
            ('⅘', 4, 5),
            ('⅙', 1, 6),
            ('⅚', 5, 6),
            ('⅐', 1, 7),
            ('⅛', 1, 8),
            ('⅜', 3, 8),
            ('⅝', 5, 8),
            ('⅞', 7, 8),
            ('⅑', 1, 9),
            ('⅒', 1, 10),
        ]
        .into_iter()
        .find(|(symbol, ..)| *symbol == ch)
        {
            let value = fraction(numerator, denominator).unwrap();
            output.push(if previous.is_some_and(|ch| ch.is_ascii_digit()) {
                format!("and {value}")
            } else {
                value
            });
            offset += ch.len_utf8();
            continue;
        }
        if let Some(value) = numeral(ch) {
            output.push(integer(value));
            offset += ch.len_utf8();
            continue;
        }
        if matches!(ch, 'v' | 'V')
            && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
            && let Some(raw) = number_at(&rest[1..])
        {
            output.push(format!("version {}", numeric(raw, true)));
            offset += 1 + raw.len();
            continue;
        }
        if matches!(ch, '$' | '£' | '€' | '₹')
            && let Some(raw) = number_at(&rest[ch.len_utf8()..])
        {
            output.push(money(raw, ch));
            offset += ch.len_utf8() + raw.len();
            continue;
        }
        if ch.is_alphabetic()
            || (ch == '\'' && rest[1..].chars().next().is_some_and(char::is_alphabetic))
        {
            let end = rest
                .char_indices()
                .find(|(index, c)| !c.is_alphabetic() && !(*c == '\'' && *index > 0))
                .map_or(rest.len(), |(index, _)| index);
            // The leading quote is skipped; internal apostrophes stay with contractions.
            if end == 0 {
                offset += ch.len_utf8();
                continue;
            }
            let token = rest[..end].trim_matches('\'');
            if !token.is_empty() {
                let capitals =
                    |word: &str| word.chars().count() > 1 && word.chars().all(char::is_uppercase);
                let before = text[..offset]
                    .trim_end_matches([' ', '\t', ','])
                    .rsplit(|ch: char| !ch.is_alphabetic())
                    .next()
                    .unwrap_or("");
                let after = text[offset + end..]
                    .trim_start_matches([' ', '\t', ','])
                    .split(|ch: char| !ch.is_alphabetic())
                    .next()
                    .unwrap_or("");
                let shouted = capitals(before) || capitals(after);
                output.push(contraction(token).unwrap_or_else(|| word(token, shouted)));
            }
            offset += end;
            continue;
        }
        if ch == '.' && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
            if previous.is_some_and(|ch| ch.is_alphanumeric() || ch == '_') {
                offset += 1;
                continue;
            }
            let raw = number_at(&rest[1..]).unwrap();
            output.push(format!("zero point {}", digits(raw)));
            offset += 1 + raw.len();
            continue;
        }
        if let Some((operator, name)) = [
            ("!==", "not strictly equal to"),
            ("===", "strictly equals"),
            (">=", "greater than or equal to"),
            ("<=", "less than or equal to"),
            ("!=", "not equal to"),
            ("==", "equals"),
            ("&&", "and"),
            ("||", "or"),
            ("=>", "to"),
            ("->", "to"),
            ("::", ""),
            ("??", ""),
            ("?.", ""),
            ("++", "increment"),
            ("--", ""),
            ("**", "to the power of"),
            ("...", "…"),
        ]
        .into_iter()
        .find(|(operator, _)| rest.starts_with(operator))
        {
            if !name.is_empty() {
                output.push(name.into());
            }
            offset += operator.len();
            continue;
        }
        let next = rest[ch.len_utf8()..].chars().next();
        let name = match ch {
            '#' if line_start && (next.is_some_and(char::is_whitespace) || next == Some('#')) => "",
            '#' if next.is_some_and(|c| c.is_ascii_digit()) => "number",
            '#' => "",
            '-' if line_start && next.is_some_and(char::is_whitespace) => "",
            '-' if previous.is_some_and(char::is_alphabetic)
                && next.is_some_and(char::is_alphabetic) =>
            {
                ""
            }
            '-' if next.is_some_and(char::is_alphabetic) => "",
            // 3-5 is a range; x86-64 and SHA-256 are names.
            '-' if previous.is_some_and(|c| c.is_ascii_digit())
                && next.is_some_and(|c| c.is_ascii_digit()) =>
            {
                let before = text[..offset]
                    .rsplit(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or("");
                if before.bytes().all(|byte| byte.is_ascii_digit()) {
                    "to"
                } else {
                    ""
                }
            }
            // GPT-4 joins silently; a single letter, as in n-1, is arithmetic.
            '-' if previous.is_some_and(char::is_alphabetic)
                && next.is_some_and(|c| c.is_ascii_digit())
                && text[..offset]
                    .rsplit(|c: char| !c.is_alphabetic())
                    .next()
                    .is_some_and(|word| word.chars().count() > 1) =>
            {
                ""
            }
            '-' | '−' => "minus",
            '–' if previous.is_some_and(|c| c.is_ascii_digit())
                && next.is_some_and(|c| c.is_ascii_digit()) =>
            {
                "to"
            }
            '—' | '–' => ",",
            '+' => "plus",
            '=' => "equals",
            '>' => "greater than",
            '<' => "less than",
            '%' => "percent",
            '&' => "and",
            '|' => "",
            '/' if previous.is_some_and(|c| c.is_ascii_digit())
                && next.is_some_and(|c| c.is_ascii_digit()) =>
            {
                "over"
            }
            '/' => "",
            '÷' => "divided by",
            '\\' => "",
            '*' | '×' => "times",
            '^' => "to the power of",
            '~' => "",
            '@' => "at",
            '$' => "",
            '_' | '`' => "",
            '(' | ')' | '[' | ']' | '{' | '}' | ':' | ';' => "",
            '.' if next.is_some_and(char::is_alphabetic) => "",
            '!' if next.is_some_and(|c| c.is_alphanumeric() || c == '(') => "not",
            // A URL query, as in `?q=1`, is not a question.
            '?' if previous.is_some_and(|c| !c.is_whitespace())
                && next.is_some_and(char::is_alphanumeric) =>
            {
                ""
            }
            '.' | ',' | '?' | '!' | '…' => {
                output.push(ch.to_string());
                offset += ch.len_utf8();
                continue;
            }
            '"' | '“' | '”' | '‘' | '’' | '\'' => "",
            '≤' => "less than or equal to",
            '≥' => "greater than or equal to",
            '≠' => "not equal to",
            '±' => "plus or minus",
            '∞' => "infinity",
            '√' => "square root",
            '∑' => "sum",
            '∫' => "integral",
            'π' => "pi",
            '°' => "degrees",
            '→' => "to",
            '←' => "from",
            '↔' => "both directions",
            '✅' | '✓' => "check mark",
            '❌' | '✗' => "cross mark",
            '⚠' => "warning",
            _ => {
                ensure!(
                    !ch.is_numeric(),
                    "Add a word replacement for this number format"
                );
                offset += ch.len_utf8();
                continue;
            }
        };
        if !name.is_empty() {
            output.push(name.into());
        }
        offset += ch.len_utf8();
        ensure!(
            output.len() <= 8192,
            "Spoken text is too long. Shorten the word replacement."
        );
    }
    let mut value = output.join(" ");
    for mark in [".", ",", "?", "!", "…"] {
        value = value.replace(&format!(" {mark}"), mark);
    }
    let value = value.trim_matches([',', ' ']).to_owned();
    if !value.chars().any(char::is_alphabetic) {
        return Ok(String::new());
    }
    ensure!(
        value.chars().count() <= 8192,
        "Spoken text is too long. Shorten the word replacement."
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn developer_examples() {
        for (input, expected) in [
            ("API", "eigh pee eye"),
            ("PR #5", "pull request number five"),
            (
                "CPU >= 90%",
                "see pee you greater than or equal to ninety percent",
            ),
            ("v1.2.3", "version one point two point three"),
            ("0xFF", "hex ef ef"),
            (
                "1.2e-3",
                "one point two times ten to the power of minus three",
            ),
            (
                "$1,234.50",
                "one thousand two hundred thirty four dollars and fifty cents",
            ),
            ("getUserName(user_id)", "get User Name user eye dee"),
            ("C++ != C#", "see plus plus not equal to see sharp"),
            ("3.14", "three point one four"),
            ("₹120.50", "one hundred twenty rupees and fifty paise"),
            (
                "16GB and 0.5ms",
                "sixteen gigabytes and zero point five milliseconds",
            ),
        ] {
            assert_eq!(
                prepare(input, &dictionary::defaults()).unwrap(),
                expected,
                "{input}"
            );
        }
    }
    #[test]
    fn numbers_markdown_and_paths_remain_unambiguous() {
        for (input, expected) in [
            (
                "## **PR #5**\n- 12.5%",
                "pull request number five twelve point five percent",
            ),
            (
                "16 GB, v2, 007, 21st",
                "sixteen gigabytes, version two, zero zero seven, twenty first",
            ),
            (
                "1/2 + 2**3 <= 9",
                "one half plus two to the power of three less than or equal to nine",
            ),
            ("src/main.rs", "src main are ess"),
            (
                "2026-10-31 at 9:05",
                "October thirty first, twenty twenty six at nine oh five",
            ),
            (
                "0b1010 & 0o755",
                "binary one zero one zero and octal seven five five",
            ),
            (
                "123456789012345678901",
                "one two three four five six seven eight nine zero one two three four five six seven eight nine zero one",
            ),
        ] {
            assert_eq!(
                prepare(input, &dictionary::defaults()).unwrap(),
                expected,
                "{input}"
            );
        }
    }
    fn check(cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(
                prepare(input, &dictionary::defaults()).unwrap(),
                *expected,
                "{input}"
            );
        }
    }
    #[test]
    fn unicode_numbers_are_read_not_dropped() {
        check(&[
            ("x² + y³", "ex squared plus why cubed"),
            (
                "2¹⁰ and 10⁻³",
                "two to the power of ten and ten to the power of minus three",
            ),
            ("CO₂ and H₂O", "see oh two and aitch two oh"),
            (
                "½ cup, 1½, ⅗",
                "one half cup, one and one half, three fifths",
            ),
            (
                "１２３ and ٤٢ and ४२",
                "one hundred twenty three and forty two and forty two",
            ),
            ("step ③ of Ⅻ", "step three of twelve"),
            ("cafe\u{301} nai\u{308}ve", "café naïve"),
        ]);
        assert!(speech_check("m²", &[]).unwrap());
        assert!(speech_check("½", &[]).unwrap());
    }
    #[test]
    fn speech_check_separates_silence_from_failure() {
        for silent in ["---", "🙂🙂", "| --- |", "...", "```"] {
            assert!(!speech_check(silent, &[]).unwrap(), "{silent}");
        }
        assert!(speech_check("Section 2", &[]).unwrap());
        assert!(speech_check("\u{1F10C}", &[]).is_err());
        let long = [WordReplacement {
            word: "x".into(),
            replacement: "word ".repeat(39) + "word",
            match_case: false,
        }];
        assert!(speech_check(&"x ".repeat(300), &long).is_err());
        assert!(speech_check(&"x ".repeat(300), &long).is_err());
    }
    #[test]
    fn hyphens_next_to_digits_are_not_minus() {
        check(&[
            ("3-5 days", "three to five days"),
            ("2020-2025", "twenty twenty to twenty twenty five"),
            ("GPT-4", "gee pee tee four"),
            ("SHA-256", "ess aitch eigh two hundred fifty six"),
            ("COVID-19", "COVID nineteen"),
            ("x86-64", "ex eighty six sixty four"),
            ("555-1234", "five five five, one two three four"),
            (
                "1-800-555-1234",
                "one, eight zero zero, five five five, one two three four",
            ),
            ("100-1000", "one hundred to one thousand"),
            (
                "-5, x = -3, n-1",
                "minus five, ex equals minus three, en minus one",
            ),
            ("2026-11-03", "November third, twenty twenty six"),
        ]);
    }
    #[test]
    fn capitals_are_words_unless_they_are_acronyms() {
        check(&[
            ("DO NOT USE", "DO NOT USE"),
            ("HELLO WORLD", "HELLO WORLD"),
            ("THE end", "THE end"),
            ("NASA and HTML", "NASA and aitch tee em el"),
            ("PLEASE DON'T DEL IT", "PLEASE do not DEL IT"),
            ("IT department", "eye tee department"),
            (
                "the URL, the AWS key, RAM",
                "the you are el, the eigh double you ess key, are eigh em",
            ),
            ("a ram and crud", "a ram and crud"),
            ("XMLHttpRequest", "ex em el aitch tee tee pee Request"),
        ]);
    }
    #[test]
    fn plural_acronyms_keep_their_letters() {
        check(&[
            ("APIs", "eigh pee eye's"),
            ("URLs and CPUs", "you are el's and see pee you's"),
            ("IDs, GPUs", "eye dee's, gee pee you's"),
            (
                "two PRs and the PR's title",
                "two pull requests and the pull request's title",
            ),
            ("getAPIs", "get eigh pee eye's"),
        ]);
    }
    #[test]
    fn years_and_decades() {
        check(&[
            ("in 1999", "in nineteen ninety nine"),
            (
                "2026, 2000, 2005",
                "twenty twenty six, two thousand, two thousand five",
            ),
            ("1905 and 1900", "nineteen oh five and nineteen hundred"),
            (
                "the 1990s, '90s and the 80s",
                "the nineteen nineties, nineties and the eighties",
            ),
            (
                "timeout 30s, took 2.5 s or 1 s",
                "timeout thirty seconds, took two point five seconds or one second",
            ),
            ("5 min", "five minutes"),
            (
                "retry in 30s, in my 30s",
                "retry in thirty seconds, in my thirties",
            ),
            (
                "port 1234, PID 1999",
                "port one thousand two hundred thirty four, pee eye dee one thousand nine hundred ninety nine",
            ),
            (
                "1,999 items and 1999 MB",
                "one thousand nine hundred ninety nine items and one thousand nine hundred ninety nine megabytes",
            ),
            (
                "2048 tokens, 3000, 1999.5",
                "two thousand forty eight tokens, three thousand, one thousand nine hundred ninety nine point five",
            ),
        ]);
    }
    #[test]
    fn slashes_dates_fractions_and_paths() {
        check(&[
            ("11/03/2026", "November third, twenty twenty six"),
            ("1/5/26", "January fifth, twenty six"),
            ("3/4 and 2/3", "three quarters and two thirds"),
            ("11/03", "eleven over three"),
            ("24/7", "twenty four seven"),
            (
                "13/45/2026",
                "thirteen over forty five over twenty twenty six",
            ),
            ("src/main.rs", "src main are ess"),
        ]);
    }
    #[test]
    fn file_locations_are_not_times() {
        check(&[
            ("main.rs:12:30", "main are ess line twelve column thirty"),
            ("src/lib.rs:42", "src lib are ess line forty two"),
            ("at 10:30", "at ten thirty"),
            ("port:8080", "port eight thousand eighty"),
        ]);
    }
    #[test]
    fn times_of_day_and_letters() {
        check(&[
            ("10am", "ten eigh em"),
            ("10 am", "ten eigh em"),
            ("at 10 a.m. today", "at ten eigh em today"),
            ("by 3 p.m.", "by three pee em."),
            ("3PM", "three pee em"),
            ("10:30pm", "ten thirty pee em"),
            ("9:00 a.m", "nine eigh em"),
            ("Vitamin A is", "Vitamin A is"),
            ("A cat sat.", "A cat sat."),
        ]);
    }
    #[test]
    fn aliases_respect_case_and_user_text() {
        check(&[("my nan, Nan and NaN", "my nan, Nan and not a number")]);
        let rules = [WordReplacement {
            word: "gran".into(),
            replacement: "nan".into(),
            match_case: false,
        }];
        assert_eq!(prepare("NaN gran", &rules).unwrap(), "not a number nan");
        let rules = [WordReplacement {
            word: "x".into(),
            replacement: "todo".into(),
            match_case: false,
        }];
        assert_eq!(prepare("x TODO", &rules).unwrap(), "todo to do");
    }
    #[test]
    fn urls_powers_and_currency() {
        check(&[
            (
                "See https://x.dev/a?q=1 now.",
                "See aitch tee tee pee ess ex dev a cue equals one now.",
            ),
            ("2 ** 10", "two to the power of ten"),
            ("some **bold** text", "some bold text"),
            ("5€ and 5 £", "five euros and five pounds"),
            ("1 USD, 2.50 EUR", "one dollar, two euros and fifty cents"),
        ]);
    }
    #[test]
    fn dividers_are_silent_and_cannot_stop_a_reading() {
        for divider in ["---", "===", "***", "| --- | :-: |", "```", "...", "🙂🙂"] {
            assert!(!speech_check(divider, &[]).unwrap(), "{divider}");
        }
        assert_eq!(prepare("---\nNext", &[]).unwrap(), "Next");
        assert!(speech_check("a === b", &[]).unwrap());
        assert!(speech_check("Section 2", &[]).unwrap());
    }
    #[test]
    fn contractions_expand_without_user_setup() {
        for (input, expected) in [
            ("i'll", "I will"),
            ("I’ll", "I will"),
            ("I'm", "I am"),
            ("i’am", "I am"),
            ("I've", "I have"),
            ("I'd", "I would"),
            ("you're", "you are"),
            ("we'll", "we will"),
            ("they've", "they have"),
            ("don't", "do not"),
            ("isn’t", "is not"),
            ("can't", "cannot"),
            ("won’t", "will not"),
            ("should've", "should have"),
            ("that's", "that is"),
            ("let's", "let us"),
        ] {
            assert_eq!(
                prepare(input, &dictionary::defaults()).unwrap(),
                expected,
                "{input}"
            );
        }
    }
    #[test]
    fn punctuation_is_quiet_and_custom_rules_take_priority() {
        assert_eq!(
            prepare(
                "Result: main.rs:42. I'll check it.",
                &dictionary::defaults()
            )
            .unwrap(),
            "Result main are ess line forty two. I will check it."
        );
        assert_eq!(
            prepare("ERROR: CPU >= 90%", &dictionary::defaults()).unwrap(),
            "ERROR see pee you greater than or equal to ninety percent"
        );
        let rules = [WordReplacement {
            word: "I'll".into(),
            replacement: "tomorrow".into(),
            match_case: false,
        }];
        assert_eq!(
            prepare("I’ll: proceed.", &rules).unwrap(),
            "tomorrow proceed."
        );
        assert_eq!(
            prepare("A developer says I can.", &[]).unwrap(),
            "A developer says I can."
        );
    }
}
