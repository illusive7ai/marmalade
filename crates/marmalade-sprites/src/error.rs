use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// anim.toml is malformed or violates a limit.
    Meta(String),
    /// A frame image is malformed, unsupported, or over a limit.
    Image(String),
    /// The animation id is not a safe name (path traversal guard).
    BadId(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Meta(m) => write!(f, "bad anim.toml: {m}"),
            Error::Image(m) => write!(f, "bad frame image: {m}"),
            Error::BadId(id) => write!(f, "invalid animation id {id:?}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
