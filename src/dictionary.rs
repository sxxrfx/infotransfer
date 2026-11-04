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
                        canonical[prior_index].to_lowercase() == canonical[index].to_lowercase()
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

/// Bytes of `text` that start with `word`, compared case-insensitively by Unicode lowercase.
fn prefix_len(text: &str, word: &str, match_case: bool) -> Option<usize> {
    if match_case {
        return text.starts_with(word).then_some(word.len());
    }
    let mut length = 0;
    let mut chars = text.chars();
    for expected in word.chars() {
        let ch = chars.next()?;
        if !ch.to_lowercase().eq(expected.to_lowercase()) {
            return None;
        }
        length += ch.len_utf8();
    }
    Some(length)
}
fn plural(replacement: &str) -> String {
    if replacement.ends_with(['s', 'x', 'z'])
        || replacement.ends_with("ch")
        || replacement.ends_with("sh")
    {
        format!("{replacement}es")
    } else if let Some(stem) = replacement.strip_suffix('y')
        && !stem.ends_with(['a', 'e', 'i', 'o', 'u'])
    {
        format!("{stem}ies")
    } else {
        format!("{replacement}s")
    }
}

/// Literal, whole-word and non-recursive. Longest matching entry wins.
pub fn replace(text: &str, entries: &[WordReplacement]) -> String {
    replace_marked(text, entries).0
}

/// Like `replace`, also returning the byte ranges of the output written by replacements,
/// so later rules can leave them alone. An all-caps entry such as `PR` also covers its
/// plural and possessive, `PRs` and `PR's`, written in capitals.
pub fn replace_marked(
    text: &str,
    entries: &[WordReplacement],
) -> (String, Vec<std::ops::Range<usize>>) {
    let canonical = text.replace(['’', '‘', 'ʼ', '＇'], "'");
    let text = canonical.as_str();
    let words: Vec<_> = entries
        .iter()
        .map(|entry| (entry, entry.word.replace(['’', '‘', 'ʼ', '＇'], "'")))
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut marked = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        let matched = words
            .iter()
            .filter_map(|(entry, word)| {
                let length = prefix_len(rest, word, entry.match_case)?;
                let acronym = word.chars().count() >= 2
                    && word.chars().all(|ch| ch.is_uppercase())
                    && rest[..length].chars().all(char::is_uppercase);
                let suffix = ["'s", "s"]
                    .into_iter()
                    .find(|suffix| {
                        acronym
                            && rest[length..].starts_with(suffix)
                            && !word_after(&rest[length + suffix.len()..])
                    })
                    .unwrap_or("");
                let length = length + suffix.len();
                (!(word.chars().next().is_some_and(word_character) && word_before(&text[..offset]))
                    && !(word.chars().next_back().is_some_and(word_character)
                        && word_after(&rest[length..])))
                .then_some((entry, length, suffix))
            })
            .max_by_key(|(_, length, _)| *length);
        if let Some((entry, length, suffix)) = matched {
            let start = output.len();
            match suffix {
                "s" => output.push_str(&plural(&entry.replacement)),
                suffix => {
                    output.push_str(&entry.replacement);
                    output.push_str(suffix);
                }
            }
            marked.push(start..output.len());
            offset += length;
        } else {
            let ch = rest.chars().next().unwrap();
            output.push(ch);
            offset += ch.len_utf8();
        }
    }
    (output, marked)
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
    fn case_folding_is_unicode_and_acronym_plurals_follow_the_entry() {
        let rules = [
            entry("école", "school", false),
            entry("PR", "pull request", false),
            entry("box", "crate", false),
        ];
        assert_eq!(replace("ÉCOLE and École", &rules), "school and school");
        assert!(validate(&[entry("école", "one", false), entry("ÉCOLE", "two", false)]).is_err());
        assert_eq!(
            replace("PRs, PR's, prs and boxes", &rules),
            "pull requests, pull request's, prs and boxes"
        );
        let (text, marked) = replace_marked("my PR", &rules);
        assert_eq!(text, "my pull request");
        assert_eq!(marked.len(), 1);
        assert_eq!(&text[marked[0].clone()], "pull request");
        assert_eq!(plural("proxy"), "proxies");
        assert_eq!(plural("key"), "keys");
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
