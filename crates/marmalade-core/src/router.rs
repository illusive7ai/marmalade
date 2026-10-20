//! Local command router: free text in, a validated `Action` (or a hint) out.
//!
//! Deterministic pattern matching only: instant, offline, no model. A language model, if one is
//! ever plugged in for fuzzy requests, must produce the same `Action` type and pass the same
//! `Action::validate()`; it never gets a shell or a free-form string to run.

use crate::action::{Action, FileKind, FileRef, FindSpec, Format, PrintOptions, Since, MAX_COPIES};
use crate::paths;

pub const MAX_INPUT_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// A complete, validated action.
    Intent(Action),
    /// The verb was understood but the request is incomplete or an argument was refused.
    NeedsMore(String),
    /// Not something the local rules recognise.
    Unknown,
}

struct Tok {
    text: String,
    lower: String,
    quoted: bool,
}

fn push_tok(toks: &mut Vec<Tok>, cur: &mut String, quoted: bool) {
    let mut text = std::mem::take(cur);
    if !quoted {
        text = text.trim_end_matches(['?', '!', ',', ';', '.']).to_string();
    }
    if text.is_empty() {
        return;
    }
    toks.push(Tok {
        lower: text.to_lowercase(),
        text,
        quoted,
    });
}

fn tokenize(input: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in input.chars() {
        match c {
            '"' | '\u{201c}' | '\u{201d}' => {
                push_tok(&mut toks, &mut cur, in_quote);
                in_quote = !in_quote;
            }
            c if c.is_whitespace() && !in_quote => push_tok(&mut toks, &mut cur, false),
            c if c.is_control() => {}
            c => cur.push(c),
        }
    }
    push_tok(&mut toks, &mut cur, in_quote);
    toks
}

const LEAD: &[&str] = &[
    "please",
    "pls",
    "hey",
    "hi",
    "hello",
    "marmalade",
    "marma",
    "can",
    "could",
    "would",
    "will",
    "you",
    "i",
    "want",
    "wanna",
    "need",
    "to",
    "like",
    "kindly",
    "just",
    "lets",
    "let's",
    "go",
    "ahead",
];

const CONTEXT_PHRASES: &[&str] = &[
    "this",
    "it",
    "that",
    "this file",
    "that file",
    "this one",
    "that one",
    "this document",
    "that document",
    "file",
    "the file",
    "document",
];

fn is_context(joined_lower: &str) -> bool {
    CONTEXT_PHRASES.contains(&joined_lower)
}

/// Turn the leftover words into a file reference.
fn file_ref(toks: &[&Tok]) -> Result<FileRef, String> {
    if toks.is_empty() {
        return Ok(FileRef::Context);
    }
    let joined = toks
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let lower = joined.to_lowercase();
    if is_context(&lower) {
        return Ok(FileRef::Context);
    }
    if toks.iter().any(|t| t.quoted) || paths::looks_like_path(&joined) {
        return paths::check(&joined)
            .map(FileRef::Path)
            .map_err(|e| e.to_string());
    }
    Ok(FileRef::Query(joined))
}

fn finish(action: Action) -> Routed {
    match action.validate() {
        Ok(()) => Routed::Intent(action),
        Err(msg) => Routed::NeedsMore(msg),
    }
}

pub fn route(input: &str) -> Routed {
    if input.chars().count() > MAX_INPUT_CHARS {
        return Routed::NeedsMore(format!(
            "That is too long (max {MAX_INPUT_CHARS} characters)."
        ));
    }
    let all = tokenize(input);
    let start = all
        .iter()
        .take_while(|t| !t.quoted && LEAD.contains(&t.lower.as_str()))
        .count();
    let toks = &all[start..];
    let Some(first) = toks.first() else {
        return Routed::Unknown;
    };
    let f = first.lower.as_str();
    let has = |w: &str| toks.iter().any(|t| !t.quoted && t.lower == w);

    if f == "help" || f == "commands" || (f == "what" && has("do") && (has("you") || has("can"))) {
        return Routed::Intent(Action::Help);
    }
    if has("printers") && toks.len() <= 4 {
        return Routed::Intent(Action::ListPrinters);
    }
    if f == "print" {
        return route_print(&toks[1..]);
    }
    if matches!(
        f,
        "convert" | "turn" | "change" | "make" | "save" | "export" | "transform"
    ) {
        let has_format = toks[1..]
            .iter()
            .any(|t| !t.quoted && Format::parse(&t.lower).is_some());
        if has_format || f == "convert" {
            return route_convert(&toks[1..]);
        }
    }
    let safety_word = [
        "virus",
        "viruses",
        "malware",
        "malicious",
        "infected",
        "safe",
    ]
    .iter()
    .any(|w| has(w));
    if f == "scan"
        || (safety_word
            && matches!(
                f,
                "check" | "is" | "does" | "analyze" | "analyse" | "test" | "verify" | "virus"
            ))
    {
        return route_scan(&toks[1..]);
    }
    if matches!(
        f,
        "find" | "search" | "locate" | "where" | "wheres" | "where's" | "show" | "look" | "get"
    ) {
        return route_find(&toks[1..]);
    }
    Routed::Unknown
}

