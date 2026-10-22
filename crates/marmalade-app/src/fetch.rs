//! The file-fetch scene as a pure timeline: no Win32, so it can be unit tested.
//!
//! The cat leaps into a folder, rummages (the folder shakes), pops up with the file in its mouth
//! and carries it back to where it started. `cat.rs` drives this and applies each `Pose`.

use crate::cat::W;

pub const FOLDER_W: i32 = 96;
pub const FOLDER_H: i32 = 80;
/// Distance between the cat's centre and the folder's centre at the start.
const GAP: i32 = 300;
/// Feet line of the sprite inside the cat window (matches `anchor.y` of the clips).
const FEET: i32 = 248;
/// The cat window is raised by this much while peeking so the clip's bottom edge hides behind the folder.
const PEEK_LIFT: i32 = 40;
const RUMMAGE_SECS: f64 = 0.7;
const WALK_PX_PER_SEC: f64 = 240.0;
/// Safety net if a clip is missing or never reports finished.
const PEEK_MAX_SECS: f64 = 4.0;
const TOTAL_MAX_SECS: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Leap,
    Rummage,
    Peek,
    Return,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Top-left of the cat window when the scene starts (and where it ends).
    pub origin: (i32, i32),
    /// +1 when the folder is to the right of the cat, -1 to the left.
    pub dir: i32,
    /// Top-left of the folder window.
    pub folder: (i32, i32),
    /// Left edge of the cat window when it is at the folder.
    pub at_folder_x: i32,
}

