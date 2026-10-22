//! Building the Windows Search "advanced query syntax" (AQS) string.
//!
//! User text is never pasted into the query as-is: every word is reduced to letters, digits and a
//! few safe punctuation marks and then quoted, so typed text cannot add operators, property
//! filters or boolean logic to the query.

use crate::timeutil::{date_range, iso_local_date};
use crate::Request;

pub const MAX_WORDS: usize = 8;
pub const MAX_WORD_CHARS: usize = 64;

/// Keep letters (any script), digits and `. _ -`. Everything else is dropped.
pub fn sanitize_word(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(MAX_WORD_CHARS)
        .collect()
}

/// Split free text into at most `MAX_WORDS` sanitised words.
pub fn words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(sanitize_word)
        .filter(|w| !w.is_empty())
        .take(MAX_WORDS)
        .collect()
}

/// One search word as an AQS clause: the file name contains this text.
///
/// Verified against the real index: `~=` is a substring match (finds "Zebrafish" for "fish"),
/// while `:` only matches word starts and `~"word"` matched nothing.
fn word_clause(word: &str) -> String {
    format!("System.ItemNameDisplay:~=\"{word}\"")
}

pub fn build_aqs(req: &Request, now_utc: i64, tz_offset: i64) -> String {
    let mut parts: Vec<String> = req
        .words
        .iter()
        .map(|w| word_clause(&sanitize_word(w)))
        .collect();
    if let Some(since) = req.since {
        let r = date_range(since, now_utc, tz_offset);
        parts.push(format!(
            "System.DateModified:>={}",
            iso_local_date(r.from, tz_offset)
        ));
        if let Some(to) = r.to {
            parts.push(format!(
                "System.DateModified:<{}",
                iso_local_date(to, tz_offset)
            ));
        }
    }
    parts.join(" ")
}