const FIND_NOISE: &[&str] = &[
    "please",
    "my",
    "the",
    "a",
    "an",
    "for",
    "me",
    "is",
    "are",
    "where",
    "wheres",
    "where's",
    "i",
    "of",
    "from",
    "in",
    "on",
    "that",
    "this",
    "last",
    "past",
    "edited",
    "modified",
    "opened",
    "downloaded",
    "saved",
    "made",
    "created",
    "file",
    "files",
    "called",
    "named",
    "with",
    "about",
    "all",
    "any",
    "some",
    "it",
    "there",
    "was",
    "were",
    "did",
    "put",
    "have",
    "had",
    "got",
    "show",
    "find",
    "search",
    "locate",
    "look",
    "get",
    "up",
    "out",
    "to",
    "around",
    "anywhere",
    "which",
    "what",
    "whats",
    "what's",
    "ago",
    "pls",
];

fn kind_of(word: &str) -> Option<FileKind> {
    Some(match word {
        "pdf" | "pdfs" => FileKind::Pdf,
        "excel" | "spreadsheet" | "spreadsheets" | "xlsx" | "xls" | "csv" => FileKind::Spreadsheet,
        "word" | "docx" | "doc" => FileKind::Document,
        "powerpoint" | "pptx" | "ppt" | "slides" | "presentation" | "presentations" => {
            FileKind::Presentation
        }
        "photo" | "photos" | "picture" | "pictures" | "image" | "images" | "pic" | "pics"
        | "screenshot" | "screenshots" => FileKind::Image,
        "video" | "videos" | "movie" | "movies" | "clip" | "clips" => FileKind::Video,
        "music" | "song" | "songs" | "audio" | "recording" | "recordings" => FileKind::Audio,
        "zip" | "zips" | "archive" | "archives" => FileKind::Archive,
        _ => return None,
    })
}

fn route_find(toks: &[Tok]) -> Routed {
    let mut words: Vec<&str> = Vec::new();
    let mut kind = None;
    let mut since = None;
    let mut doc_word = false;

    for t in toks {
        if t.quoted {
            words.push(&t.text);
            continue;
        }
        let w = t.lower.as_str();
        match w {
            "today" => since = Some(Since::Today),
            "yesterday" => since = Some(Since::Yesterday),
            "week" | "weeks" | "recent" | "recently" | "latest" => since = Some(Since::Week),
            "month" | "months" => since = Some(Since::Month),
            "document" | "documents" | "docs" => doc_word = true,
            _ if FIND_NOISE.contains(&w) => {}
            _ => match kind_of(w) {
                Some(k) if kind.is_none() => kind = Some(k),
                _ => words.push(&t.text),
            },
        }
    }

    let mut query = words.join(" ");
    if query.is_empty() && doc_word {
        if kind.is_none() && since.is_none() {
            query = "document".to_string();
        } else if kind.is_none() {
            kind = Some(FileKind::Document);
        }
    }
    if query.is_empty() && kind.is_none() && since.is_none() {
        return Routed::NeedsMore("Find what? Try \u{201c}find my CV\u{201d} or \u{201c}find PDFs from last week\u{201d}.".into());
    }
    finish(Action::Find(FindSpec { query, kind, since }))
}

/// Mark the first run of tokens matching `phrase` as consumed.
fn take_phrase(toks: &[Tok], skip: &mut [bool], phrase: &[&str]) -> bool {
    if phrase.is_empty() || toks.len() < phrase.len() {
        return false;
    }
    for i in 0..=toks.len() - phrase.len() {
        let hit = phrase
            .iter()
            .enumerate()
            .all(|(j, p)| !skip[i + j] && !toks[i + j].quoted && toks[i + j].lower == *p);
        if hit {
            skip[i..i + phrase.len()].iter_mut().for_each(|s| *s = true);
            return true;
        }
    }
    false
}

