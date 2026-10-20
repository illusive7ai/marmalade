use std::path::PathBuf;

use crate::action::{can_convert, Risk, MAX_COPIES};
use crate::paths::{self, PathError};
use crate::{route, Action, FileKind, FileRef, FindSpec, Format, PrintOptions, Routed, Since};

fn intent(input: &str) -> Action {
    match route(input) {
        Routed::Intent(a) => a,
        other => panic!("{input:?} -> {other:?}"),
    }
}

fn needs_more(input: &str) -> String {
    match route(input) {
        Routed::NeedsMore(m) => m,
        other => panic!("{input:?} -> {other:?}"),
    }
}

fn find(query: &str, kind: Option<FileKind>, since: Option<Since>) -> Action {
    Action::Find(FindSpec {
        query: query.into(),
        kind,
        since,
    })
}

// ---- find ----

#[test]
fn find_basic_queries() {
    assert_eq!(intent("find my cv"), find("cv", None, None));
    assert_eq!(intent("Where is the invoice?"), find("invoice", None, None));
    assert_eq!(
        intent("can you please find budget 2024"),
        find("budget 2024", None, None)
    );
    assert_eq!(intent("where is document?"), find("document", None, None));
    assert_eq!(
        intent("find \"my cv.docx\""),
        find("my cv.docx", None, None)
    );
}

#[test]
fn find_with_kind_and_time() {
    assert_eq!(
        intent("find pdfs from last week"),
        find("", Some(FileKind::Pdf), Some(Since::Week))
    );
    assert_eq!(
        intent("find excel files from yesterday"),
        find("", Some(FileKind::Spreadsheet), Some(Since::Yesterday))
    );
    assert_eq!(
        intent("search photos today"),
        find("", Some(FileKind::Image), Some(Since::Today))
    );
    assert_eq!(
        intent("find the contract pdf from last month"),
        find("contract", Some(FileKind::Pdf), Some(Since::Month))
    );
    assert_eq!(
        intent("find documents from today"),
        find("", Some(FileKind::Document), Some(Since::Today))
    );
}

#[test]
fn find_needs_a_target() {
    assert!(needs_more("find").starts_with("Find what?"));
    assert!(needs_more("where is the").starts_with("Find what?"));
}

// ---- print ----

#[test]
fn print_basic_and_options() {
    assert_eq!(
        intent("print invoice.pdf"),
        Action::Print {
            file: FileRef::Path("invoice.pdf".into()),
            opts: PrintOptions::default()
        }
    );
    assert_eq!(
        intent("print 2 copies of report.docx double sided"),
        Action::Print {
            file: FileRef::Path("report.docx".into()),
            opts: PrintOptions {
                copies: 2,
                duplex: true,
                grayscale: false
            },
        }
    );
    assert_eq!(
        intent("print this in black and white"),
        Action::Print {
            file: FileRef::Context,
            opts: PrintOptions {
                copies: 1,
                duplex: false,
                grayscale: true
            }
        }
    );
    assert_eq!(
        intent("hey marmalade, could you please print twice a.pdf"),
        Action::Print {
            file: FileRef::Path("a.pdf".into()),
            opts: PrintOptions {
                copies: 2,
                ..PrintOptions::default()
            },
        }
    );
    assert_eq!(
        intent("print my cover letter"),
        Action::Print {
            file: FileRef::Query("cover letter".into()),
            opts: PrintOptions::default()
        }
    );
}

