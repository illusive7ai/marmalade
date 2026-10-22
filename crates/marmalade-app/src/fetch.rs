//! The file-fetch scene as a pure timeline: no Win32, so it can be unit tested.
//!
//! One continuous performance, built from segments that each start where the last one ended:
//! get up and walk to the folder, crouch and spring in (fading as it goes), the folder wobbles,
//! the cat rises from behind it with the file, walks back and sits, the file is delivered,
//! then it strolls home. `cat.rs` drives this and applies each `Pose`.

use crate::cat::W;
use crate::props::{FILE_H, FILE_W};

pub const FOLDER_W: i32 = 96;
pub const FOLDER_H: i32 = 80;
/// Distance between the cat's centre and the folder's centre at the start.
const GAP: f64 = 360.0;
/// Distance from the folder's centre at which the cat crouches and springs.
const JUMP: f64 = 190.0;
/// Feet line of the sprite inside the cat window (matches `anchor.y` of the clips).
const FEET: i32 = 248;
/// The cat window is raised by this much while peeking so the clip's bottom edge hides behind the folder.
const PEEK_LIFT: f64 = 40.0;
/// How far below its resting height the cat starts when rising from behind the folder.
const RISE: f64 = 130.0;
const SINK: f64 = 36.0;
const ARC: f64 = 46.0;
/// Walk speeds in px/s at the middle of an eased walk; tuned by eye against the clips' gait.
pub const WALK_SPEED: f64 = 150.0;
pub const CARRY_SPEED: f64 = 200.0;
const LANDING_OFFSET: f64 = 40.0;
const MIN_WALK: f64 = 24.0;
/// Safety net if a clip is missing or never reports finished.
const TOTAL_MAX_SECS: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    Linear,
    InOut,
    Out,
    OutBack,
    /// Stays put for the first 30%, then eases (the crouch before a spring).
    Late,
}

