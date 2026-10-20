//! Generates the placeholder sprite set (flat shapes, not a cat) so the app runs before real frames exist.
//!
//!     cargo run -p marmalade-sprites --example gen_placeholders -- assets/placeholder
//!
//! Output layout matches docs/ASSET-SPEC.md: `<out>/<id>/anim.toml` and `<out>/<id>/0001.png`...

use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

const SIZE: u32 = 256;

struct Spec {
    id: &'static str,
    frames: u32,
    fps: u32,
    looping: bool,
    rgb: (u8, u8, u8),
    mouth: bool,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "sleep",
        frames: 8,
        fps: 4,
        looping: true,
        rgb: (120, 130, 160),
        mouth: false,
    },
    Spec {
        id: "sit",
        frames: 12,
        fps: 8,
        looping: true,
        rgb: (200, 200, 205),
        mouth: false,
    },
    Spec {
        id: "lie",
        frames: 8,
        fps: 6,
        looping: true,
        rgb: (180, 190, 180),
        mouth: false,
    },
    Spec {
        id: "walk",
        frames: 12,
        fps: 12,
        looping: true,
        rgb: (160, 170, 190),
        mouth: false,
    },
    Spec {
        id: "leap",
        frames: 12,
        fps: 12,
        looping: false,
        rgb: (230, 170, 90),
        mouth: false,
    },
    Spec {
        id: "peek",
        frames: 12,
        fps: 10,
        looping: false,
        rgb: (140, 190, 140),
        mouth: false,
    },
    Spec {
        id: "carry",
        frames: 12,
        fps: 12,
        looping: true,
        rgb: (190, 150, 190),
        mouth: true,
    },
];

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn main() {
    let out: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/placeholder".into())
        .into();

    for s in SPECS {
        let dir = out.join(s.id);
        fs::create_dir_all(&dir).unwrap();
        let mouth = if s.mouth {
            "\n[mouth]\nx = 200\ny = 150\n"
        } else {
            ""
        };
        fs::write(
            dir.join("anim.toml"),
            format!(
                "fps = {}\nloop = {}\nframes = {}\n\n[anchor]\nx = {}\ny = {}\n{mouth}",
                s.fps,
                s.looping,
                s.frames,
                SIZE / 2,
                SIZE - 8
            ),
        )
        .unwrap();

        for i in 0..s.frames {
            let t = i as f32 / s.frames as f32;
            let mut pm = Pixmap::new(SIZE, SIZE).unwrap();

            // Body: a rounded block that bobs a little so playback is visible.
            let bob = (t * TAU).sin() * 6.0;
            let (r, g, b) = s.rgb;
            let body = Rect::from_xywh(40.0, 110.0 + bob, 176.0, 110.0).unwrap();
            let mut pb = PathBuilder::new();
            pb.push_rect(body);
            pm.fill_path(
                &pb.finish().unwrap(),
                &paint(Color::from_rgba8(r, g, b, 255)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );

            // Progress bar along the top: shows the frame index at a glance.
            let w = 200.0 * (i + 1) as f32 / s.frames as f32;
            let mut pb = PathBuilder::new();
            pb.push_rect(Rect::from_xywh(28.0, 20.0, w, 10.0).unwrap());
            pm.fill_path(
                &pb.finish().unwrap(),
                &paint(Color::from_rgba8(40, 40, 40, 255)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );

            // Facing marker on the right side (placeholders face right, like the real clips).
            let mut pb = PathBuilder::new();
            pb.push_circle(196.0, 140.0 + bob, 12.0);
            pm.fill_path(
                &pb.finish().unwrap(),
                &paint(Color::from_rgba8(30, 30, 30, 255)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );

            pm.save_png(dir.join(format!("{:04}.png", i + 1))).unwrap();
        }
        println!("wrote {} ({} frames)", dir.display(), s.frames);
    }
}
