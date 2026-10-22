//! Every pack in `assets/cat` must load through the same code path the app uses.

use std::path::PathBuf;

use marmalade_sprites::SpritePlayer;

const P0: [&str; 7] = ["sleep", "sit", "lie", "walk", "leap", "peek", "carry"];

fn cat_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/cat")
}

#[test]
fn every_p0_pack_loads_and_fits_the_budget() {
    let dir = cat_dir();
    if !dir.is_dir() {
        return; // the real cat is optional; the placeholder set is the fallback
    }
    let mut total = 0;
    for id in P0 {
        let mut player = SpritePlayer::new(&dir);
        player
            .play(id)
            .unwrap_or_else(|e| panic!("{id} failed to load: {e}"));
        let frame = player.frame().expect("first frame");
        assert!(frame.width <= 256 && frame.height <= 256, "{id} too large");
        assert!(player.duration().is_some());
        total += player.memory_bytes();
        // A pack may hold at most one animation in memory, so the largest one is the cap.
        assert!(
            player.memory_bytes() < 12 * 1024 * 1024,
            "{id} too big in RAM"
        );
    }
    assert!(total > 0);
}

#[test]
fn carry_declares_a_mouth_inside_the_frame() {
    let dir = cat_dir();
    if !dir.is_dir() {
        return;
    }
    let mut player = SpritePlayer::new(&dir);
    player.play("carry").unwrap();
    let (x, y) = player.mouth().expect("carry needs [mouth]");
    assert!((0..256).contains(&x) && (0..256).contains(&y));
}