/// Put the folder on the side of the cat that has room, inside the horizontal work area.
pub fn plan(origin: (i32, i32), work_left: i32, work_right: i32) -> Plan {
    let center = origin.0 + W / 2;
    let room_right = work_right - (center + GAP + FOLDER_W / 2);
    let room_left = (center - GAP - FOLDER_W / 2) - work_left;
    let want = if room_right >= 0 || room_right >= room_left {
        1
    } else {
        -1
    };
    let lo = work_left + FOLDER_W / 2;
    let hi = (work_right - FOLDER_W / 2).max(lo);
    let folder_cx = (center + want * GAP).clamp(lo, hi);
    let dir = match folder_cx - center {
        d if d < 0 => -1,
        _ => 1,
    };
    Plan {
        origin,
        dir,
        folder: (folder_cx - FOLDER_W / 2, origin.1 + FEET + 4 - FOLDER_H),
        at_folder_x: folder_cx - W / 2,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pose {
    pub cat: (i32, i32),
    pub cat_visible: bool,
    pub mirrored: bool,
    pub carrying: bool,
    pub folder: (i32, i32),
    pub folder_visible: bool,
}

pub struct Scene {
    plan: Plan,
    phase: Phase,
    /// Seconds spent in the current phase.
    t: f64,
    total: f64,
    leap_secs: f64,
    /// Pixels walked back since the return began.
    travel: f64,
}

impl Scene {
    pub fn new(plan: Plan, leap_secs: f64) -> Self {
        Scene {
            plan,
            phase: Phase::Leap,
            t: 0.0,
            total: 0.0,
            leap_secs: leap_secs.clamp(0.2, 3.0),
            travel: 0.0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The sprite clip for the current phase (`None` while the cat is out of sight).
    pub fn clip(&self) -> Option<&'static str> {
        match self.phase {
            Phase::Leap => Some("leap"),
            Phase::Peek => Some("peek"),
            Phase::Return => Some("carry"),
            Phase::Rummage | Phase::Done => None,
        }
    }

    fn return_distance(&self) -> f64 {
        (self.plan.at_folder_x - self.plan.origin.0).abs() as f64
    }

    /// Advance by `dt` seconds. `clip_done` says whether the current one-shot clip has finished.
    /// At most one phase change per call, so the caller can start the next clip before the next tick.
    pub fn tick(&mut self, dt: f64, clip_done: bool) {
        if self.phase == Phase::Done {
            return;
        }
        self.t += dt;
        self.total += dt;
        if self.total > TOTAL_MAX_SECS {
            self.phase = Phase::Done;
            return;
        }
        match self.phase {
            Phase::Leap if self.t >= self.leap_secs => self.enter(Phase::Rummage),
            Phase::Rummage if self.t >= RUMMAGE_SECS => self.enter(Phase::Peek),
            Phase::Peek if clip_done || self.t >= PEEK_MAX_SECS => self.enter(Phase::Return),
            Phase::Return => {
                self.travel += WALK_PX_PER_SEC * dt;
                if self.travel >= self.return_distance() {
                    self.enter(Phase::Done);
                }
            }
            _ => {}
        }
    }

    fn enter(&mut self, next: Phase) {
        self.phase = next;
        self.t = 0.0;
    }

    pub fn pose(&self) -> Pose {
        let p = &self.plan;
        let out_mirrored = p.dir < 0;
        let mut pose = Pose {
            cat: p.origin,
            cat_visible: true,
            mirrored: out_mirrored,
            carrying: false,
            folder: p.folder,
            folder_visible: true,
        };
        match self.phase {
            Phase::Leap => {
                let k = (self.t / self.leap_secs).clamp(0.0, 1.0);
                let x = p.origin.0 as f64 + (p.at_folder_x - p.origin.0) as f64 * k;
                pose.cat = (x.round() as i32, p.origin.1);
            }
            Phase::Rummage => {
                pose.cat_visible = false;
                let dx = if (self.t * 12.5) as i64 % 2 == 0 {
                    -3
                } else {
                    3
                };
                pose.folder.0 += dx;
            }
            Phase::Peek => pose.cat = (p.at_folder_x, p.origin.1 - PEEK_LIFT),
            Phase::Return => {
                let walked = self.travel.min(self.return_distance());
                let x = p.at_folder_x as f64 - p.dir as f64 * walked;
                pose.cat = (x.round() as i32, p.origin.1);
                pose.mirrored = !out_mirrored;
                pose.carrying = true;
                pose.folder_visible = walked < (FOLDER_W + 20) as f64;
            }
            Phase::Done => {
                pose.folder_visible = false;
                pose.mirrored = !out_mirrored;
            }
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cat::H as CAT_H;

    impl Scene {
        fn is_done(&self) -> bool {
            self.phase == Phase::Done
        }
    }

    const LEFT: i32 = 0;
    const RIGHT: i32 = 1920;

    fn run_to_done(scene: &mut Scene) -> Vec<Phase> {
        let mut seen = vec![scene.phase()];
        for _ in 0..2000 {
            // The peek clip is reported finished as soon as it has run for a moment.
            let done = scene.phase() == Phase::Peek;
            scene.tick(0.016, done);
            if seen.last() != Some(&scene.phase()) {
                seen.push(scene.phase());
            }
            if scene.is_done() {
                break;
            }
        }
        seen
    }

    #[test]
    fn folder_goes_on_the_side_with_room() {
        let right = plan((100, 700), LEFT, RIGHT);
        assert_eq!(right.dir, 1);
        let left = plan((1600, 700), LEFT, RIGHT);
        assert_eq!(left.dir, -1);
        for p in [right, left] {
            assert!(p.folder.0 >= LEFT && p.folder.0 + FOLDER_W <= RIGHT);
        }
    }

    #[test]
    fn folder_stays_on_screen_in_a_narrow_work_area() {
        let p = plan((50, 700), 0, 400);
        assert!(p.folder.0 >= 0 && p.folder.0 + FOLDER_W <= 400);
    }

    #[test]
    fn visits_every_phase_in_order_and_ends_at_the_origin() {
        let mut scene = Scene::new(plan((300, 600), LEFT, RIGHT), 1.0);
        let seen = run_to_done(&mut scene);
        assert_eq!(
            seen,
            [
                Phase::Leap,
                Phase::Rummage,
                Phase::Peek,
                Phase::Return,
                Phase::Done
            ]
        );
        assert_eq!(scene.pose().cat, (300, 600));
        assert!(!scene.pose().folder_visible);
    }

    #[test]
    fn cat_is_hidden_only_while_rummaging_and_carries_only_on_the_way_back() {
        let mut scene = Scene::new(plan((300, 600), LEFT, RIGHT), 0.5);
        loop {
            let pose = scene.pose();
            assert_eq!(pose.cat_visible, scene.phase() != Phase::Rummage);
            assert_eq!(pose.carrying, scene.phase() == Phase::Return);
            if scene.is_done() {
                break;
            }
            scene.tick(0.02, scene.phase() == Phase::Peek);
        }
    }

    #[test]
    fn faces_the_folder_going_out_and_the_origin_coming_back() {
        let mut scene = Scene::new(plan((1600, 600), LEFT, RIGHT), 0.5); // folder on the left
        assert!(scene.pose().mirrored);
        while scene.phase() != Phase::Return {
            scene.tick(0.02, scene.phase() == Phase::Peek);
        }
        assert!(!scene.pose().mirrored);
    }

    #[test]
    fn a_clip_that_never_finishes_cannot_hang_the_scene() {
        let mut scene = Scene::new(plan((300, 600), LEFT, RIGHT), 1.0);
        for _ in 0..2000 {
            scene.tick(0.016, false);
        }
        assert!(scene.is_done());
    }

    #[test]
    fn peek_is_raised_so_the_folder_covers_the_clip_bottom() {
        let p = plan((300, 600), LEFT, RIGHT);
        let mut scene = Scene::new(p, 0.5);
        while scene.phase() != Phase::Peek {
            scene.tick(0.02, false);
        }
        let pose = scene.pose();
        let cat_bottom = pose.cat.1 + CAT_H;
        assert!(cat_bottom > p.folder.1 && cat_bottom < p.folder.1 + FOLDER_H);
    }
}
