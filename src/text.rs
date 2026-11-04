use anyhow::{Result, ensure};

pub const MAX_CHARACTERS: usize = 100_000;

pub fn validate(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "No text selected");
    ensure!(
        !text.contains('\0'),
        "Selected text contains an unsupported null character"
    );
    if text.len() > MAX_CHARACTERS {
        ensure!(
            text.chars().take(MAX_CHARACTERS + 1).count() <= MAX_CHARACTERS,
            "Select at most 100,000 characters"
        );
    }
    Ok(())
}

pub struct Chunks<'a> {
    remaining: &'a str,
    first: bool,
    first_limit: usize,
    following_limit: usize,
    boundary: Boundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Continuation,
    Sentence,
    Paragraph,
}

impl<'a> Chunks<'a> {
    pub fn boundary(&self) -> Boundary {
        self.boundary
    }
    pub fn has_more(&self) -> bool {
        !self.remaining.trim().is_empty()
    }

    pub fn new(text: &'a str) -> Self {
        Self::with_limits(text, 100, 180)
    }
    pub fn with_limits(text: &'a str, first: usize, following: usize) -> Self {
        Self {
            remaining: text,
            first: true,
            first_limit: first.clamp(8, 180),
            following_limit: following.clamp(8, 180),
            boundary: Boundary::Continuation,
        }
    }
}

/// First speech waits for the whole first chunk. An opening sentence longer than
/// `longest` ends early, preferring punctuation, then a word that starts a clause,
/// then any word break.
fn opening_clause(sentence: &str, longest: usize) -> Option<usize> {
    let shortest = longest * 2 / 5;
    const JOINERS: [&str; 10] = [
        "and ", "but ", "or ", "because ", "which ", "that ", "when ", "while ", "so ", "if ",
    ];
    if sentence.chars().count() <= longest {
        return None;
    }
    let (mut punctuation, mut joiner, mut space) = (None, None, None);
    let mut chars = sentence.char_indices().enumerate().peekable();
    while let Some((count, (index, ch))) = chars.next() {
        if count >= longest {
            break;
        }
        let spaced = chars
            .peek()
            .is_some_and(|(_, (_, next))| next.is_whitespace());
        if count >= shortest && spaced && matches!(ch, ',' | ':' | '—') {
            punctuation = Some(index + ch.len_utf8());
        }
        if ch.is_whitespace() {
            let rest = &sentence[index + ch.len_utf8()..];
            space = Some(index);
            if count >= shortest && JOINERS.iter().any(|word| rest.starts_with(word)) {
                joiner = Some(index);
            }
        }
    }
    punctuation.or(joiner).or(space)
}

