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
        let mut whitespace = None;
        for (count, (index, ch)) in self.remaining.char_indices().enumerate() {
            if count >= limit {
                end = whitespace.filter(|at| *at > 0).unwrap_or(index);
                break;
            }
            if ch.is_whitespace() {
                whitespace = Some(index);
            }
            if matches!(ch, '!' | '?' | ';')
                || (ch == '\n' && self.remaining[index..].starts_with("\n\n"))
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
        if opening && let Some(clause) = opening_clause(&self.remaining[..end], self.first_limit)
        {
            end = clause;
        }
        let (chunk, tail) = self.remaining.split_at(end);
        let leading = tail.len() - tail.trim_start().len();
        let paragraph = chunk
            .chars()
            .rev()
            .take_while(|ch| ch.is_whitespace())
            .chain(tail[..leading].chars())
            .filter(|ch| *ch == '\n')
            .count()
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
        let text = "After the long report finally arrives, read the summary first and then the details.";
        let mut chunks = Chunks::with_limits(text, 60, 180);
        assert_eq!(chunks.next(), Some("After the long report finally arrives,"));
        assert_eq!(chunks.next(), Some("read the summary first and then the details."));
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
    fn one_long_token_is_not_lost() {
        let text = "a".repeat(1000);
        assert_eq!(Chunks::new(&text).collect::<String>(), text);
    }
}
