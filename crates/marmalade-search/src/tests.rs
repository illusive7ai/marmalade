use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use marmalade_core::{FileKind, FindSpec, Since};

use crate::timeutil::{civil_from_days, date_range, iso_local_date};
use crate::{build_aqs, rank, sanitize_word, walk, words, Hit, Limits, Request};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("marmalade-search-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        File::create(&p).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn req(words: &[&str]) -> Request {
    Request {
        words: words.iter().map(|s| s.to_string()).collect(),
        kind: None,
        since: None,
        limit: 10,
    }
}

fn names(hits: &[Hit]) -> Vec<String> {
    hits.iter()
        .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Inverse of `civil_from_days`, to build test timestamps without hardcoding magic numbers.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn utc(y: i64, m: i64, d: i64, h: i64, min: i64) -> i64 {
    days_from_civil(y, m, d) * 86_400 + h * 3_600 + min * 60
}

// ---- query building ----

#[test]
fn words_are_sanitised_so_text_cannot_inject_query_syntax() {
    let w = words(r#"cv" OR System.Kind:folder (x) *~<> \ /"#);
    assert!(!w.is_empty());
    for word in &w {
        assert!(
            !word.contains(['"', ':', '(', ')', '*', '~', '<', '>', '\\', '/']),
            "{word:?}"
        );
    }
    assert_eq!(sanitize_word("inv*oice?"), "invoice");
    assert_eq!(sanitize_word("r\u{e9}sum\u{e9}"), "r\u{e9}sum\u{e9}"); // letters in any script survive
    assert_eq!(sanitize_word("***"), "");
    assert_eq!(words("  a   b  "), vec!["a", "b"]);
}

#[test]
fn word_count_and_length_are_capped() {
    assert_eq!(words(&"w ".repeat(50)).len(), 8);
    assert_eq!(sanitize_word(&"x".repeat(500)).chars().count(), 64);
}

#[test]
fn aqs_has_one_quoted_clause_per_word_and_a_date_filter() {
    let mut r = req(&["invoice", "march"]);
    let now = utc(2026, 10, 20, 15, 0);
    assert_eq!(
        build_aqs(&r, now, 0),
        "System.ItemNameDisplay:~=\"invoice\" System.ItemNameDisplay:~=\"march\""
    );
    r.since = Some(Since::Yesterday);
    let q = build_aqs(&r, now, 0);
    assert!(
        q.ends_with("System.DateModified:>=2026-10-19 System.DateModified:<2026-10-20"),
        "{q}"
    );
    r.since = Some(Since::Week);
    assert!(build_aqs(&r, now, 0).ends_with("System.DateModified:>=2026-10-14"));
    assert_eq!(build_aqs(&req(&[]), now, 0), "");
}

#[test]
fn request_from_spec_sanitises_the_query() {
    let spec = FindSpec {
        query: "my \"cv\" *".into(),
        kind: Some(FileKind::Pdf),
        since: None,
    };
    let r = Request::from_spec(&spec, 6);
    assert_eq!(r.words, vec!["my", "cv"]);
    assert_eq!(r.kind, Some(FileKind::Pdf));
    assert_eq!(r.limit, 6);
}

// ---- dates ----

#[test]
fn civil_from_days_round_trips() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
    assert_eq!(
        civil_from_days(days_from_civil(2026, 10, 20)),
        (2026, 10, 20)
    );
    assert_eq!(
        civil_from_days(days_from_civil(1969, 12, 31)),
        (1969, 12, 31)
    );
    for day in (-40_000..60_000).step_by(37) {
        let (y, m, d) = civil_from_days(day);
        assert_eq!(days_from_civil(y, m as i64, d as i64), day);
    }
}

#[test]
fn date_ranges_anchor_to_local_midnight() {
    // 15:00 UTC on 20 Oct, local time is UTC+3 (18:00), so local midnight was 21:00 UTC on the 19th.
    let now = utc(2026, 10, 20, 15, 0);
    let tz = 3 * 3_600;
    let midnight = utc(2026, 10, 19, 21, 0);
    assert_eq!(date_range(Since::Today, now, tz).from, midnight);
    let y = date_range(Since::Yesterday, now, tz);
    assert_eq!((y.from, y.to), (midnight - 86_400, Some(midnight)));
    assert_eq!(date_range(Since::Week, now, tz).from, midnight - 6 * 86_400);
    assert_eq!(
        date_range(Since::Month, now, tz).from,
        midnight - 29 * 86_400
    );

    // Just after local midnight the day has already rolled over, even though UTC has not.
    let now = utc(2026, 10, 19, 21, 30);
    assert_eq!(
        date_range(Since::Today, now, tz).from,
        utc(2026, 10, 19, 21, 0)
    );

    // Negative offsets (the Americas).
    let now = utc(2026, 10, 20, 2, 0); // 21:00 on the 19th at UTC-5
    assert_eq!(
        date_range(Since::Today, now, -5 * 3_600).from,
        utc(2026, 10, 19, 5, 0)
    );
}

#[test]
fn iso_dates_use_the_local_day() {
    let t = utc(2026, 10, 20, 23, 30);
    assert_eq!(iso_local_date(t, 0), "2026-10-20");
    assert_eq!(iso_local_date(t, 3 * 3_600), "2026-10-21");
    assert_eq!(iso_local_date(t, -5 * 3_600), "2026-10-20");
    let r = date_range(Since::Today, t, 0);
    assert!(r.contains(t));
    assert!(!r.contains(r.from - 1));
}

// ---- ranking ----

#[test]
fn rank_prefers_exact_then_prefix_then_contains_then_newest() {
    let hit = |p: &str, m: i64| Hit {
        path: p.into(),
        modified: Some(m),
    };
    let hits = vec![
        hit(r"C:\x\old-cv-draft.docx", 100),
        hit(r"C:\x\cv.pdf", 1),
        hit(r"C:\x\cv notes.txt", 50),
        hit(r"C:\x\cv backup.docx", 90),
        hit(r"C:\x\unrelated content match.txt", 999),
    ];
    let ranked = rank(hits, &["cv".to_string()], 10);
    assert_eq!(
        names(&ranked),
        // Both "cv ..." names start with the word, so the newer one (90 vs 50) comes first.
        vec![
            "cv.pdf",
            "cv backup.docx",
            "cv notes.txt",
            "old-cv-draft.docx",
            "unrelated content match.txt"
        ]
    );
}

#[test]
fn rank_dedupes_case_insensitively_and_truncates() {
    let hit = |p: &str| Hit {
        path: p.into(),
        modified: None,
    };
    let hits = vec![
        hit(r"C:\A\Cv.pdf"),
        hit(r"c:\a\cv.PDF"),
        hit(r"C:\a\cv2.pdf"),
        hit(r"C:\a\cv3.pdf"),
    ];
    let ranked = rank(hits, &["cv".to_string()], 2);
    assert_eq!(ranked.len(), 2);
}

// ---- walker ----

fn walk_in(dir: &TempDir, r: &Request) -> Vec<String> {
    let mut n = names(&walk(
        r,
        &[dir.path().to_path_buf()],
        Limits::default(),
        0,
        0,
    ));
    n.sort();
    n
}

#[test]
fn walker_matches_all_words_case_insensitively() {
    let t = TempDir::new();
    t.file("Invoice March.pdf");
    t.file("invoice-notes.txt");
    t.file("photo.png");
    t.file("sub/deeper/MARCH invoice scan.png");
    assert_eq!(
        walk_in(&t, &req(&["invoice"])),
        vec![
            "Invoice March.pdf",
            "MARCH invoice scan.png",
            "invoice-notes.txt"
        ]
    );
    assert_eq!(
        walk_in(&t, &req(&["invoice", "march"])),
        vec!["Invoice March.pdf", "MARCH invoice scan.png"]
    );
    assert!(walk_in(&t, &req(&["nonexistent"])).is_empty());
}

#[test]
fn walker_filters_by_kind_and_needs_no_words() {
    let t = TempDir::new();
    t.file("a.pdf");
    t.file("b.PDF");
    t.file("c.txt");
    t.file("noext");
    let mut r = req(&[]);
    r.kind = Some(FileKind::Pdf);
    assert_eq!(walk_in(&t, &r), vec!["a.pdf", "b.PDF"]);
}

#[test]
fn walker_filters_by_modified_time() {
    let t = TempDir::new();
    let fresh = t.file("fresh.txt");
    let stale = t.file("stale.txt");
    let now = SystemTime::now();
    File::options()
        .write(true)
        .open(&fresh)
        .unwrap()
        .set_modified(now)
        .unwrap();
    File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(now - Duration::from_secs(10 * 86_400))
        .unwrap();

    let now_unix = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let mut r = req(&[]);
    r.since = Some(Since::Week);
    let found = names(&walk(
        &r,
        &[t.path().to_path_buf()],
        Limits::default(),
        now_unix,
        0,
    ));
    assert_eq!(found, vec!["fresh.txt"]);
}

#[test]
fn walker_respects_depth_skip_dirs_and_entry_cap() {
    let t = TempDir::new();
    t.file("top.txt");
    t.file("a/b/c/d/e/too-deep.txt");
    t.file("node_modules/pkg/skipped.txt");
    t.file(".hidden-dir/skipped2.txt");
    let found = walk_in(&t, &req(&[]));
    assert_eq!(found, vec!["top.txt"]);

    let shallow = Limits {
        max_depth: 1,
        ..Limits::default()
    };
    t.file("a/one.txt");
    let found = names(&walk(&req(&[]), &[t.path().to_path_buf()], shallow, 0, 0));
    assert_eq!(found, vec!["top.txt"]);

    for i in 0..50 {
        t.file(&format!("many/f{i}.txt"));
    }
    let capped = Limits {
        max_entries: 10,
        ..Limits::default()
    };
    let found = walk(&req(&[]), &[t.path().to_path_buf()], capped, 0, 0);
    assert!(found.len() <= 10);
}

#[test]
fn walker_ignores_missing_roots() {
    let missing = PathBuf::from(r"C:\definitely\not\here\marmalade");
    assert!(walk(&req(&["x"]), &[missing], Limits::default(), 0, 0).is_empty());
}