/// Combining marks, joiners, variation selectors and skin tones continue the character
/// before them, so a cut never lands in front of one.
fn extends(ch: char) -> bool {
    matches!(ch,
        '\u{300}'..='\u{36f}'
        | '\u{1ab0}'..='\u{1aff}'
        | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}'
        | '\u{fe00}'..='\u{fe0f}'
        | '\u{fe20}'..='\u{fe2f}'
        | '\u{200d}'
        | '\u{1f3fb}'..='\u{1f3ff}'
        | '\u{e0020}'..='\u{e007f}')
}
/// A blank line follows: `\n\n`, `\r\n\r\n` or a line of spaces.
fn blank_line_follows(after: &str) -> bool {
    after
        .find(['\n', '\u{2029}'])
        .is_some_and(|end| after[..end].trim().is_empty())
}
/// A short unit after a number, as in `3.5 GB` or `10 ms`, stays with the number.
fn unit_follows(before: &str, after: &str) -> bool {
    before.ends_with(|ch: char| ch.is_ascii_digit())
        && after.split_whitespace().next().is_some_and(|word| {
            (1..=4).contains(&word.chars().count())
                && word
                    .chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, '%' | '/'))
        })
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        self.remaining = self.remaining.trim_start();
        if self.remaining.is_empty() {
            return None;
        }
        // The opening chunk is scanned like any other, then shortened at a clause break.
        let limit = self.following_limit.max(self.first_limit);
        let opening = std::mem::take(&mut self.first);
        let mut end = self.remaining.len();
        let (mut whitespace, mut earlier) = (None, None);
        for (count, (index, ch)) in self.remaining.char_indices().enumerate() {
            if count >= limit {
                end = match whitespace.filter(|at| *at > 0) {
                    Some(at)
                        if unit_follows(&self.remaining[..at], &self.remaining[at..])
                            && earlier.is_some_and(|at| at > 0) =>
                    {
                        earlier.unwrap()
                    }
                    Some(at) => at,
                    None => {
                        let mut cut = index;
                        while cut > 0
                            && (self.remaining[cut..].chars().next().is_some_and(extends)
                                || self.remaining[..cut].ends_with('\u{200d}'))
                        {
                            cut -= self.remaining[..cut]
                                .chars()
                                .next_back()
                                .unwrap()
                                .len_utf8();
                        }
                        if cut == 0 { index } else { cut }
                    }
                };
                break;
            }
            if ch.is_whitespace() {
                if whitespace.is_some_and(|at| at + 1 < index) {
                    earlier = whitespace;
                }
                whitespace = Some(index);
            }
            let spaced = self.remaining[index + ch.len_utf8()..]
                .chars()
                .next()
                .is_none_or(|next| {
                    next.is_whitespace()
                        || matches!(next, '!' | '?' | '"' | '”' | '\'' | '’' | ')' | ']')
                });
            // `?` inside a URL query is not the end of a sentence.
            if (matches!(ch, '!' | '?' | ';') && spaced)
                || ch == '\u{2029}'
                || (ch == '\n' && blank_line_follows(&self.remaining[index + 1..]))
            {
                end = index + ch.len_utf8();
                while let Some(next) = self.remaining[end..].chars().next() {
                    if !matches!(next, '!' | '?' | '"' | '”' | '\'' | '’' | ')' | ']') {
                        break;
                    }
                    end += next.len_utf8();
                }
                break;
            }
            if ch == '.' {
                let before = &self.remaining[..index];
                let word = before.split_whitespace().last().unwrap_or("");
                let suffix = &self.remaining[index + 1..];
                let closing = suffix.len()
                    - suffix
                        .trim_start_matches(['"', '”', '\'', '’', ')', ']'])
                        .len();
                let next = suffix[closing..].chars().next();
                let abbreviation = word.chars().count() == 1
                    || (word.contains('.')
                        && word.split('.').all(|part| part.chars().count() == 1))
                    || [
                        "Mr", "Mrs", "Ms", "Dr", "Prof", "St", "vs", "etc", "e.g", "i.e", "a.m",
                        "p.m",
                    ]
                    .iter()
                    .any(|prefix| word.eq_ignore_ascii_case(prefix));
                if !abbreviation && next.is_none_or(char::is_whitespace) {
                    end = index + 1 + closing;
                    break;
                }
            }
        }
        if opening && let Some(clause) = opening_clause(&self.remaining[..end], self.first_limit) {
            end = clause;
        }
        let (chunk, tail) = self.remaining.split_at(end);
        let leading = tail.len() - tail.trim_start().len();
        let paragraph = chunk
            .chars()
            .rev()
            .take_while(|ch| ch.is_whitespace())
            .chain(tail[..leading].chars())
            .map(|ch| match ch {
                '\u{2029}' => 2,
                '\n' => 1,
                _ => 0,
            })
            .sum::<usize>()
            >= 2;
        let ending = chunk
            .trim_end()
            .trim_end_matches(['"', '”', '\'', '’', ')', ']']);
        self.boundary = if paragraph {
            Boundary::Paragraph
        } else if ending.ends_with(['.', '!', '?', '…']) {
            Boundary::Sentence
        } else {
            Boundary::Continuation
        };
        self.remaining = tail;
        Some(chunk.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_reading_keeps_sentence_context() {
        let text = "Reading at your own pace can make a difficult paragraph easier to follow. Take a breath, then continue.";
        assert_eq!(
            Chunks::new(text).next().unwrap(),
            text.split_inclusive('.').next().unwrap()
        );
    }
    #[test]
    fn long_opening_sentence_starts_at_a_clause_break() {
        let text = "The committee reviewed the proposal over several long afternoons, and eventually agreed that the budget should be increased because the estimate was low.";
        let mut chunks = Chunks::new(text);
        assert_eq!(
            chunks.next(),
            Some("The committee reviewed the proposal over several long afternoons,")
        );
        assert_eq!(chunks.boundary(), Boundary::Continuation);
        assert!(chunks.next().unwrap().starts_with("and eventually"));
        let plain = "The committee reviewed a total of 1,000 proposals over many long afternoons and then agreed on every remaining detail of the plan today.";
        assert_eq!(
            Chunks::new(plain).next(),
            Some("The committee reviewed a total of 1,000 proposals over many long afternoons")
        );
        let unbroken = "word ".repeat(60);
        assert!(Chunks::new(&unbroken).next().unwrap().chars().count() <= 100);
    }
    #[test]
    fn a_shorter_opening_limit_still_prefers_clause_breaks() {
        let text =
            "After the long report finally arrives, read the summary first and then the details.";
        let mut chunks = Chunks::with_limits(text, 60, 180);
        assert_eq!(
            chunks.next(),
            Some("After the long report finally arrives,")
        );
        assert_eq!(
            chunks.next(),
            Some("read the summary first and then the details.")
        );
        let short = "Short opening sentences stay whole.";
        assert_eq!(Chunks::with_limits(short, 60, 180).next(), Some(short));
    }
    #[test]
    fn short_sentences_remain_separate() {
        assert_eq!(
            Chunks::new("Hello. Ready? Go!").collect::<Vec<_>>(),
            ["Hello.", "Ready?", "Go!"]
        );
    }
    #[test]
    fn quoted_sentences_abbreviations_and_paragraphs() {
        let mut chunks = Chunks::new("Dr. Lee said, “Ready?” Next sentence.\n\nA new paragraph.");
        assert_eq!(chunks.next(), Some("Dr. Lee said, “Ready?”"));
        assert_eq!(chunks.boundary(), Boundary::Sentence);
        assert_eq!(chunks.next(), Some("Next sentence."));
        assert_eq!(chunks.boundary(), Boundary::Paragraph);
        assert!(chunks.has_more());
        assert_eq!(chunks.next(), Some("A new paragraph."));
        assert!(!chunks.has_more());
        assert_eq!(
            Chunks::new("A wrapped\nline continues here.").collect::<Vec<_>>(),
            ["A wrapped\nline continues here."]
        );
    }
    #[test]
    fn chunks_preserve_content_and_bound_long_unpunctuated_input() {
        let text = "Reading naïve café words 🦀 safely ".repeat(4000);
        let chunks = Chunks::new(&text).collect::<Vec<_>>();
        assert!(chunks.len() > 100);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 180));
        let meaningful = |s: &str| {
            s.chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
        };
        assert_eq!(meaningful(&chunks.concat()), meaningful(&text));
    }
    #[test]
    fn limit_counts_unicode_not_utf8_bytes() {
        assert!(validate(&"é".repeat(100_000)).is_ok());
        assert!(validate(&"é".repeat(100_001)).is_err());
        assert!(validate(" \n ").is_err());
    }
    #[test]
    fn other_paragraph_breaks_and_urls() {
        for text in [
            "One.\r\n\r\nTwo.",
            "One.\n \nTwo.",
            "One.\u{2029}Two.",
            "One\n\t\nTwo.",
        ] {
            let mut chunks = Chunks::new(text);
            assert!(chunks.next().unwrap().starts_with("One"), "{text:?}");
            assert_eq!(chunks.boundary(), Boundary::Paragraph, "{text:?}");
            assert_eq!(chunks.next(), Some("Two."));
        }
        assert_eq!(
            Chunks::new("Open https://x.dev/a?q=1&b=2 now. Done?").collect::<Vec<_>>(),
            ["Open https://x.dev/a?q=1&b=2 now.", "Done?"]
        );
    }
    #[test]
    fn hard_cuts_keep_units_and_marks_together() {
        let text = format!("{}3.5 GB more words", "word ".repeat(35));
        let first = Chunks::with_limits(&text, 180, 180).next().unwrap();
        assert!(first.ends_with("word"), "{first}");
        let marked = "e\u{301}".repeat(200);
        for chunk in Chunks::new(&marked) {
            assert!(!chunk.starts_with('\u{301}'));
        }
        let family = "👩\u{200d}👩\u{200d}👧".repeat(60);
        for chunk in Chunks::new(&family) {
            assert!(!chunk.starts_with('\u{200d}') && !chunk.ends_with('\u{200d}'));
        }
    }
    /// Deterministic xorshift input of mixed scripts, symbols, digits and markup.
    fn random_text(seed: u64, length: usize) -> String {
        let pool: Vec<char> =
            "aZ09 -./:,?!*#_'\"$€£²₂½①Ⅻ٤１\u{301}\u{308}\u{200D}🙂éß→≥—–\n\t\r\u{2029}%^~|"
                .chars()
                .collect();
        let mut state = seed | 1;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if state.is_multiple_of(5) {
                    char::from_u32((state >> 20) as u32 % 0x3000).unwrap_or('x')
                } else {
                    pool[(state >> 8) as usize % pool.len()]
                }
            })
            .collect()
    }
    #[test]
    fn random_input_chunks_without_panics_or_loss() {
        let meaningful = |s: &str| {
            s.chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
        };
        for seed in 0..400 {
            let text = random_text(seed, 1 + seed as usize % 600);
            let chunks: Vec<_> = Chunks::with_limits(&text, 8 + seed as usize % 100, 180).collect();
            assert!(chunks.iter().all(|chunk| !chunk.is_empty()));
            assert_eq!(meaningful(&chunks.concat()), meaningful(&text), "{seed}");
            for chunk in chunks {
                let _ = crate::spoken::prepare(chunk, &crate::dictionary::defaults());
                let _ = crate::spoken::speech_check(chunk, &[]);
            }
        }
    }
    #[test]
    fn one_long_token_is_not_lost() {
        let text = "a".repeat(1000);
        assert_eq!(Chunks::new(&text).collect::<String>(), text);
    }
}
