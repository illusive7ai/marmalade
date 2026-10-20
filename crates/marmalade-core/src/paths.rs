//! Validation of user-supplied path text.
//!
//! Anything that ends up in a `FileRef::Path` goes through `check`. It rejects the path forms
//! that are dangerous on Windows: UNC and device paths (opening `\\host\share` leaks the user's
//! NTLM credentials), alternate data streams, reserved device names, traversal, and
//! characters Windows silently rewrites.

use std::fmt;
use std::path::PathBuf;

pub const MAX_PATH_CHARS: usize = 260;
const MAX_COMPONENT_CHARS: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    Empty,
    TooLong,
    ControlChar,
    ForbiddenChar(char),
    /// `\\server\share`, `\\?\C:\...`, `\\.\PhysicalDrive0`, or the `//` forms.
    NetworkOrDevice,
    Traversal,
    Reserved(String),
    /// Component ends in a dot or space (Windows strips these, so it names a different file).
    TrailingDotOrSpace,
    /// `C:file` (drive-relative) is ambiguous.
    DriveRelative,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Empty => write!(f, "the path is empty"),
            PathError::TooLong => write!(f, "the path is too long"),
            PathError::ControlChar => write!(f, "the path contains control characters"),
            PathError::ForbiddenChar(c) => {
                write!(f, "the path contains {c:?}, which is not allowed")
            }
            PathError::NetworkOrDevice => {
                write!(f, "network and device paths are blocked for safety")
            }
            PathError::Traversal => write!(f, "paths with \"..\" are not allowed"),
            PathError::Reserved(n) => write!(f, "{n} is a reserved Windows device name"),
            PathError::TrailingDotOrSpace => write!(f, "names cannot end with a dot or a space"),
            PathError::DriveRelative => write!(f, "use a full path like C:\\folder\\file"),
        }
    }
}

impl std::error::Error for PathError {}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// True when the text looks like a path rather than words to search for.
pub fn looks_like_path(text: &str) -> bool {
    let t = text.trim();
    if t.contains('\\') || t.contains('/') {
        return true;
    }
    let b = t.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    has_extension(t)
}

/// `name.ext` with a 1-5 char alphanumeric extension and a non-empty stem, no spaces after the dot.
pub fn has_extension(text: &str) -> bool {
    match text.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphanumeric())
                && ext.bytes().any(|b| b.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Lowercase extension without the dot, if any.
pub fn extension_of(text: &str) -> Option<String> {
    if has_extension(text) {
        text.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
    } else {
        None
    }
}

/// Validate and normalise (forward slashes become backslashes). Does not touch the filesystem.
pub fn check(text: &str) -> Result<PathBuf, PathError> {
    let t = text
        .trim()
        .trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}')
        .trim();
    if t.is_empty() {
        return Err(PathError::Empty);
    }
    if t.chars().count() > MAX_PATH_CHARS {
        return Err(PathError::TooLong);
    }
    if t.starts_with("\\\\") || t.starts_with("//") {
        return Err(PathError::NetworkOrDevice);
    }

    let norm: String = t.chars().map(|c| if c == '/' { '\\' } else { c }).collect();
    let chars: Vec<char> = norm.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if (c as u32) < 0x20 || c as u32 == 0x7f {
            return Err(PathError::ControlChar);
        }
        match c {
            '<' | '>' | '"' | '|' | '?' | '*' => return Err(PathError::ForbiddenChar(c)),
            ':' => {
                let drive_colon = i == 1 && chars[0].is_ascii_alphabetic();
                if !drive_colon {
                    return Err(PathError::ForbiddenChar(':')); // also blocks alternate data streams
                }
                if chars.len() > 2 && chars[2] != '\\' {
                    return Err(PathError::DriveRelative);
                }
            }
            _ => {}
        }
    }

    let rest = if chars.len() >= 2 && chars[1] == ':' {
        &norm[2..]
    } else {
        &norm[..]
    };
    for comp in rest.split('\\').filter(|c| !c.is_empty()) {
        if comp == "." {
            continue;
        }
        if comp == ".." {
            return Err(PathError::Traversal);
        }
        if comp.chars().count() > MAX_COMPONENT_CHARS {
            return Err(PathError::TooLong);
        }
        if comp.ends_with('.') || comp.ends_with(' ') {
            return Err(PathError::TrailingDotOrSpace);
        }
        let stem = comp
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if RESERVED.contains(&stem.as_str()) {
            return Err(PathError::Reserved(stem));
        }
    }
    Ok(PathBuf::from(norm))
}
