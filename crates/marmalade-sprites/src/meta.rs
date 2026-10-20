use serde::Deserialize;

use crate::error::{Error, Result};

/// Hard limits. Sprite packs are untrusted input (community skins), so everything is capped.
pub const MAX_FRAMES: u32 = 64;
pub const MAX_DIM: u32 = 512;
pub const MAX_FPS: f32 = 60.0;
pub const MAX_META_BYTES: u64 = 16 * 1024;
pub const MAX_FRAME_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Contents of `anim.toml`.
///
/// ```toml
/// fps = 12
/// loop = true
/// frames = 12
/// [anchor]   # foot point, in frame pixels
/// x = 128
/// y = 250
/// [mouth]    # only for `carry`: where the carried file is drawn
/// x = 200
/// y = 140
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimMeta {
    pub fps: f32,
    #[serde(rename = "loop", default)]
    pub looping: bool,
    pub frames: u32,
    pub anchor: Option<Point>,
    pub mouth: Option<Point>,
}

impl AnimMeta {
    pub fn parse(text: &str) -> Result<Self> {
        let meta: AnimMeta = toml::from_str(text).map_err(|e| Error::Meta(e.to_string()))?;
        meta.validate()?;
        Ok(meta)
    }

    fn validate(&self) -> Result<()> {
        if !(self.fps.is_finite() && self.fps > 0.0 && self.fps <= MAX_FPS) {
            return Err(Error::Meta(format!(
                "fps must be in (0, {MAX_FPS}], got {}",
                self.fps
            )));
        }
        if self.frames == 0 || self.frames > MAX_FRAMES {
            return Err(Error::Meta(format!(
                "frames must be in 1..={MAX_FRAMES}, got {}",
                self.frames
            )));
        }
        Ok(())
    }
}

/// Animation ids become directory names, so only `[a-z0-9_]` (1..=32 chars) is accepted.
pub fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(Error::BadId(id.to_string()))
    }
}
