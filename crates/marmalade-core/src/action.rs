//! The fixed set of things Marmalade can be asked to do.
//!
//! This enum is the only interface between "text from the user (or later, an LLM)" and
//! "code that touches files, printers or processes". Nothing here is a shell command or free-form
//! string passed to the OS. Whatever produces an `Action` must call `validate()` before it is run.

use std::fmt;
use std::path::PathBuf;

use crate::paths;

pub const MAX_QUERY_CHARS: usize = 120;
pub const MAX_COPIES: u8 = 99;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRef {
    /// A validated path (see `paths::check`). May be relative; the executor resolves it.
    Path(PathBuf),
    /// Words to resolve through file search.
    Query(String),
    /// "this" / "it": the file the user dropped on the cat, or the last search result.
    Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Video,
    Audio,
    Archive,
}

impl FileKind {
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Document => "Word",
            FileKind::Spreadsheet => "Excel",
            FileKind::Presentation => "PowerPoint",
            FileKind::Pdf => "PDF",
            FileKind::Image => "image",
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Archive => "archive",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileKind::Document => &["doc", "docx", "odt", "rtf", "txt"],
            FileKind::Spreadsheet => &["xls", "xlsx", "ods", "csv"],
            FileKind::Presentation => &["ppt", "pptx", "odp"],
            FileKind::Pdf => &["pdf"],
            FileKind::Image => &["png", "jpg", "jpeg", "gif", "webp", "bmp", "heic", "tiff"],
            FileKind::Video => &["mp4", "mov", "mkv", "avi", "webm"],
            FileKind::Audio => &["mp3", "wav", "m4a", "flac", "ogg", "aac"],
            FileKind::Archive => &["zip", "7z", "rar", "tar", "gz"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Since {
    Today,
    Yesterday,
    Week,
    Month,
}

impl Since {
    pub fn label(self) -> &'static str {
        match self {
            Since::Today => "today",
            Since::Yesterday => "yesterday",
            Since::Week => "the last week",
            Since::Month => "the last month",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindSpec {
    pub query: String,
    pub kind: Option<FileKind>,
    pub since: Option<Since>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Png,
    Jpg,
    Webp,
    Gif,
    Mp3,
    Mp4,
}

impl Format {
    pub fn parse(word: &str) -> Option<Format> {
        match word.trim_start_matches('.') {
            "pdf" => Some(Format::Pdf),
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpg),
            "webp" => Some(Format::Webp),
            "gif" => Some(Format::Gif),
            "mp3" => Some(Format::Mp3),
            "mp4" => Some(Format::Mp4),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Gif => "gif",
            Format::Mp3 => "mp3",
            Format::Mp4 => "mp4",
        }
    }
}

/// Allow-list of conversions. Anything not listed here is refused, not attempted.
pub fn can_convert(from_ext: &str, to: Format) -> bool {
    let from = from_ext.to_ascii_lowercase();
    if from == to.ext() || (from == "jpeg" && to == Format::Jpg) {
        return false;
    }
    let is = |kind: FileKind| kind.extensions().contains(&from.as_str());
    match to {
        Format::Png | Format::Jpg | Format::Webp => is(FileKind::Image),
        Format::Pdf => {
            is(FileKind::Image)
                || is(FileKind::Document)
                || is(FileKind::Spreadsheet)
                || is(FileKind::Presentation)
        }
        Format::Gif | Format::Mp4 => is(FileKind::Video),
        Format::Mp3 => is(FileKind::Video) || is(FileKind::Audio),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintOptions {
    pub copies: u8,
    pub duplex: bool,
    pub grayscale: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            copies: 1,
            duplex: false,
            grayscale: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Find(FindSpec),
    Print { file: FileRef, opts: PrintOptions },
    Convert { file: FileRef, to: Format },
    Scan { file: FileRef },
    ListPrinters,
    Help,
}

/// What an action can do to the user's world. Drives the confirmation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Reads or lists only.
    ReadOnly,
    /// Creates a new file, never overwrites or deletes.
    CreatesFile,
    /// Has a real-world effect that cannot be undone (paper, ink).
    Outward,
}

impl Action {
    pub fn risk(&self) -> Risk {
        match self {
            Action::Find(_) | Action::Scan { .. } | Action::ListPrinters | Action::Help => {
                Risk::ReadOnly
            }
            Action::Convert { .. } => Risk::CreatesFile,
            Action::Print { .. } => Risk::Outward,
        }
    }

    pub fn requires_confirmation(&self) -> bool {
        self.risk() == Risk::Outward
    }

    /// Re-check every argument. The router calls this on what it builds, and anything else that
    /// constructs an `Action` (a future LLM adapter, a plugin, the MCP server) must too.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Find(spec) => check_query(&spec.query, true),
            Action::Print { file, opts } => {
                if !(1..=MAX_COPIES).contains(&opts.copies) {
                    return Err(format!("copies must be 1 to {MAX_COPIES}"));
                }
                check_file(file)
            }
            Action::Convert { file, to } => {
                check_file(file)?;
                if let FileRef::Path(p) = file {
                    let name = p.to_string_lossy();
                    match paths::extension_of(&name) {
                        Some(ext) if can_convert(&ext, *to) => {}
                        Some(ext) => return Err(format!("cannot convert .{ext} to {}", to.ext())),
                        None => return Err("the file needs a known extension to convert".into()),
                    }
                }
                Ok(())
            }
            Action::Scan { file } => check_file(file),
            Action::ListPrinters | Action::Help => Ok(()),
        }
    }
}

fn check_query(q: &str, allow_empty: bool) -> Result<(), String> {
    if q.is_empty() && !allow_empty {
        return Err("empty search".into());
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err("search text is too long".into());
    }
    if q.chars().any(|c| c.is_control()) {
        return Err("search text contains control characters".into());
    }
    Ok(())
}

fn check_file(f: &FileRef) -> Result<(), String> {
    match f {
        FileRef::Context => Ok(()),
        FileRef::Query(q) => check_query(q, false),
        FileRef::Path(p) => paths::check(&p.to_string_lossy())
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

impl fmt::Display for FileRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileRef::Path(p) => write!(f, "{}", p.display()),
            FileRef::Query(q) => write!(f, "\u{201c}{q}\u{201d}"),
            FileRef::Context => write!(f, "this file"),
        }
    }
}

/// Human-readable one-line summary, shown as the live preview in the chat bar.
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Find(s) => {
                write!(f, "Find ")?;
                if let Some(k) = s.kind {
                    write!(f, "{} ", k.label())?;
                }
                write!(f, "files")?;
                if !s.query.is_empty() {
                    write!(f, " matching \u{201c}{}\u{201d}", s.query)?;
                }
                if let Some(since) = s.since {
                    write!(f, " from {}", since.label())?;
                }
                Ok(())
            }
            Action::Print { file, opts } => {
                write!(f, "Print {file}")?;
                if opts.copies > 1 {
                    write!(f, ", {} copies", opts.copies)?;
                }
                if opts.duplex {
                    write!(f, ", double-sided")?;
                }
                if opts.grayscale {
                    write!(f, ", black and white")?;
                }
                Ok(())
            }
            Action::Convert { file, to } => {
                write!(f, "Convert {file} to {}", to.ext().to_uppercase())
            }
            Action::Scan { file } => write!(f, "Scan {file} for malware"),
            Action::ListPrinters => write!(f, "List printers"),
            Action::Help => write!(f, "Show what I can do"),
        }
    }
}
