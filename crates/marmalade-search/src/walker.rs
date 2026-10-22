//! Fallback search: a bounded breadth-first walk of a few well-known folders.
//!
//! Used when the Windows Search index is unavailable or finds nothing. Everything is capped:
//! depth, entries scanned, and wall-clock time, so a huge folder can never freeze the app.
//! Hidden and system files are skipped, and symlinks and junctions are never followed.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::timeutil::{date_range, DateRange};
use crate::{Hit, Request};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
    pub budget: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: 5,
            max_entries: 60_000,
            budget: Duration::from_millis(600),
        }
    }
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "appdata",
    "$recycle.bin",
    "system volume information",
    "target",
];

/// The folders people keep files in. Missing ones are skipped by the walk.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut add = |base: Option<String>| {
        if let Some(base) = base.filter(|b| !b.is_empty()) {
            for sub in [
                "Desktop",
                "Documents",
                "Downloads",
                "Pictures",
                "Videos",
                "Music",
            ] {
                let p = Path::new(&base).join(sub);
                if !roots.contains(&p) {
                    roots.push(p);
                }
            }
        }
    };
    add(std::env::var("USERPROFILE").ok());
    add(std::env::var("OneDrive").ok());
    add(std::env::var("OneDriveConsumer").ok());
    roots
}

#[cfg(windows)]
fn is_hidden_or_system(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    meta.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden_or_system(_meta: &fs::Metadata) -> bool {
    false
}

fn extension_matches(name: &str, req: &Request) -> bool {
    let Some(kind) = req.kind else { return true };
    match name.rsplit_once('.') {
        Some((_, ext)) => kind
            .extensions()
            .contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

pub fn walk(
    req: &Request,
    roots: &[PathBuf],
    limits: Limits,
    now_utc: i64,
    tz_offset: i64,
) -> Vec<Hit> {
    let start = Instant::now();
    let words: Vec<String> = req.words.iter().map(|w| w.to_lowercase()).collect();
    let range: Option<DateRange> = req.since.map(|s| date_range(s, now_utc, tz_offset));

    let mut hits = Vec::new();
    let mut scanned = 0usize;
    let mut queue: VecDeque<(PathBuf, usize)> = roots.iter().cloned().map(|r| (r, 0)).collect();

    'outer: while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned += 1;
            if scanned > limits.max_entries
                || (scanned.is_multiple_of(256) && start.elapsed() > limits.budget)
            {
                break 'outer;
            }
            let (Ok(ft), Ok(meta)) = (entry.file_type(), entry.metadata()) else {
                continue;
            };
            if ft.is_symlink() || is_hidden_or_system(&meta) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                let lower = name.to_lowercase();
                if depth + 1 < limits.max_depth
                    && !lower.starts_with('.')
                    && !SKIP_DIRS.contains(&lower.as_str())
                {
                    queue.push_back((entry.path(), depth + 1));
                }
                continue;
            }
            if !extension_matches(&name, req) {
                continue;
            }
            let lower = name.to_lowercase();
            if !words.iter().all(|w| lower.contains(w.as_str())) {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            if let Some(range) = range {
                match modified {
                    Some(t) if range.contains(t) => {}
                    _ => continue,
                }
            }
            hits.push(Hit {
                path: entry.path(),
                modified,
            });
        }
    }
    hits
}
