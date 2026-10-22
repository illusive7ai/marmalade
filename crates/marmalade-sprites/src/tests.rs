use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::{validate_id, AnimMeta, Error, Frame, SpritePlayer, MAX_DIM};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique temp directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("marmalade-sprites-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let mut enc = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// Write `<root>/<id>/anim.toml` plus `frames` PNGs of `w`x`h`, frame i filled with grey level i*10+10.
fn make_anim(root: &Path, id: &str, fps: u32, looping: bool, frames: u32, w: u32, h: u32) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("anim.toml"),
        format!("fps = {fps}\nloop = {looping}\nframes = {frames}\n"),
    )
    .unwrap();
    for i in 1..=frames {
        let g = (i * 10 + 10) as u8;
        let px: Vec<u8> = (0..w * h).flat_map(|_| [g, g, g, 255]).collect();
        write_png(&dir.join(format!("{i:04}.png")), w, h, &px);
    }
}

#[test]
fn meta_parses_and_validates() {
    let m = AnimMeta::parse("fps = 12\nloop = true\nframes = 8\n[anchor]\nx = 1\ny = 2\n").unwrap();
    assert_eq!(m.frames, 8);
    assert!(m.looping);
    assert_eq!(m.anchor.unwrap().y, 2);
    assert!(!AnimMeta::parse("fps = 12\nframes = 8\n").unwrap().looping);

    assert!(AnimMeta::parse("fps = 0\nframes = 8\n").is_err());
    assert!(AnimMeta::parse("fps = 500\nframes = 8\n").is_err());
    assert!(AnimMeta::parse("fps = 12\nframes = 0\n").is_err());
    assert!(AnimMeta::parse("fps = 12\nframes = 1000\n").is_err());
    assert!(AnimMeta::parse("fps = 12\nframes = 8\nsurprise = 1\n").is_err());
}

