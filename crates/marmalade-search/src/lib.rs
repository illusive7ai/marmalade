//! File search for Marmalade.
//!
//! Strategy: ask the Windows Search index first (fast, no admin, covers the whole profile); if it
//! is unavailable or finds nothing, fall back to a small bounded folder walk. Results are ranked
//! the same way whichever backend produced them.

mod aqs;
#[cfg(windows)]
mod indexer;
mod timeutil;
mod walker;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use marmalade_core::{FileKind, FindSpec, Since};

pub use aqs::{build_aqs, sanitize_word, words};
pub use timeutil::{civil_from_days, date_range, iso_local_date, DateRange};
pub use walker::{default_roots, walk, Limits};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Sanitised words; every one must match.
    pub words: Vec<String>,
    pub kind: Option<FileKind>,
    pub since: Option<Since>,
    pub limit: usize,
}

impl Request {
    pub fn from_spec(spec: &FindSpec, limit: usize) -> Request {
        Request {
            words: words(&spec.query),
            kind: spec.kind,
            since: spec.since,
            limit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// Unix seconds, when the backend knows it.
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Indexer,
    Walker,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    pub source: Source,
    pub elapsed: Duration,
}

/// How well a file name matches: lower is better.
fn name_rank(path: &Path, words: &[String]) -> u8 {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = name
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| name.clone());
    let joined = words.join(" ").to_lowercase();
    if words.is_empty() || stem == joined || name == joined {
        0
    } else if words
        .first()
        .is_some_and(|w| name.starts_with(&w.to_lowercase()))
    {
        1
    } else if words.iter().all(|w| name.contains(&w.to_lowercase())) {
        2
    } else {
        3 // matched on something other than the name
    }
}

/// Order best-first (exact name, starts with, contains, anything else; newest first within a
/// rank), drop duplicates, and keep `limit`.
pub fn rank(mut hits: Vec<Hit>, words: &[String], limit: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| {
        name_rank(&a.path, words)
            .cmp(&name_rank(&b.path, words))
            .then_with(|| b.modified.cmp(&a.modified))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.path.to_string_lossy().to_lowercase()));
    hits.truncate(limit);
    hits
}

/// Run one raw query against the index under the user profile. For diagnostics and experiments.
#[cfg(windows)]
#[doc(hidden)]
pub fn index_query_raw(aqs: &str, kind: Option<FileKind>, fetch: u32) -> Result<Vec<Hit>, String> {
    let profile = std::env::var("USERPROFILE").map_err(|e| e.to_string())?;
    let req = Request {
        words: Vec::new(),
        kind,
        since: None,
        limit: fetch as usize,
    };
    indexer::query(&profile, aqs, &req, fetch).map_err(|e| e.to_string())
}

/// Run a search. Blocks, so call it from a worker thread.
pub fn search(req: &Request) -> Outcome {
    let start = Instant::now();
    let now = timeutil::now_unix();
    let tz = timeutil::local_tz_offset();
    let fetch = (req.limit * 8).clamp(20, 200);

    let mut source = Source::Walker;
    let mut hits: Vec<Hit> = Vec::new();

    #[cfg(windows)]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let aqs = build_aqs(req, now, tz);
        if let Ok(found) = indexer::query(&profile, &aqs, req, fetch as u32) {
            if !found.is_empty() {
                source = Source::Indexer;
                hits = found;
            }
        }
    }

    if hits.is_empty() {
        hits = walk(req, &default_roots(), Limits::default(), now, tz);
        source = Source::Walker;
    }

    Outcome {
        hits: rank(hits, &req.words, req.limit),
        source,
        elapsed: start.elapsed(),
    }
}

#[cfg(test)]
mod tests;
