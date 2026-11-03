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
static INITIALS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z]\b").unwrap());
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
    "uri", "uuid", "id", "ide", "ip", "tcp", "udp", "dns", "ssh", "ssl", "tls", "jwt", "jsx",
    "tsx", "js", "ts", "rs", "rb", "py", "npm", "pnpm", "npx", "ci", "cd", "crud", "orm", "oop",
    "ram", "rom", "ssd", "hdd", "ascii", "utf", "tts", "llm", "ai",
];

fn word(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for index in 1..chars.len() {
        if chars[index].is_uppercase()
            && (chars[index - 1].is_lowercase()
                || (chars[index - 1].is_uppercase()
                    && chars.get(index + 1).is_some_and(|ch| ch.is_lowercase())))
        {
            parts.push(chars[start..index].iter().collect::<String>());
            start = index;
        }
    }
    parts.push(chars[start..].iter().collect::<String>());
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
            if (part.len() > 1
                && part.len() <= 5
                && part.chars().all(|ch| ch.is_ascii_uppercase())
                && !ordinary_word)
                || ACRONYMS
                    .iter()
                    .any(|known| part.eq_ignore_ascii_case(known))
            {
                part.chars()
                    .map(|ch| {
                        LETTER_NAMES[ch.to_ascii_uppercase() as usize - b'A' as usize].to_owned()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                part
            }
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
    let year: u64 = captures[1].parse().ok()?;
    let month: usize = captures[2].parse().ok()?;
    let day: u64 = captures[3].parse().ok()?;
    let days = [
        31,
        if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
            29
        } else {
            28
        },
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
    let name = [
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
    ][month - 1];
    Some((
        captures[0].len(),
        format!("{name} {} {}", ordinal(&day.to_string()), integer(year)),
    ))
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

/// Whether a chunk has anything to say. Dividers and emoji-only lines do not, and
/// they must not stop the rest of a reading.
pub fn speakable(text: &str, replacements: &[WordReplacement]) -> bool {
    prepare(text, replacements).is_ok_and(|spoken| spoken.chars().any(char::is_alphanumeric))
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
    let text = dictionary::replace(text, replacements);
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
            if (line_start && after.is_some_and(char::is_whitespace))
                || (count >= 2 && (boundary(previous) || boundary(after)))
            {
                offset += count;
                continue;
            }
        }
        if boundary(previous)
            && let Some((key, value)) = ALIASES
                .iter()
                .filter(|(key, _)| {
                    rest.get(..key.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(key))
                        && boundary(rest[key.len()..].chars().next())
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
            if let Some((count, value)) = calendar(rest).or_else(|| clock(rest)) {
                output.push(value);
                offset += count;
                continue;
            }
            let raw = number_at(rest).unwrap();
            output.push(numeric(raw, false));
            offset += raw.len();
            let spaces =
                text[offset..].len() - text[offset..].trim_start_matches([' ', '\t']).len();
            if let Some((suffix, expanded)) = unit(&text[offset + spaces..], raw == "1") {
                output.push(expanded);
                offset += spaces + suffix.len();
            }
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
            let (singular, plural, cent, cents) = match ch {
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
            output.push(value);
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
                output.push(contraction(token).unwrap_or_else(|| word(token)));
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
            '½' => "one half",
            '¼' => "one quarter",
            '¾' => "three quarters",
            '⅓' => "one third",
            '⅔' => "two thirds",
            '⅛' => "one eighth",
            '⅜' => "three eighths",
            '⅝' => "five eighths",
            '⅞' => "seven eighths",
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
    ensure!(
        value.chars().any(char::is_alphabetic),
        "No words or numbers to read"
    );
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
                "one over two plus two to the power of three less than or equal to nine",
            ),
            ("src/main.rs", "src main are ess"),
            (
                "2026-10-31 at 9:05",
                "October thirty first two thousand twenty six at nine oh five",
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
    #[test]
    fn dividers_are_silent_and_cannot_stop_a_reading() {
        for divider in ["---", "===", "***", "| --- | :-: |", "```", "...", "🙂🙂"] {
            assert!(!speakable(divider, &[]), "{divider}");
        }
        assert_eq!(prepare("---\nNext", &[]).unwrap(), "Next");
        assert!(speakable("a === b", &[]));
        assert!(speakable("Section 2", &[]));
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
            "Result main are ess forty two. I will check it."
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