#[test]
fn ids_cannot_escape_the_assets_dir() {
    for ok in ["walk", "sleep_2", "a"] {
        assert!(validate_id(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "..",
        "../walk",
        "a/b",
        "a\\b",
        "C:\\x",
        "Walk",
        "walk.png",
        &"a".repeat(33),
    ] {
        assert!(matches!(validate_id(bad), Err(Error::BadId(_))), "{bad}");
    }
    let mut p = SpritePlayer::new(std::env::temp_dir());
    assert!(matches!(p.play("../x"), Err(Error::BadId(_))));
}

#[test]
fn premultiplies_and_swaps_channels() {
    let f = Frame::from_rgba(1, 1, &[255, 0, 0, 128]);
    assert_eq!(f.bgra, vec![0, 0, 128, 128]);
    let f = Frame::from_rgba(1, 1, &[10, 20, 30, 0]);
    assert_eq!(f.bgra, vec![0, 0, 0, 0]);
}

#[test]
fn looping_animation_advances_and_wraps() {
    let t = TempDir::new();
    make_anim(t.path(), "walk", 10, true, 4, 2, 2); // 100 ms per frame
    let mut p = SpritePlayer::new(t.path());
    p.play("walk").unwrap();

    assert!(!p.tick(Duration::from_millis(50)));
    let wake = p.wake_in().unwrap();
    assert!(
        wake > Duration::from_millis(40) && wake <= Duration::from_millis(60),
        "{wake:?}"
    );
    assert!(p.tick(Duration::from_millis(60))); // 110 ms -> frame 1
    assert_eq!(p.frame().unwrap().bgra[0], 30); // second frame: grey = 2 * 10 + 10
    assert!(p.tick(Duration::from_millis(300))); // 410 ms -> index 4 % 4 = 0
    assert_eq!(p.frame().unwrap().bgra[0], 20); // back to the first frame
    assert!(!p.is_finished());
    assert!(p.wake_in().is_some());
}

#[test]
fn one_shot_stops_on_last_frame_and_needs_no_timer() {
    let t = TempDir::new();
    make_anim(t.path(), "leap", 10, false, 3, 2, 2);
    let mut p = SpritePlayer::new(t.path());
    p.play("leap").unwrap();

    p.tick(Duration::from_millis(2000));
    assert!(p.is_finished());
    assert_eq!(p.frame().unwrap().bgra[0], 40); // last frame
    assert_eq!(p.wake_in(), None);
    assert!(!p.tick(Duration::from_millis(500)));
}

#[test]
fn sleep_frees_all_but_one_frame() {
    let t = TempDir::new();
    make_anim(t.path(), "sit", 12, true, 12, 16, 16);
    let mut p = SpritePlayer::new(t.path());
    assert_eq!(p.memory_bytes(), 0);
    p.play("sit").unwrap();
    let full = p.memory_bytes();
    assert_eq!(full, 12 * 16 * 16 * 4);

    p.tick(Duration::from_millis(250));
    let shown = p.frame().unwrap().clone();
    p.sleep();
    assert!(p.is_asleep());
    assert_eq!(p.memory_bytes(), 16 * 16 * 4);
    assert_eq!(p.frame().unwrap(), &shown);
    assert_eq!(p.wake_in(), None);
    assert!(!p.tick(Duration::from_secs(5)));

    p.play("sit").unwrap(); // waking up reloads
    assert_eq!(p.memory_bytes(), full);
}

#[test]
fn switching_animation_replaces_the_old_one() {
    let t = TempDir::new();
    make_anim(t.path(), "walk", 10, true, 10, 8, 8);
    make_anim(t.path(), "sit", 10, true, 2, 8, 8);
    let mut p = SpritePlayer::new(t.path());
    p.play("walk").unwrap();
    assert_eq!(p.memory_bytes(), 10 * 8 * 8 * 4);
    p.play("sit").unwrap();
    assert_eq!(p.memory_bytes(), 2 * 8 * 8 * 4);
    assert_eq!(p.current_id(), Some("sit"));
}

#[test]
fn blit_mirrors_and_clips() {
    let t = TempDir::new();
    let dir = t.path().join("pose");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("anim.toml"), "fps = 1\nloop = true\nframes = 1\n").unwrap();
    // 2x1: left red, right blue, both opaque.
    write_png(
        &dir.join("0001.png"),
        2,
        1,
        &[255, 0, 0, 255, 0, 0, 255, 255],
    );

    let mut p = SpritePlayer::new(t.path());
    p.play("pose").unwrap();

    let mut dst = vec![0u8; 2 * 4];
    p.blit(&mut dst, 2, 1, 0, 0);
    assert_eq!(&dst[0..4], &[0, 0, 255, 255]); // BGRA red on the left
    assert_eq!(&dst[4..8], &[255, 0, 0, 255]); // blue on the right

    p.set_mirrored(true);
    let mut dst = vec![0u8; 2 * 4];
    p.blit(&mut dst, 2, 1, 0, 0);
    assert_eq!(&dst[0..4], &[255, 0, 0, 255]); // swapped
    assert_eq!(&dst[4..8], &[0, 0, 255, 255]);

    // Fully off-screen and partially clipped blits must not panic.
    let mut small = vec![0u8; 4];
    p.blit(&mut small, 1, 1, -5, -5);
    p.blit(&mut small, 1, 1, 0, 0);
    p.blit(&mut small, 1, 1, 1, 1);
}

#[test]
fn rejects_missing_oversized_and_inconsistent_frames() {
    let t = TempDir::new();

    // Missing frame file.
    let dir = t.path().join("gap");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("anim.toml"), "fps = 10\nframes = 2\n").unwrap();
    write_png(&dir.join("0001.png"), 1, 1, &[0, 0, 0, 255]);
    let mut p = SpritePlayer::new(t.path());
    assert!(matches!(p.play("gap"), Err(Error::Image(_))));

    // Over the dimension cap, rejected from the header before allocating the pixel buffer.
    let dir = t.path().join("huge");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("anim.toml"), "fps = 10\nframes = 1\n").unwrap();
    let w = MAX_DIM + 1;
    write_png(&dir.join("0001.png"), w, 1, &vec![0u8; (w * 4) as usize]);
    assert!(matches!(p.play("huge"), Err(Error::Image(_))));

    // Frames of different sizes.
    let dir = t.path().join("mixed");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("anim.toml"), "fps = 10\nframes = 2\n").unwrap();
    write_png(&dir.join("0001.png"), 1, 1, &[0, 0, 0, 255]);
    write_png(&dir.join("0002.png"), 2, 1, &[0, 0, 0, 255, 0, 0, 0, 255]);
    assert!(matches!(p.play("mixed"), Err(Error::Image(_))));

    // A failed play leaves the previously loaded animation untouched.
    make_anim(t.path(), "ok", 10, true, 2, 2, 2);
    p.play("ok").unwrap();
    let before = p.memory_bytes();
    assert!(p.play("gap").is_err());
    assert_eq!(p.memory_bytes(), before);
    assert_eq!(p.current_id(), Some("ok"));
}

#[test]
fn decodes_lossless_webp_with_alpha() {
    let t = TempDir::new();
    let dir = t.path().join("w");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("anim.toml"), "fps = 10\nframes = 1\n").unwrap();

    let rgba = [200u8, 100, 50, 255, 0, 0, 0, 0];
    let out = BufWriter::new(File::create(dir.join("0001.webp")).unwrap());
    image_webp::WebPEncoder::new(out)
        .encode(&rgba, 2, 1, image_webp::ColorType::Rgba8)
        .unwrap();

    let mut p = SpritePlayer::new(t.path());
    p.play("w").unwrap();
    let f = p.frame().unwrap();
    assert_eq!((f.width, f.height), (2, 1));
    assert_eq!(&f.bgra[0..4], &[50, 100, 200, 255]);
    assert_eq!(f.bgra[7], 0);
}

#[test]
fn reports_duration_and_a_mirrored_mouth() {
    let t = TempDir::new();
    make_anim(t.path(), "carry", 10, true, 5, 8, 4);
    let toml = t.path().join("carry").join("anim.toml");
    let mut text = fs::read_to_string(&toml).unwrap();
    text.push_str(
        "[mouth]
x = 6
y = 1
",
    );
    fs::write(&toml, text).unwrap();

    let mut p = SpritePlayer::new(t.path());
    assert_eq!(p.duration(), None);
    assert_eq!(p.mouth(), None);
    p.play("carry").unwrap();
    assert_eq!(p.duration(), Some(Duration::from_millis(500)));
    assert_eq!(p.mouth(), Some((6, 1)));
    p.set_mirrored(true);
    assert_eq!(p.mouth(), Some((1, 1))); // 8 - 1 - 6
}
