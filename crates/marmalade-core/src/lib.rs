//! Marmalade core: the validated action vocabulary and the local command router.
//!
//! No I/O happens here. Parsing text into an `Action` and validating its arguments is pure logic,
//! so it is fast, testable, and the single choke point for everything the app is allowed to do.

pub mod action;
pub mod paths;
pub mod router;

pub use action::{
    can_convert, Action, FileKind, FileRef, FindSpec, Format, PrintOptions, Risk, Since,
};
pub use paths::PathError;
pub use router::{route, Routed};

#[cfg(test)]
mod tests;