#[test]
fn print_normalises_windows_paths() {
    match intent(r"print C:\Users\Me\Documents\a b.pdf") {
        Action::Print {
            file: FileRef::Path(p),
            ..
        } => {
            assert_eq!(p, PathBuf::from(r"C:\Users\Me\Documents\a b.pdf"))
        }
        other => panic!("{other:?}"),
    }
    match intent("print C:/Users/Me/a.pdf") {
        Action::Print {
            file: FileRef::Path(p),
            ..
        } => assert_eq!(p, PathBuf::from(r"C:\Users\Me\a.pdf")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn print_is_the_action_that_needs_confirmation() {
    assert!(intent("print a.pdf").requires_confirmation());
    assert_eq!(intent("print a.pdf").risk(), Risk::Outward);
    assert!(!intent("find cv").requires_confirmation());
    assert!(!intent("convert a.png to jpg").requires_confirmation());
}

#[test]
fn print_rejects_absurd_copies_and_hostile_paths() {
    assert!(needs_more("print 500 copies of a.pdf").contains("1 to 99"));
    assert!(needs_more("print 0 copies of a.pdf").contains("1 to 99"));
    needs_more(r"print \\evil\share\x.pdf");
    needs_more(r"print C:\a\..\..\Windows\win.ini");
    needs_more("print NUL.txt");
}

// ---- convert ----

#[test]
fn convert_patterns() {
    assert_eq!(
        intent("convert photo.heic to jpg"),
        Action::Convert {
            file: FileRef::Path("photo.heic".into()),
            to: Format::Jpg
        }
    );
    assert_eq!(
        intent("make this a pdf"),
        Action::Convert {
            file: FileRef::Context,
            to: Format::Pdf
        }
    );
    assert_eq!(
        intent("turn report.docx into pdf"),
        Action::Convert {
            file: FileRef::Path("report.docx".into()),
            to: Format::Pdf
        }
    );
    assert_eq!(
        intent("convert my holiday video to gif"),
        Action::Convert {
            file: FileRef::Query("holiday video".into()),
            to: Format::Gif
        }
    );
    assert_eq!(
        intent("save clip.mov as mp3"),
        Action::Convert {
            file: FileRef::Path("clip.mov".into()),
            to: Format::Mp3
        }
    );
}

#[test]
fn convert_refuses_what_is_not_on_the_allow_list() {
    assert!(needs_more("convert song.mp3 to mp3").contains("cannot convert"));
    assert!(needs_more("convert photo.png to mp4").contains("cannot convert"));
    assert!(needs_more("convert setup.exe to pdf").contains("cannot convert"));
    assert!(needs_more("convert photo").starts_with("Convert to what?"));
    assert!(!can_convert("jpeg", Format::Jpg));
    assert!(can_convert("HEIC", Format::Jpg));
    assert!(can_convert("docx", Format::Pdf));
    assert!(!can_convert("pdf", Format::Png)); // PDF input is not supported yet
}

// ---- scan ----

#[test]
fn scan_patterns() {
    assert_eq!(
        intent("scan setup.exe"),
        Action::Scan {
            file: FileRef::Path("setup.exe".into())
        }
    );
    assert_eq!(
        intent("is this file safe?"),
        Action::Scan {
            file: FileRef::Context
        }
    );
    assert_eq!(
        intent("is it safe"),
        Action::Scan {
            file: FileRef::Context
        }
    );
    assert_eq!(
        intent("check downloads for viruses"),
        Action::Scan {
            file: FileRef::Query("downloads".into())
        }
    );
    needs_more(r"scan \\server\share\a.exe");
}

// ---- misc ----

#[test]
fn help_and_printers() {
    assert_eq!(intent("help"), Action::Help);
    assert_eq!(intent("what can you do?"), Action::Help);
    assert_eq!(intent("list printers"), Action::ListPrinters);
    assert_eq!(intent("show my printers"), Action::ListPrinters);
}

#[test]
fn unknown_and_limits() {
    assert_eq!(route(""), Routed::Unknown);
    assert_eq!(route("   "), Routed::Unknown);
    assert_eq!(route("tell me a joke"), Routed::Unknown);
    assert_eq!(route("make me a sandwich"), Routed::Unknown);
    assert!(needs_more(&"a".repeat(301)).contains("too long"));
}

#[test]
fn summaries_read_naturally() {
    assert_eq!(
        intent("find pdfs from last week").to_string(),
        "Find PDF files from the last week"
    );
    assert_eq!(
        intent("find my cv").to_string(),
        "Find files matching \u{201c}cv\u{201d}"
    );
    assert_eq!(
        intent("print 2 copies of a.pdf duplex").to_string(),
        "Print a.pdf, 2 copies, double-sided"
    );
    assert_eq!(
        intent("convert a.png to jpg").to_string(),
        "Convert a.png to JPG"
    );
}

// ---- paths ----

#[test]
fn paths_accept_normal_windows_paths() {
    assert_eq!(
        paths::check(r"C:\Users\me\file.txt").unwrap(),
        PathBuf::from(r"C:\Users\me\file.txt")
    );
    assert_eq!(
        paths::check("docs/report.pdf").unwrap(),
        PathBuf::from(r"docs\report.pdf")
    );
    assert_eq!(paths::check("file.txt").unwrap(), PathBuf::from("file.txt"));
    assert_eq!(
        paths::check("\"C:\\a b\\c.txt\"").unwrap(),
        PathBuf::from(r"C:\a b\c.txt")
    );
    assert_eq!(
        paths::check(r"C:\a\.\b.txt").unwrap(),
        PathBuf::from(r"C:\a\.\b.txt")
    );
}

#[test]
fn paths_reject_dangerous_forms() {
    use PathError::*;
    assert_eq!(paths::check(""), Err(Empty));
    assert_eq!(paths::check(r"\\server\share\f"), Err(NetworkOrDevice));
    assert_eq!(paths::check("//server/x"), Err(NetworkOrDevice));
    assert_eq!(paths::check(r"\\?\C:\x"), Err(NetworkOrDevice));
    assert_eq!(paths::check(r"\\.\PhysicalDrive0"), Err(NetworkOrDevice));
    assert_eq!(paths::check(r"C:\a\..\b"), Err(Traversal));
    assert_eq!(paths::check(r"C:\a\NUL"), Err(Reserved("NUL".into())));
    assert_eq!(paths::check("con.txt"), Err(Reserved("CON".into())));
    assert_eq!(
        paths::check(r"C:\x\file.txt:hidden"),
        Err(ForbiddenChar(':'))
    );
    assert_eq!(paths::check("C:foo"), Err(DriveRelative));
    assert_eq!(paths::check("a<b"), Err(ForbiddenChar('<')));
    assert_eq!(paths::check("a|b"), Err(ForbiddenChar('|')));
    assert_eq!(paths::check("a*b"), Err(ForbiddenChar('*')));
    assert_eq!(paths::check("a\u{0}b"), Err(ControlChar));
    assert_eq!(paths::check(r"C:\a.\b"), Err(TrailingDotOrSpace));
    assert_eq!(
        paths::check(&format!("C:\\{}", "a".repeat(300))),
        Err(TooLong)
    );
}

#[test]
fn path_heuristics() {
    assert!(paths::looks_like_path("a.pdf"));
    assert!(paths::looks_like_path(r"C:\x"));
    assert!(paths::looks_like_path("x/y"));
    assert!(!paths::looks_like_path("my cv"));
    assert!(!paths::looks_like_path("v1.5.")); // no usable extension
    assert_eq!(paths::extension_of("A.JPG").as_deref(), Some("jpg"));
    assert_eq!(paths::extension_of("noext"), None);
}

// ---- validation and fuzzing ----

#[test]
fn validate_catches_hand_built_actions() {
    let bad_copies = Action::Print {
        file: FileRef::Context,
        opts: PrintOptions {
            copies: 0,
            ..Default::default()
        },
    };
    assert!(bad_copies.validate().is_err());
    let too_many = Action::Print {
        file: FileRef::Context,
        opts: PrintOptions {
            copies: MAX_COPIES + 1,
            ..Default::default()
        },
    };
    assert!(too_many.validate().is_err());
    let unc = Action::Scan {
        file: FileRef::Path(r"\\evil\share\a.exe".into()),
    };
    assert!(unc.validate().is_err());
    let long = Action::Find(FindSpec {
        query: "x".repeat(500),
        kind: None,
        since: None,
    });
    assert!(long.validate().is_err());
    let ctrl = Action::Find(FindSpec {
        query: "a\nb".into(),
        kind: None,
        since: None,
    });
    assert!(ctrl.validate().is_err());
}

#[test]
fn router_never_panics_and_only_emits_valid_actions() {
    const WORDS: &[&str] = &[
        "find",
        "print",
        "convert",
        "scan",
        "to",
        "pdf",
        "jpg",
        "this",
        "it",
        "copies",
        "2",
        "0",
        "999",
        "a.pdf",
        r"C:\x\y.png",
        r"\\h\s\f",
        "..",
        "\"",
        "\u{201c}",
        "con",
        "nul",
        "last",
        "week",
        "safe",
        "is",
        "?",
        ":",
        "twice",
        "black",
        "and",
        "white",
        "double",
        "sided",
        "x",
        "\u{0}",
        "\u{1F408}",
        "e\u{301}",
        "",
    ];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        let n = (next() % 8) as usize;
        let text = (0..n)
            .map(|_| WORDS[(next() % WORDS.len() as u64) as usize])
            .collect::<Vec<_>>()
            .join(" ");
        if let Routed::Intent(a) = route(&text) {
            assert!(
                a.validate().is_ok(),
                "{text:?} produced an invalid action {a:?}"
            );
        }
    }
}
