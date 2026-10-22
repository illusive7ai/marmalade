//! Props drawn by the app on top of the sprites: the folder the cat jumps into and the file it
//! carries. Plain premultiplied BGRA buffers, opaque pixels only, so no sprite art is needed.

use crate::fetch::{FOLDER_H, FOLDER_W};

pub const FILE_W: i32 = 28;
pub const FILE_H: i32 = 34;

type Rgb = (u8, u8, u8);

const OUTLINE: Rgb = (60, 52, 46);
const FOLDER_BACK: Rgb = (214, 150, 48);
const FOLDER_FRONT: Rgb = (250, 198, 76);
const FOLDER_SHINE: Rgb = (255, 224, 140);
const PAGE: Rgb = (248, 248, 250);
const PAGE_FOLD: Rgb = (200, 206, 220);
const PAGE_LINE: Rgb = (150, 160, 184);

/// Opaque `w`x`h` BGRA surface with just enough drawing for icons.
struct Surface<'a> {
    buf: &'a mut [u8],
    w: i32,
    h: i32,
}

impl Surface<'_> {
    fn put(&mut self, x: i32, y: i32, (r, g, b): Rgb) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&[b, g, r, 255]);
    }

    /// Filled rectangle `[x0, x1) x [y0, y1)` with corners rounded by `r`.
    fn round_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, c: Rgb) {
        for y in y0..y1 {
            for x in x0..x1 {
                let cx = if x < x0 + r {
                    x0 + r - 1
                } else if x >= x1 - r {
                    x1 - r
                } else {
                    x
                };
                let cy = if y < y0 + r {
                    y0 + r - 1
                } else if y >= y1 - r {
                    y1 - r
                } else {
                    y
                };
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= r * r {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// A rounded rectangle with a 1px outline.
    fn outlined(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, c: Rgb) {
        self.round_rect(x0, y0, x1, y1, r, OUTLINE);
        self.round_rect(x0 + 1, y0 + 1, x1 - 1, y1 - 1, (r - 1).max(0), c);
    }
}

/// A `FOLDER_W` x `FOLDER_H` folder icon as premultiplied BGRA (transparent corners).
pub fn folder_pixels() -> Vec<u8> {
    let (w, h) = (FOLDER_W, FOLDER_H);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut s = Surface {
        buf: &mut buf,
        w,
        h,
    };
    s.outlined(0, 12, w, h, 7, FOLDER_BACK);
    s.outlined(6, 0, 40, 20, 5, FOLDER_BACK);
    s.outlined(0, 28, w, h, 7, FOLDER_FRONT);
    s.round_rect(5, 32, w - 5, 36, 2, FOLDER_SHINE);
    buf
}

/// Draw the carried file centred on (`cx`, `cy`) into a `dst_w`x`dst_h` premultiplied BGRA buffer.
pub fn draw_file(dst: &mut [u8], dst_w: i32, dst_h: i32, cx: i32, cy: i32) {
    debug_assert_eq!(dst.len(), (dst_w * dst_h * 4) as usize);
    let (x0, y0) = (cx - FILE_W / 2, cy - FILE_H / 2);
    let mut s = Surface {
        buf: dst,
        w: dst_w,
        h: dst_h,
    };
    s.outlined(x0, y0, x0 + FILE_W, y0 + FILE_H, 3, PAGE);
    // Dog-ear: clear the top-right corner, then draw the fold.
    let fold = 9;
    for i in 0..fold {
        for j in 0..(fold - i) {
            s.put(x0 + FILE_W - 1 - j, y0 + i, OUTLINE);
        }
    }
    for i in 1..fold - 1 {
        for j in 0..(fold - 1 - i) {
            s.put(x0 + FILE_W - 2 - j, y0 + 1 + i, PAGE_FOLD);
        }
    }
    for row in 0..3 {
        let y = y0 + 15 + row * 5;
        let end = if row == 2 { 18 } else { 23 };
        for x in 5..end {
            s.put(x0 + x, y, PAGE_LINE);
        }
    }
}

/// The file icon on its own, for the window it becomes once the cat drops it.
pub fn file_pixels() -> Vec<u8> {
    let mut buf = vec![0u8; (FILE_W * FILE_H * 4) as usize];
    draw_file(&mut buf, FILE_W, FILE_H, FILE_W / 2, FILE_H / 2);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(buf: &[u8], w: i32, x: i32, y: i32) -> u8 {
        buf[((y * w + x) * 4 + 3) as usize]
    }

    #[test]
    fn folder_has_opaque_body_and_transparent_corners() {
        let px = folder_pixels();
        assert_eq!(px.len(), (FOLDER_W * FOLDER_H * 4) as usize);
        assert_eq!(alpha(&px, FOLDER_W, FOLDER_W / 2, FOLDER_H / 2), 255);
        assert_eq!(alpha(&px, FOLDER_W, 0, FOLDER_H - 1), 0); // rounded bottom-left corner
        assert_eq!(alpha(&px, FOLDER_W, FOLDER_W - 1, 0), 0); // beside the tab
    }

    #[test]
    fn the_dropped_file_icon_fills_its_own_window() {
        let px = file_pixels();
        assert_eq!(px.len(), (FILE_W * FILE_H * 4) as usize);
        assert_eq!(alpha(&px, FILE_W, FILE_W / 2, FILE_H / 2), 255);
    }

    #[test]
    fn file_is_drawn_around_its_centre_and_clipped_at_the_edges() {
        let (w, h) = (64, 64);
        let mut dst = vec![0u8; (w * h * 4) as usize];
        draw_file(&mut dst, w, h, 32, 32);
        assert_eq!(alpha(&dst, w, 32, 32), 255);
        assert_eq!(alpha(&dst, w, 2, 2), 0);

        // Mostly off-screen: must not panic or wrap around.
        let mut edge = vec![0u8; (w * h * 4) as usize];
        draw_file(&mut edge, w, h, 0, 0);
        draw_file(&mut edge, w, h, w + 5, h + 5);
        assert_eq!(alpha(&edge, w, w - 1, 0), 0);
    }
}