fn ease(e: Ease, q: f64) -> f64 {
    let q = q.clamp(0.0, 1.0);
    match e {
        Ease::Linear => q,
        Ease::InOut => q * q * (3.0 - 2.0 * q),
        Ease::Out => 1.0 - (1.0 - q).powi(3),
        Ease::OutBack => {
            let (c1, c3) = (1.70158, 2.70158);
            1.0 + c3 * (q - 1.0).powi(3) + c1 * (q - 1.0).powi(2)
        }
        Ease::Late => {
            let r = ((q - 0.3) / 0.7).clamp(0.0, 1.0);
            r * r * (3.0 - 2.0 * r)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name {
    Approach,
    Leap,
    Hidden,
    Peek,
    Return,
    Settle,
    Home,
    Perch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Until {
    Secs(f64),
    /// Until the one-shot clip finishes, but never longer than `max`.
    ClipDone {
        max: f64,
    },
}

/// The carried file leaving the cat's mouth and landing on the ground.
#[derive(Debug, Clone, Copy)]
pub struct FileDrop {
    /// Top-left of the icon, at the mouth and where it comes to rest.
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// Seconds into the segment before it lets go, and how long the fall takes.
    pub delay: f64,
    pub secs: f64,
}

/// Ease-out bounce: 0 to 1, rebounding a little before it settles (a dropped object).
fn bounce(q: f64) -> f64 {
    let (n, d) = (7.5625, 2.75);
    let q = q.clamp(0.0, 1.0);
    if q < 1.0 / d {
        n * q * q
    } else if q < 2.0 / d {
        let q = q - 1.5 / d;
        n * q * q + 0.75
    } else if q < 2.5 / d {
        let q = q - 2.25 / d;
        n * q * q + 0.9375
    } else {
        let q = q - 2.625 / d;
        n * q * q + 0.984375
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Seg {
    #[allow(dead_code)] // read by the tests; handy when debugging a scene
    pub name: Name,
    pub clip: Option<&'static str>,
    pub until: Until,
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub ease_x: Ease,
    pub ease_y: Ease,
    /// The vertical move finishes after this long (default: the whole segment).
    pub y_secs: Option<f64>,
    /// Extra upward arc in px, peaking mid-segment.
    pub arc: f64,
    pub mirrored: bool,
    pub visible: bool,
    /// Progress at which the cat starts fading out, reaching nothing at the end.
    pub fade_from: Option<f64>,
    pub carrying: bool,
    pub folder: bool,
    pub shake: bool,
    /// Hide everything of the cat below the folder's bottom edge (it is behind the folder).
    pub clip_line: bool,
    /// Hand over the file when this segment starts.
    pub deliver: bool,
    /// The file falling out of the cat's mouth during this segment.
    pub file: Option<FileDrop>,
}

impl Seg {
    fn base(name: Name, from: (f64, f64)) -> Seg {
        Seg {
            name,
            clip: None,
            until: Until::Secs(1.0),
            from,
            to: from,
            ease_x: Ease::InOut,
            ease_y: Ease::InOut,
            y_secs: None,
            arc: 0.0,
            mirrored: false,
            visible: true,
            fade_from: None,
            carrying: false,
            folder: true,
            shake: false,
            clip_line: false,
            deliver: false,
            file: None,
        }
    }

    fn secs(&self) -> f64 {
        match self.until {
            Until::Secs(s) => s,
            Until::ClipDone { max } => max,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Top-left of the cat window when the scene starts.
    pub origin: (i32, i32),
    /// +1 when the folder is to the right of the cat, -1 to the left.
    pub dir: i32,
    /// Top-left of the folder window.
    pub folder: (i32, i32),
    /// Left edge of the cat window when the cat is centred on the folder.
    pub at_folder_x: i32,
}

/// Put the folder on the side of the cat that has room, inside the horizontal work area.
pub fn plan(origin: (i32, i32), work_left: i32, work_right: i32) -> Plan {
    let gap = GAP as i32;
    let center = origin.0 + W / 2;
    let room_right = work_right - (center + gap + FOLDER_W / 2);
    let room_left = (center - gap - FOLDER_W / 2) - work_left;
    let want = if room_right >= 0 || room_right >= room_left {
        1
    } else {
        -1
    };
    let lo = work_left + FOLDER_W / 2;
    let hi = (work_right - FOLDER_W / 2).max(lo);
    let folder_cx = (center + want * gap).clamp(lo, hi);
    let dir = if folder_cx < center { -1 } else { 1 };
    Plan {
        origin,
        dir,
        folder: (folder_cx - FOLDER_W / 2, origin.1 + FEET + 4 - FOLDER_H),
        at_folder_x: folder_cx - W / 2,
    }
}

/// The whole performance. `leap_secs` is the length of the leap clip; `home` is where the cat's
/// window rests in its corner; `mouth` is where the carried file is held in the cat's window while
/// it walks back (already flipped for the direction it faces).
pub fn build(plan: Plan, leap_secs: f64, home: (i32, i32), mouth: (i32, i32)) -> Vec<Seg> {
    let (ox, oy) = (plan.origin.0 as f64, plan.origin.1 as f64);
    let d = plan.dir as f64;
    let at_folder = plan.at_folder_x as f64;
    let out_mirrored = plan.dir < 0;
    let mut segs = Vec::new();

    // 1. Walk up to the jump point.
    let jump_x = at_folder - d * JUMP;
    let mut x = ox;
    if (jump_x - ox) * d > MIN_WALK {
        let dist = (jump_x - ox).abs();
        let mut s = Seg::base(Name::Approach, (ox, oy));
        s.clip = Some("walk");
        s.to = (jump_x, oy);
        s.until = Until::Secs((dist / (WALK_SPEED * 0.8)).max(0.6));
        s.mirrored = out_mirrored;
        segs.push(s);
        x = jump_x;
    }

    // 2. Crouch and spring into the folder, fading out as the last of it goes in.
    let mut leap = Seg::base(Name::Leap, (x, oy));
    leap.clip = Some("leap");
    leap.to = (at_folder - d * LANDING_OFFSET, oy + SINK);
    leap.ease_x = Ease::Late;
    leap.ease_y = Ease::Linear;
    leap.arc = ARC;
    leap.until = Until::Secs(leap_secs + 0.2);
    leap.mirrored = out_mirrored;
    leap.fade_from = Some(0.72);
    segs.push(leap);

    // 3. Out of sight: the folder wobbles.
    let mut hidden = Seg::base(Name::Hidden, leap.to);
    hidden.until = Until::Secs(0.55);
    hidden.visible = false;
    hidden.shake = true;
    segs.push(hidden);

    // 4. Rise from behind the folder and look around.
    let peek_y = oy - PEEK_LIFT;
    let mut peek = Seg::base(Name::Peek, (at_folder, peek_y + RISE));
    peek.clip = Some("peek");
    peek.to = (at_folder, peek_y);
    peek.ease_y = Ease::OutBack;
    peek.y_secs = Some(0.4);
    peek.until = Until::ClipDone { max: 4.0 };
    peek.mirrored = out_mirrored;
    peek.clip_line = true;
    segs.push(peek);

    // 5. Climb out carrying the file and walk back, slowing to a stop.
    let back = (at_folder - ox).abs();
    let mut ret = Seg::base(Name::Return, (at_folder, oy + RISE));
    ret.clip = Some("carry");
    ret.to = (ox, oy);
    ret.ease_y = Ease::Out;
    ret.y_secs = Some(0.45);
    ret.until = Until::Secs((back / (CARRY_SPEED * 0.8)).max(0.9));
    ret.mirrored = !out_mirrored;
    ret.carrying = true;
    ret.clip_line = true;
    segs.push(ret);

    // 6. Sit down and drop the file on the ground in front of the paws.
    // The cat sits facing back the way it came (mirrored when it set out to the right).
    let facing = if out_mirrored { 1.0 } else { -1.0 };
    let (fw, fh) = (FILE_W as f64, FILE_H as f64);
    let mut settle = Seg::base(Name::Settle, (ox, oy));
    settle.clip = Some("sit");
    settle.until = Until::Secs(1.9);
    settle.mirrored = !out_mirrored;
    settle.file = Some(FileDrop {
        from: (
            ox + mouth.0 as f64 - fw / 2.0,
            oy + mouth.1 as f64 - fh / 2.0,
        ),
        to: (
            ox + W as f64 / 2.0 + facing * 74.0 - fw / 2.0,
            oy + FEET as f64 - fh + 4.0,
        ),
        delay: 0.25,
        secs: 0.55,
    });
    segs.push(settle);

    // 7. Stroll home to the corner.
    let (hx, hy) = (home.0 as f64, home.1 as f64);
    if (hx - ox).abs() >= MIN_WALK {
        let mut go = Seg::base(Name::Home, (ox, oy));
        go.clip = Some("walk");
        go.to = (hx, hy);
        // A brisk stroll, and never a trek: the corner can be most of a screen away.
        let brisk = WALK_SPEED * 1.8 * 0.8;
        go.until = Until::Secs(((hx - ox).abs() / brisk).clamp(0.8, 4.5));
        go.mirrored = hx < ox;
        go.folder = false;
        segs.push(go);
    } else if (hy - oy).abs() > 0.5 {
        // Same column, different height: no walk to show, just settle into place.
        segs.last_mut().expect("settle").to = (hx, hy);
    }

    // The file is opened as the cat leaves it behind.
    let settle_idx = segs
        .iter()
        .position(|s| s.name == Name::Settle)
        .expect("settle");
    segs[settle_idx + 1].deliver = true;

    // 8. Settle into the corner: a short sit so the walk dissolves into the resting pose.
    let end = segs.last().expect("settle").to;
    let mut perch = Seg::base(Name::Perch, end);
    perch.clip = Some("sit");
    perch.until = Until::Secs(0.4);
    perch.mirrored = segs.last().expect("settle").mirrored;
    perch.folder = false;
    segs.push(perch);
    segs
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub cat: (i32, i32),
    pub visible: bool,
    pub alpha: f32,
    pub mirrored: bool,
    pub carrying: bool,
    pub folder: (i32, i32),
    pub folder_visible: bool,
    /// Screen y at and below which the cat is hidden (behind the folder), if any.
    pub clip_y: Option<i32>,
    /// Top-left of the dropped file's icon while it is out of the cat's mouth and on the desktop.
    pub file: Option<(i32, i32)>,
}

pub struct Scene {
    segs: Vec<Seg>,
    folder: (i32, i32),
    idx: usize,
    t: f64,
    total: f64,
    deliver_pending: bool,
}

impl Scene {
    pub fn new(segs: Vec<Seg>, folder: (i32, i32)) -> Self {
        assert!(!segs.is_empty());
        Scene {
            deliver_pending: segs[0].deliver,
            segs,
            folder,
            idx: 0,
            t: 0.0,
            total: 0.0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.idx >= self.segs.len()
    }

    pub fn current(&self) -> Option<&Seg> {
        self.segs.get(self.idx)
    }

    /// True once, when a segment that hands over the file has started.
    pub fn take_deliver(&mut self) -> bool {
        std::mem::take(&mut self.deliver_pending)
    }

    /// Advance by `dt` seconds. `clip_done` says whether the current one-shot clip has finished.
    /// Returns true when a new segment began, so the caller can start its clip. At most one
    /// segment change per call.
    pub fn tick(&mut self, dt: f64, clip_done: bool) -> bool {
        let Some(seg) = self.current().copied() else {
            return false;
        };
        self.t += dt;
        self.total += dt;
        let finished = match seg.until {
            Until::Secs(s) => self.t >= s,
            Until::ClipDone { max } => clip_done || self.t >= max,
        } || self.total > TOTAL_MAX_SECS;
        if !finished {
            return false;
        }
        self.idx += 1;
        self.t = 0.0;
        if let Some(next) = self.segs.get(self.idx) {
            self.deliver_pending |= next.deliver;
        }
        true
    }

    pub fn pose(&self) -> Pose {
        let (seg, q_time) = match self.segs.get(self.idx) {
            Some(s) => (s, self.t),
            None => {
                let last = self.segs.last().expect("non-empty");
                (last, last.secs())
            }
        };
        let q = (q_time / seg.secs()).clamp(0.0, 1.0);
        let qy = seg.y_secs.map_or(q, |ys| (q_time / ys).clamp(0.0, 1.0));
        let x = seg.from.0 + (seg.to.0 - seg.from.0) * ease(seg.ease_x, q);
        let y = seg.from.1 + (seg.to.1 - seg.from.1) * ease(seg.ease_y, qy)
            - seg.arc * (std::f64::consts::PI * q).sin();
        let alpha = match seg.fade_from {
            Some(f) if q > f => (1.0 - (q - f) / (1.0 - f)) as f32,
            _ => 1.0,
        };
        let mut folder = self.folder;
        if seg.shake {
            folder.0 += if (q_time * 12.5) as i64 % 2 == 0 {
                -3
            } else {
                3
            };
        }
        Pose {
            cat: (x.round() as i32, y.round() as i32),
            visible: seg.visible,
            alpha,
            mirrored: seg.mirrored,
            carrying: seg.carrying,
            folder,
            folder_visible: seg.folder && !self.is_done(),
            clip_y: seg.clip_line.then_some(self.folder.1 + FOLDER_H),
            file: seg.file.map(|f| {
                let q = ((q_time - f.delay) / f.secs).clamp(0.0, 1.0);
                let x = f.from.0 + (f.to.0 - f.from.0) * ease(Ease::Out, q);
                let y = f.from.1 + (f.to.1 - f.from.1) * bounce(q);
                (x.round() as i32, y.round() as i32)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: i32 = 0;
    const RIGHT: i32 = 1920;
    const HOME: (i32, i32) = (1650, 780);
    const MOUTH: (i32, i32) = (20, 150);

    fn segs(origin: (i32, i32), leap_secs: f64) -> (Plan, Vec<Seg>) {
        let p = plan(origin, LEFT, RIGHT);
        (p, build(p, leap_secs, HOME, MOUTH))
    }

    fn names(v: &[Seg]) -> Vec<Name> {
        v.iter().map(|s| s.name).collect()
    }

    /// Run a scene to the end, reporting clips as done as soon as they have run for a moment.
    fn run(scene: &mut Scene) -> Vec<Name> {
        let mut seen = vec![scene.current().unwrap().name];
        for _ in 0..4000 {
            let done = scene.t > 0.3;
            scene.tick(0.016, done);
            match scene.current() {
                Some(s) if seen.last() != Some(&s.name) => seen.push(s.name),
                None => break,
                _ => {}
            }
        }
        seen
    }

    #[test]
    fn folder_goes_on_the_side_with_room() {
        assert_eq!(plan((100, 700), LEFT, RIGHT).dir, 1);
        assert_eq!(plan((1600, 700), LEFT, RIGHT).dir, -1);
        for o in [(100, 700), (1600, 700), (50, 700)] {
            let p = plan(o, LEFT, RIGHT);
            assert!(p.folder.0 >= LEFT && p.folder.0 + FOLDER_W <= RIGHT);
        }
    }

    #[test]
    fn folder_stays_on_screen_in_a_narrow_work_area() {
        let p = plan((50, 700), 0, 400);
        assert!(p.folder.0 >= 0 && p.folder.0 + FOLDER_W <= 400);
    }

    #[test]
    fn performs_every_beat_in_order() {
        let (p, v) = segs((300, 600), 0.8);
        assert_eq!(
            names(&v),
            [
                Name::Approach,
                Name::Leap,
                Name::Hidden,
                Name::Peek,
                Name::Return,
                Name::Settle,
                Name::Home,
                Name::Perch
            ]
        );
        let mut scene = Scene::new(v, p.folder);
        assert_eq!(run(&mut scene).len(), 8);
        assert!(scene.is_done());
    }

    #[test]
    fn each_move_starts_where_the_previous_one_ended() {
        for origin in [(300, 600), (1600, 600)] {
            let (_, v) = segs(origin, 0.8);
            for pair in v.windows(2) {
                let (a, b) = (&pair[0], &pair[1]);
                // The cat is out of sight between the leap and the peek, and the peek and return
                // both start hidden behind the folder; every visible hand-off must line up.
                let hidden_gap = matches!(
                    (a.name, b.name),
                    (Name::Leap, Name::Hidden)
                        | (Name::Hidden, Name::Peek)
                        | (Name::Peek, Name::Return)
                );
                if !hidden_gap {
                    assert_eq!(a.to, b.from, "{:?} -> {:?}", a.name, b.name);
                }
            }
        }
    }

    #[test]
    fn ends_in_the_corner_and_fires_the_delivery_exactly_once() {
        let (p, v) = segs((300, 600), 0.8);
        let mut scene = Scene::new(v, p.folder);
        let mut delivered = 0;
        for _ in 0..4000 {
            let done = scene.t > 0.3;
            scene.tick(0.016, done);
            if scene.take_deliver() {
                delivered += 1;
                // The file is opened once the cat has dropped it and gets up to leave.
                assert_eq!(scene.current().unwrap().name, Name::Home);
            }
            if scene.is_done() {
                break;
            }
        }
        assert_eq!(delivered, 1);
        assert_eq!(scene.pose().cat, HOME);
        assert!(!scene.pose().folder_visible);
    }

    #[test]
    fn cat_is_hidden_only_while_the_folder_wobbles_and_carries_only_on_the_way_back() {
        let (p, v) = segs((300, 600), 0.8);
        let mut scene = Scene::new(v, p.folder);
        while let Some(seg) = scene.current().copied() {
            let pose = scene.pose();
            assert_eq!(pose.visible, seg.name != Name::Hidden);
            assert_eq!(pose.carrying, seg.name == Name::Return);
            let done = scene.t > 0.3;
            scene.tick(0.02, done);
        }
    }

    #[test]
    fn faces_the_folder_going_out_and_the_origin_coming_back() {
        let settle = |v: &[Seg]| v.iter().find(|s| s.name == Name::Settle).unwrap().mirrored;
        let (_, v) = segs((1600, 600), 0.8); // folder on the left
        assert!(v[0].mirrored && !settle(&v));
        let (_, v) = segs((300, 600), 0.8); // folder on the right
        assert!(!v[0].mirrored && settle(&v));
    }

    #[test]
    fn walking_eases_in_and_out_and_never_overshoots() {
        let (p, v) = segs((300, 600), 0.8);
        let mut scene = Scene::new(v, p.folder);
        let (mut prev, mut max_step, mut first_step) = (scene.pose().cat.0, 0, None);
        while scene.current().map(|s| s.name) == Some(Name::Approach) {
            scene.tick(0.016, false);
            let x = scene.pose().cat.0;
            let step = x - prev;
            assert!(step >= 0, "walked backwards");
            first_step.get_or_insert(step);
            max_step = max_step.max(step);
            prev = x;
        }
        assert!(first_step.unwrap() < max_step, "no ease-in");
        assert!(prev as f64 <= p.at_folder_x as f64 - JUMP + 1.0);
    }

    #[test]
    fn the_leap_fades_out_and_the_peek_rises_from_below_the_folder() {
        let (p, v) = segs((300, 600), 0.8);
        let mut scene = Scene::new(v, p.folder);
        let (mut min_alpha, mut rise_start, mut rise_end) = (1.0f32, None, None);
        while let Some(seg) = scene.current().copied() {
            let pose = scene.pose();
            if seg.name == Name::Leap {
                min_alpha = min_alpha.min(pose.alpha);
            }
            if seg.name == Name::Peek {
                rise_start.get_or_insert(pose.cat.1);
                rise_end = Some(pose.cat.1);
                assert_eq!(pose.clip_y, Some(p.folder.1 + FOLDER_H));
            }
            let done = scene.t > 1.0;
            scene.tick(0.016, done);
        }
        assert!(
            min_alpha < 0.2,
            "the cat should be nearly gone: {min_alpha}"
        );
        assert!(rise_start.unwrap() > rise_end.unwrap() + 100);
    }

    #[test]
    fn the_file_is_carried_then_dropped_in_front_of_the_sitting_cat() {
        for origin in [(300, 600), (1600, 600)] {
            let (p, v) = segs(origin, 0.8);
            let facing = if v.iter().any(|s| s.name == Name::Settle && s.mirrored) {
                -1
            } else {
                1
            };
            let mut scene = Scene::new(v, p.folder);
            let (mut first, mut last) = (None, None);
            while let Some(seg) = scene.current().copied() {
                let pose = scene.pose();
                // On the desktop only while sitting; in the mouth (drawn by the cat) before that.
                assert_eq!(
                    pose.file.is_some(),
                    seg.name == Name::Settle,
                    "{:?}",
                    seg.name
                );
                if let Some(f) = pose.file {
                    first.get_or_insert(f);
                    last = Some(f);
                }
                let done = scene.t > 0.3;
                scene.tick(0.016, done);
            }
            let (first, last) = (first.unwrap(), last.unwrap());
            assert!(last.1 > first.1 + 40, "it falls");
            assert!(
                (last.0 - (origin.0 + 128)) * facing > 40,
                "it lands ahead of the cat, not under it"
            );
        }
    }

    #[test]
    fn a_dropped_file_bounces_and_settles_at_rest() {
        let ys: Vec<f64> = (0..=20).map(|i| bounce(i as f64 / 20.0)).collect();
        assert_eq!(ys[0], 0.0);
        assert!((ys[20] - 1.0).abs() < 1e-9);
        assert!(ys.windows(2).any(|w| w[1] < w[0]), "no rebound");
        assert!(ys.iter().all(|&y| (0.0..=1.0).contains(&y)));
    }

    #[test]
    fn a_clip_that_never_finishes_cannot_hang_the_scene() {
        let (p, v) = segs((300, 600), 0.8);
        let mut scene = Scene::new(v, p.folder);
        for _ in 0..4000 {
            scene.tick(0.016, false);
        }
        assert!(scene.is_done());
    }

    #[test]
    fn a_cat_already_beside_the_folder_skips_the_walk() {
        let p = plan((300, 600), LEFT, RIGHT);
        let close = Plan {
            origin: (p.at_folder_x - 100, 600),
            ..p
        };
        let v = build(close, 0.8, HOME, MOUTH);
        assert_eq!(v[0].name, Name::Leap);
    }
}
