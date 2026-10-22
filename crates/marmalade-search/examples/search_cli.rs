//! Diagnostics for the search backends. Prints counts and timings, not file names, except for
//! files whose path contains the marker given with `--show` (used with a throwaway test folder).
//!
//!     search_cli [--show MARKER] [--raw AQS] [words...]

use marmalade_search::{search, Request, Source};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut show: Option<String> = None;
    let mut raw: Option<String> = None;
    while let Some(i) = args.iter().position(|a| a == "--show" || a == "--raw") {
        let flag = args.remove(i);
        let value = (i < args.len()).then(|| args.remove(i));
        if flag == "--show" {
            show = value;
        } else {
            raw = value;
        }
    }
    let marker = show.map(|s| s.to_lowercase());
    let visible = |p: &std::path::Path| {
        marker
            .as_ref()
            .is_some_and(|m| p.to_string_lossy().to_lowercase().contains(m))
    };

    if let Some(aqs) = raw {
        // Run three times in one process: the first call pays one-off WinRT start-up costs.
        for run in 1..=3 {
            let t = std::time::Instant::now();
            match marmalade_search::index_query_raw(&aqs, None, 100) {
                Ok(hits) => {
                    println!(
                        "raw index query #{run}: {} hits in {:?}",
                        hits.len(),
                        t.elapsed()
                    );
                    if run == 1 {
                        for h in hits.iter().filter(|h| visible(&h.path)) {
                            println!("  {}", h.path.display());
                        }
                    }
                }
                Err(e) => println!("raw index query failed: {e}"),
            }
        }
        return;
    }

    let walker_only = args
        .iter()
        .position(|a| a == "--walker")
        .map(|i| args.remove(i))
        .is_some();
    let req = Request {
        words: marmalade_search::words(&args.join(" ")),
        kind: None,
        since: None,
        limit: 10,
    };
    if walker_only {
        let t = std::time::Instant::now();
        let roots = marmalade_search::default_roots();
        let hits = marmalade_search::walk(&req, &roots, marmalade_search::Limits::default(), 0, 0);
        let existing = roots.iter().filter(|r| r.is_dir()).count();
        println!(
            "walker: {} hits across {existing} existing roots in {:?}",
            hits.len(),
            t.elapsed()
        );
        for h in hits.iter().filter(|h| visible(&h.path)) {
            println!("  {}", h.path.display());
        }
        return;
    }
    let out = search(&req);
    let source = match out.source {
        Source::Indexer => "index",
        Source::Walker => "walker",
    };
    println!(
        "{} hits from the {source} in {:?}",
        out.hits.len(),
        out.elapsed
    );
    for h in out.hits.iter().filter(|h| visible(&h.path)) {
        println!("  {}", h.path.display());
    }
}
