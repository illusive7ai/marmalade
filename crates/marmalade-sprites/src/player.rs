use std::path::PathBuf;
use std::time::Duration;

use crate::error::Result;
use crate::frame::{Animation, Frame};
use crate::meta::validate_id;

enum Mode {
    /// Nothing loaded.
    Empty,
    /// One animation resident in memory.
    Playing(Animation),
    /// Asleep: a single frame kept for display, everything else freed.
    Resting(Frame),
}

/// Plays one animation at a time and keeps memory bounded:
/// only the playing animation is decoded, and `sleep()` drops all but one frame.
///
/// It owns no timer. Ask `wake_in()` when the next frame is due and arm a single OS timer
/// for that; when it returns `None` nothing needs to wake up.
pub struct SpritePlayer {
    root: PathBuf,
    mode: Mode,
    current_id: Option<String>,
    elapsed: f64,
    last_index: usize,
    mirrored: bool,
}

impl SpritePlayer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        SpritePlayer {
            root: root.into(),
            mode: Mode::Empty,
            current_id: None,
            elapsed: 0.0,
            last_index: 0,
            mirrored: false,
        }
    }

    /// Load `<root>/<id>/` and start it from the first frame. Frees the previous animation first,
    /// so peak memory is one animation (plus the new one while decoding).
    pub fn play(&mut self, id: &str) -> Result<()> {
        validate_id(id)?;
        let anim = Animation::load(&self.root.join(id))?;
        self.mode = Mode::Playing(anim);
        self.current_id = Some(id.to_string());
        self.elapsed = 0.0;
        self.last_index = 0;
        Ok(())
    }

    pub fn current_id(&self) -> Option<&str> {
        self.current_id.as_deref()
    }

    pub fn set_mirrored(&mut self, mirrored: bool) {
        self.mirrored = mirrored;
    }

    pub fn mirrored(&self) -> bool {
        self.mirrored
    }

    /// Advance time. Returns true when the visible frame changed (so a redraw is needed).
    pub fn tick(&mut self, dt: Duration) -> bool {
        if matches!(self.mode, Mode::Playing(_)) {
            self.elapsed += dt.as_secs_f64();
        }
        let idx = self.frame_index();
        let changed = idx != self.last_index;
        self.last_index = idx;
        changed
    }

    fn frame_index(&self) -> usize {
        match &self.mode {
            Mode::Playing(a) => {
                let n = a.frames.len();
                let raw = (self.elapsed * a.meta.fps as f64).floor() as usize;
                if a.meta.looping {
                    raw % n
                } else {
                    raw.min(n - 1)
                }
            }
            _ => 0,
        }
    }

    /// True once a non-looping animation has shown its last frame for a full frame time.
    pub fn is_finished(&self) -> bool {
        match &self.mode {
            Mode::Playing(a) if !a.meta.looping => {
                self.elapsed * a.meta.fps as f64 >= a.frames.len() as f64
            }
            _ => false,
        }
    }

    /// Time until the next frame change, or `None` if nothing will change (asleep, empty, or a
    /// finished one-shot). Use this to arm one timer instead of ticking at a fixed rate.
    pub fn wake_in(&self) -> Option<Duration> {
        match &self.mode {
            Mode::Playing(a) if !self.is_finished() => {
                let frame_time = 1.0 / a.meta.fps as f64;
                let into_frame = self.elapsed % frame_time;
                Some(Duration::from_secs_f64(
                    (frame_time - into_frame).max(0.001),
                ))
            }
            _ => None,
        }
    }

    pub fn frame(&self) -> Option<&Frame> {
        match &self.mode {
            Mode::Playing(a) => a.frames.get(self.frame_index()),
            Mode::Resting(f) => Some(f),
            Mode::Empty => None,
        }
    }

    /// Free everything except the frame currently shown.
    pub fn sleep(&mut self) {
        let idx = self.frame_index();
        if let Mode::Playing(a) = &mut self.mode {
            let keep = a.frames.swap_remove(idx);
            self.mode = Mode::Resting(keep);
            self.last_index = 0; // the resting frame is index 0; avoids a spurious "changed" on the next tick
        }
    }

    pub fn is_asleep(&self) -> bool {
        matches!(self.mode, Mode::Resting(_))
    }

    /// Decoded frame bytes currently held.
    pub fn memory_bytes(&self) -> usize {
        match &self.mode {
            Mode::Playing(a) => a.memory_bytes(),
            Mode::Resting(f) => f.byte_len(),
            Mode::Empty => 0,
        }
    }

    /// Source-over blit of the current frame (premultiplied BGRA) into `dst` at (x, y),
    /// mirrored horizontally if requested. Clips to the destination.
    pub fn blit(&self, dst: &mut [u8], dst_w: u32, dst_h: u32, x: i32, y: i32) {
        let Some(f) = self.frame() else { return };
        let (fw, fh) = (f.width as i32, f.height as i32);
        for sy in 0..fh {
            let dy = y + sy;
            if dy < 0 || dy >= dst_h as i32 {
                continue;
            }
            for sx in 0..fw {
                let dx = x + sx;
                if dx < 0 || dx >= dst_w as i32 {
                    continue;
                }
                let src_x = if self.mirrored { fw - 1 - sx } else { sx };
                let s = ((sy * fw + src_x) * 4) as usize;
                let d = ((dy as u32 * dst_w + dx as u32) * 4) as usize;
                let sa = f.bgra[s + 3] as u32;
                if sa == 0 {
                    continue;
                }
                for c in 0..4 {
                    let blended =
                        f.bgra[s + c] as u32 + (dst[d + c] as u32 * (255 - sa) + 127) / 255;
                    dst[d + c] = blended.min(255) as u8;
                }
            }
        }
    }
}