const PRINT_NOISE: &[&str] = &[
    "please", "pls", "my", "the", "a", "an", "out", "of", "for", "me", "on", "in", "with", "and",
    "then", "now", "off", "up",
];

fn route_print(toks: &[Tok]) -> Routed {
    let mut skip = vec![false; toks.len()];
    let mut opts = PrintOptions::default();

    // "<n> copies" / "<n> copy"
    for i in 0..toks.len().saturating_sub(1) {
        if toks[i].quoted || skip[i] || skip[i + 1] {
            continue;
        }
        if matches!(toks[i + 1].lower.as_str(), "copies" | "copy") {
            if let Ok(n) = toks[i].lower.parse::<u32>() {
                if n == 0 || n > MAX_COPIES as u32 {
                    return Routed::NeedsMore(format!("I can print 1 to {MAX_COPIES} copies."));
                }
                opts.copies = n as u8;
                skip[i] = true;
                skip[i + 1] = true;
            }
        }
    }
    for (i, t) in toks.iter().enumerate() {
        if t.quoted || skip[i] {
            continue;
        }
        match t.lower.as_str() {
            "twice" => {
                opts.copies = 2;
                skip[i] = true;
            }
            "thrice" => {
                opts.copies = 3;
                skip[i] = true;
            }
            "duplex" | "double-sided" | "two-sided" | "2-sided" => {
                opts.duplex = true;
                skip[i] = true;
            }
            "grayscale" | "greyscale" | "monochrome" | "b&w" | "bw" | "mono" => {
                opts.grayscale = true;
                skip[i] = true;
            }
            _ => {}
        }
    }
    if take_phrase(toks, &mut skip, &["double", "sided"])
        || take_phrase(toks, &mut skip, &["both", "sides"])
    {
        opts.duplex = true;
    }
    if take_phrase(toks, &mut skip, &["black", "and", "white"]) {
        opts.grayscale = true;
    }

    let target: Vec<&Tok> = toks
        .iter()
        .enumerate()
        .filter(|(i, t)| !skip[*i] && (t.quoted || !PRINT_NOISE.contains(&t.lower.as_str())))
        .map(|(_, t)| t)
        .collect();
    match file_ref(&target) {
        Ok(file) => finish(Action::Print { file, opts }),
        Err(msg) => Routed::NeedsMore(msg),
    }
}

const CONVERT_CONNECTORS: &[&str] = &["to", "into", "as", "a", "an", "in"];
const CONVERT_NOISE: &[&str] = &["please", "pls", "my", "the", "out"];

fn route_convert(toks: &[Tok]) -> Routed {
    let Some(j) = toks
        .iter()
        .rposition(|t| !t.quoted && Format::parse(&t.lower).is_some())
    else {
        return Routed::NeedsMore(
            "Convert to what? Try \u{201c}convert photo.heic to jpg\u{201d}.".into(),
        );
    };
    let to = Format::parse(&toks[j].lower).expect("checked above");

    let mut end = j;
    while end > 0
        && !toks[end - 1].quoted
        && CONVERT_CONNECTORS.contains(&toks[end - 1].lower.as_str())
    {
        end -= 1;
    }
    let target: Vec<&Tok> = toks[..end]
        .iter()
        .filter(|t| t.quoted || !CONVERT_NOISE.contains(&t.lower.as_str()))
        .collect();
    match file_ref(&target) {
        Ok(file) => finish(Action::Convert { file, to }),
        Err(msg) => Routed::NeedsMore(msg),
    }
}

const SCAN_NOISE: &[&str] = &[
    "please",
    "pls",
    "my",
    "the",
    "a",
    "an",
    "for",
    "viruses",
    "virus",
    "malware",
    "malicious",
    "infected",
    "safe",
    "is",
    "if",
    "does",
    "have",
    "contain",
    "has",
    "any",
    "and",
    "on",
    "trojans",
    "threats",
    "check",
    "scan",
];

fn route_scan(toks: &[Tok]) -> Routed {
    let target: Vec<&Tok> = toks
        .iter()
        .filter(|t| t.quoted || !SCAN_NOISE.contains(&t.lower.as_str()))
        .collect();
    match file_ref(&target) {
        Ok(file) => finish(Action::Scan { file }),
        Err(msg) => Routed::NeedsMore(msg),
    }
}
