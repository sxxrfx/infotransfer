use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordReplacement {
    pub word: String,
    pub replacement: String,
    #[serde(default)]
    pub match_case: bool,
}
pub fn defaults() -> Vec<WordReplacement> {
    [
        ("PR", "pull request"),
        ("repo", "repository"),
        ("deps", "dependencies"),
    ]
    .into_iter()
    .map(|(word, replacement)| WordReplacement {
        word: word.into(),
        replacement: replacement.into(),
        match_case: false,
    })
    .collect()
}
pub fn validate(entries: &[WordReplacement]) -> Result<()> {
    ensure!(entries.len() <= 200, "Keep at most 200 word replacements");
    let canonical: Vec<_> = entries
        .iter()
        .map(|entry| entry.word.replace(['’', '‘', 'ʼ', '＇'], "'"))
        .collect();
    for (index, entry) in entries.iter().enumerate() {
        ensure!(
            !entry.word.is_empty()
                && entry.word.chars().count() <= 64
                && !entry.word.chars().any(char::is_whitespace)
                && !entry.word.contains('\0'),
            "Enter one word or abbreviation, up to 64 characters"
        );
        ensure!(
            !entry.replacement.trim().is_empty()
                && entry.replacement.chars().count() <= 200
                && !entry.replacement.chars().any(char::is_control),
            "Enter a spoken replacement, up to 200 characters"
        );
        ensure!(
            !entries[..index]
                .iter()
                .enumerate()
                .any(
                    |(prior_index, prior)| if prior.match_case && entry.match_case {
                        canonical[prior_index] == canonical[index]
                    } else {
                        canonical[prior_index].eq_ignore_ascii_case(&canonical[index])
                    }
                ),
            "A replacement for this word already exists"
        );
    }
    Ok(())
}
fn word_character(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
fn word_before(text: &str) -> bool {
    let mut chars = text.chars().rev();
    match chars.next() {
        Some('\'') => chars.next().is_some_and(word_character),
        ch => ch.is_some_and(word_character),
    }
}
fn word_after(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some('\'') => chars.next().is_some_and(word_character),
        ch => ch.is_some_and(word_character),
    }
}

/// Literal, whole-word and non-recursive. Longest matching entry wins.
pub fn replace(text: &str, entries: &[WordReplacement]) -> String {
    let canonical = text.replace(['’', '‘', 'ʼ', '＇'], "'");
    let text = canonical.as_str();
    let words: Vec<_> = entries
        .iter()
        .map(|entry| (entry, entry.word.replace(['’', '‘', 'ʼ', '＇'], "'")))
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        let matched = words
            .iter()
            .filter(|(entry, word)| {
                let Some(candidate) = rest.get(..word.len()) else {
                    return false;
                };
                let equal = if entry.match_case {
                    candidate == word
                } else {
                    candidate.eq_ignore_ascii_case(word)
                };
                equal
                    && !(word.chars().next().is_some_and(word_character)
                        && word_before(&text[..offset]))
                    && !(word.chars().next_back().is_some_and(word_character)
                        && word_after(&rest[word.len()..]))
            })
            .max_by_key(|(_, word)| word.len());
        if let Some((entry, word)) = matched {
            output.push_str(&entry.replacement);
            offset += word.len();
        } else {
            let ch = rest.chars().next().unwrap();
            output.push(ch);
            offset += ch.len_utf8();
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(word: &str, replacement: &str, match_case: bool) -> WordReplacement {
        WordReplacement {
            word: word.into(),
            replacement: replacement.into(),
            match_case,
        }
    }
    #[test]
    fn words_are_literal_bounded_and_non_recursive() {
        let rules = [
            entry("PR", "pull request", false),
            entry("request", "task", false),
            entry("C++", "C plus plus", true),
            entry("US", "United States", true),
        ];
        assert_eq!(
            replace("pr #5, SPR, PR2, C++ and us/US", &rules),
            "pull request #5, SPR, PR2, C plus plus and us/United States"
        );
        assert!(validate(&rules).is_ok());
        assert!(validate(&[entry("PR", "one", false), entry("pr", "two", true)]).is_err());
        assert!(validate(&[entry("", "one", false)]).is_err());
        assert!(validate(&[entry("two words", "one", false)]).is_err());
    }
    #[test]
    fn apostrophe_styles_share_the_same_user_replacement() {
        let rules = [entry("I'll", "my override", false)];
        assert_eq!(
            replace("I’ll and i'll", &rules),
            "my override and my override"
        );
        assert!(validate(&[entry("I'll", "one", false), entry("I’ll", "two", false)]).is_err());
    }
    #[test]
    fn replacements_do_not_break_contractions_and_work_inside_quotes() {
        let rules = [
            entry("can", "able", false),
            entry("PR", "pull request", false),
        ];
        assert_eq!(
            replace("can't can 'PR'", &rules),
            "can't able 'pull request'"
        );
    }
}
