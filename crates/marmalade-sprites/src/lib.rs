//! Sprite loading and playback for Marmalade.
//!
//! Design goals: bounded memory (one animation resident, one frame when asleep), no timers of
//! its own (the caller arms one timer from `wake_in()`), and untrusted-input safety (strict
//! id validation, size caps checked before allocation).

mod error;
mod frame;
mod meta;
mod player;

pub use error::{Error, Result};
pub use frame::{decode_file, Animation, Frame};
pub use meta::{validate_id, AnimMeta, Point, MAX_DIM, MAX_FRAMES};
pub use player::SpritePlayer;

#[cfg(test)]
mod tests;
