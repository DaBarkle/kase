//! The live case card: what the 3D case shows, moved toward what is really happening.
//!
//! The AirPods' case state (lid, each pod in or out, see `core::case_state`) is the
//! target. This keeps the pose on screen and moves it toward the target one physical step
//! at a time, with pre-rendered clips:
//!
//! * The lid opens before any pod moves and closes only once the pods are where they
//!   belong; pods never move while the lid does.
//! * A pod whose target flips back mid-move plays its clip backwards from where it is, so
//!   nothing jumps and nothing moves the wrong way.
//! * A pod change has to hold for `SETTLE` before it plays, and two pods that change
//!   together start `STAGGER` apart.
//! * An unknown pod or lid keeps what is shown. While the card is hidden the clock
//!   stops; hidden longer than `SNAP_AFTER`, the pose jumps straight to the truth.
//!
//! The card draws three layers that line up pixel for pixel: the case (lid and seated
//! pods baked in) and one layer per pod that is out or moving. Pure logic; the frames are
//! played by `anim::LiveArt`.

use crate::core::case_state::{CaseState, Lid, Pod};

pub const FPS: f32 = 144.0;
/// A pod change must hold this long before it plays.
pub const SETTLE: f32 = 0.12;
/// Two pods that change together start this far apart (as on the iPhone's case card).
pub const STAGGER: f32 = 0.08;
/// Hidden this long, the pose jumps to the truth instead of animating old news.
pub const SNAP_AFTER: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    L,
    R,
}

/// Which pods are seated in the case layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Seated {
    Both,
    L,
    R,
    None,
}

impl Seated {
    fn of(l: bool, r: bool) -> Self {
        match (l, r) {
            (true, true) => Seated::Both,
            (true, false) => Seated::L,
            (false, true) => Seated::R,
            (false, false) => Seated::None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Seated::Both => "both",
            Seated::L => "L",
            Seated::R => "R",
            Seated::None => "none",
        }
    }
}

/// A rendered clip of the live card (folder `airpods4-live/<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveClip {
    LidOpen(Seated),
    LidClose(Seated),
    /// First connect of the day: the case turns, then the lid opens over both pods.
    SpinLidOpen,
    /// A still of the open case.
    Case(Seated),
    Rise(Side),
    Sink(Side),
}

impl LiveClip {
    pub fn name(self) -> String {
        let side = |s: Side| if s == Side::L { "L" } else { "R" };
        match self {
            LiveClip::LidOpen(s) => format!("lid-open-{}", s.name()),
            LiveClip::LidClose(s) => format!("lid-close-{}", s.name()),
            LiveClip::SpinLidOpen => "spin-lid-open".into(),
            LiveClip::Case(s) => format!("case-{}", s.name()),
            LiveClip::Rise(s) => format!("rise-{}", side(s)),
            LiveClip::Sink(s) => format!("sink-{}", side(s)),
        }
    }

    /// Every clip the live card can ask for; all must be installed for live mode.
    pub fn all() -> Vec<LiveClip> {
        let mut v = vec![LiveClip::SpinLidOpen];
        for s in [Seated::Both, Seated::L, Seated::R] {
            v.push(LiveClip::LidOpen(s));
            v.push(LiveClip::LidClose(s));
        }
        for s in [Seated::Both, Seated::L, Seated::R, Seated::None] {
            v.push(LiveClip::Case(s));
        }
        for s in [Side::L, Side::R] {
            v.push(LiveClip::Rise(s));
            v.push(LiveClip::Sink(s));
        }
        v
    }
}

/// One layer's picture right now: a frame of a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shot {
    pub clip: LiveClip,
    pub frame: usize,
    /// The segment this frame belongs to: played from `from`, backwards if `reverse`
    /// (a held frame is a segment of its own).
    pub from: usize,
    pub reverse: bool,
}

impl Shot {
    fn hold(clip: LiveClip, frame: usize) -> Self {
        Self {
            clip,
            frame,
            from: frame,
            reverse: false,
        }
    }

    fn of(m: &Move, frame: usize) -> Self {
        Self {
            clip: m.clip,
            frame,
            from: m.from,
            reverse: m.reverse,
        }
    }

    /// Frames into the segment.
    pub fn pos(&self) -> usize {
        self.frame.abs_diff(self.from)
    }
}

/// A clip playing on one layer, from `from`, forwards or backwards.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Move {
    clip: LiveClip,
    from: usize,
    reverse: bool,
    start: f32,
}

impl Move {
    fn frame(&self, now: f32, len: usize) -> usize {
        let n = ((now - self.start).max(0.0) * FPS) as usize;
        let last = len.saturating_sub(1);
        if self.reverse {
            self.from.saturating_sub(n)
        } else {
            (self.from + n).min(last)
        }
    }

    fn done(&self, now: f32, len: usize) -> bool {
        let f = self.frame(now, len);
        if self.reverse {
            f == 0
        } else {
            f >= len.saturating_sub(1)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LidShow {
    Closed,
    Open,
    Moving { m: Move, opening: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PodShow {
    Seated,
    Out,
    /// `rising`: heading out (a Rise played forwards or a Sink backwards).
    Moving {
        m: Move,
        rising: bool,
    },
}

/// The live case card's pose. `lens` gives each clip's frame count.
pub struct CasePose<F: Fn(LiveClip) -> usize> {
    lens: F,
    clock: f32,
    hidden_for: f32,
    lid: LidShow,
    pods: [PodShow; 2],
    target: CaseState,
    /// When each pod's current target was first seen (for SETTLE).
    since: [f32; 2],
    last_start: f32,
    /// The next lid opening from a fully seated case spins first.
    pub spin_next: bool,
    reduce_motion: bool,
}

impl<F: Fn(LiveClip) -> usize> CasePose<F> {
    /// Starts showing `truth` as it is (no animation).
    pub fn new(lens: F, truth: CaseState) -> Self {
        let mut p = Self {
            lens,
            clock: 0.0,
            hidden_for: 0.0,
            lid: LidShow::Closed,
            pods: [PodShow::Seated; 2],
            target: truth,
            since: [0.0; 2],
            last_start: f32::MIN,
            spin_next: false,
            reduce_motion: false,
        };
        p.snap();
        p
    }

    pub fn set_reduce_motion(&mut self, on: bool) {
        self.reduce_motion = on;
    }

    /// Takes the latest truth. A change restarts the hidden timer, so news that arrives
    /// while the card is still opening is animated, not skipped.
    pub fn set_target(&mut self, truth: CaseState) {
        if (truth.lid, truth.left, truth.right)
            != (self.target.lid, self.target.left, self.target.right)
        {
            self.hidden_for = 0.0;
        }
        for (i, (old, new)) in [
            (self.target.left, truth.left),
            (self.target.right, truth.right),
        ]
        .into_iter()
        .enumerate()
        {
            if old != new {
                self.since[i] = self.clock;
            }
        }
        self.target = truth;
    }

    /// Jumps to the truth, keeping what is unknown.
    pub fn snap(&mut self) {
        let t = self.target;
        for (i, want) in [t.left, t.right].into_iter().enumerate() {
            self.pods[i] = match (want, self.pods[i]) {
                (Pod::Seated, _) => PodShow::Seated,
                (Pod::Out, _) => PodShow::Out,
                (Pod::Unknown, PodShow::Moving { rising, .. }) => {
                    if rising {
                        PodShow::Out
                    } else {
                        PodShow::Seated
                    }
                }
                (Pod::Unknown, p) => p,
            };
        }
        self.lid = match (t.lid, self.lid) {
            (Lid::Open, _) => LidShow::Open,
            (Lid::Closed, _) => LidShow::Closed,
            (Lid::Unknown, LidShow::Moving { opening, .. }) => {
                if opening {
                    LidShow::Open
                } else {
                    LidShow::Closed
                }
            }
            (Lid::Unknown, l) => l,
        };
        // Pods that aren't seated can only be out of an open case (unless the lid shut
        // after they came out, which the truth says).
        if t.lid == Lid::Unknown
            && self.lid == LidShow::Closed
            && self.pods.contains(&PodShow::Out)
            && self.target_pods().contains(&Pod::Seated)
        {
            self.lid = LidShow::Open;
        }
    }

    fn target_pods(&self) -> [Pod; 2] {
        [self.target.left, self.target.right]
    }

    fn len(&self, c: LiveClip) -> usize {
        (self.lens)(c).max(1)
    }

    /// Advances `dt` seconds. The clock only runs while the card is `visible`.
    pub fn tick(&mut self, dt: f32, visible: bool) {
        if !visible {
            self.hidden_for += dt;
            if self.hidden_for >= SNAP_AFTER {
                self.snap();
            }
            return;
        }
        self.hidden_for = 0.0;
        self.clock += dt;
        if self.reduce_motion {
            self.snap();
            return;
        }
        self.finish_moves();
        self.plan();
    }

    fn finish_moves(&mut self) {
        let now = self.clock;
        if let LidShow::Moving { m, opening } = self.lid
            && m.done(now, self.len(m.clip))
        {
            self.lid = if opening {
                LidShow::Open
            } else {
                LidShow::Closed
            };
        }
        for i in 0..2 {
            if let PodShow::Moving { m, rising } = self.pods[i]
                && m.done(now, self.len(m.clip))
            {
                self.pods[i] = if rising {
                    PodShow::Out
                } else {
                    PodShow::Seated
                };
            }
        }
    }

    fn seated_now(&self) -> Seated {
        Seated::of(
            self.pods[0] == PodShow::Seated,
            self.pods[1] == PodShow::Seated,
        )
    }

    fn pod_wants(&self, i: usize) -> Option<bool> {
        // Some(true): should be out; Some(false): should be seated.
        match self.target_pods()[i] {
            Pod::Unknown => None,
            Pod::Out => Some(true),
            Pod::Seated => Some(false),
        }
    }

    fn pod_settled(&self, i: usize) -> bool {
        match self.pods[i] {
            PodShow::Seated => self.pod_wants(i) != Some(true),
            PodShow::Out => self.pod_wants(i) != Some(false),
            PodShow::Moving { .. } => false,
        }
    }

    fn plan(&mut self) {
        let now = self.clock;
        if let LidShow::Moving { .. } = self.lid {
            return;
        }
        let pods_to_move: Vec<usize> = (0..2)
            .filter(|&i| match (self.pods[i], self.pod_wants(i)) {
                (PodShow::Seated, Some(true)) | (PodShow::Out, Some(false)) => true,
                (PodShow::Moving { rising, .. }, Some(out)) => rising != out,
                _ => false,
            })
            .collect();
        let pods_moving = self
            .pods
            .iter()
            .any(|p| matches!(p, PodShow::Moving { .. }));
        match self.lid {
            LidShow::Closed => {
                // Pods can only move with the lid open: if they have to (a missed lid
                // event), open it, move them, and close it again if the truth says so.
                let opens = self.target.lid == Lid::Open || !pods_to_move.is_empty();
                if opens {
                    self.open_lid(now);
                }
                return;
            }
            LidShow::Open => {}
            LidShow::Moving { .. } => return,
        }
        // Lid open: pods first, then the lid may close.
        for i in pods_to_move {
            if now - self.since[i] < SETTLE {
                continue;
            }
            if let PodShow::Moving { m, rising } = self.pods[i] {
                // Changed its mind mid-move: play back from here.
                let f = m.frame(now, self.len(m.clip));
                self.pods[i] = PodShow::Moving {
                    m: Move {
                        clip: m.clip,
                        from: f,
                        reverse: !m.reverse,
                        start: now,
                    },
                    rising: !rising,
                };
                continue;
            }
            if now - self.last_start < STAGGER {
                continue;
            }
            let side = if i == 0 { Side::L } else { Side::R };
            let rising = self.pods[i] == PodShow::Seated;
            let clip = if rising {
                LiveClip::Rise(side)
            } else {
                LiveClip::Sink(side)
            };
            self.pods[i] = PodShow::Moving {
                m: Move {
                    clip,
                    from: 0,
                    reverse: false,
                    start: now,
                },
                rising,
            };
            self.last_start = now;
        }
        if self.target.lid == Lid::Closed && !pods_moving && (0..2).all(|i| self.pod_settled(i)) {
            match self.seated_now() {
                Seated::None => self.lid = LidShow::Closed,
                s => {
                    self.lid = LidShow::Moving {
                        m: Move {
                            clip: LiveClip::LidClose(s),
                            from: 0,
                            reverse: false,
                            start: now,
                        },
                        opening: false,
                    };
                }
            }
        }
    }

    fn open_lid(&mut self, now: f32) {
        let s = self.seated_now();
        let clip = match s {
            Seated::None => {
                self.lid = LidShow::Open;
                return;
            }
            Seated::Both if self.spin_next => {
                self.spin_next = false;
                LiveClip::SpinLidOpen
            }
            s => LiveClip::LidOpen(s),
        };
        self.lid = LidShow::Moving {
            m: Move {
                clip,
                from: 0,
                reverse: false,
                start: now,
            },
            opening: true,
        };
    }

    /// The three layers to draw, bottom first: case, left pod, right pod.
    pub fn layers(&self) -> [Option<Shot>; 3] {
        let now = self.clock;
        let case = match self.lid {
            LidShow::Moving { m, .. } => Shot::of(&m, m.frame(now, self.len(m.clip))),
            LidShow::Open => Shot::hold(LiveClip::Case(self.seated_now()), 0),
            LidShow::Closed => {
                let c = LiveClip::LidClose(Seated::Both);
                Shot::hold(c, self.len(c) - 1)
            }
        };
        let pod = |i: usize| -> Option<Shot> {
            let side = if i == 0 { Side::L } else { Side::R };
            match self.pods[i] {
                PodShow::Seated => None,
                PodShow::Out => {
                    let c = LiveClip::Rise(side);
                    Some(Shot::hold(c, self.len(c) - 1))
                }
                PodShow::Moving { m, .. } => Some(Shot::of(&m, m.frame(now, self.len(m.clip)))),
            }
        };
        [Some(case), pod(0), pod(1)]
    }

    /// Something is moving (frames are needed at the display's rate) or waiting to.
    pub fn moving(&self) -> bool {
        matches!(self.lid, LidShow::Moving { .. })
            || self
                .pods
                .iter()
                .any(|p| matches!(p, PodShow::Moving { .. }))
            || self.pending()
    }

    /// A change is known but hasn't started (settling, staggered, or behind the lid).
    pub fn pending(&self) -> bool {
        let lid_wants = matches!(
            (self.lid, self.target.lid),
            (LidShow::Closed, Lid::Open) | (LidShow::Open, Lid::Closed)
        );
        lid_wants
            || (0..2)
                .any(|i| !self.pod_settled(i) && !matches!(self.pods[i], PodShow::Moving { .. }))
    }

    /// The pose shown, as a case state (for tests and logs).
    pub fn shown(&self) -> (Lid, Pod, Pod) {
        let lid = match self.lid {
            LidShow::Open => Lid::Open,
            LidShow::Closed => Lid::Closed,
            LidShow::Moving { .. } => Lid::Unknown,
        };
        let pod = |p: PodShow| match p {
            PodShow::Seated => Pod::Seated,
            PodShow::Out => Pod::Out,
            PodShow::Moving { .. } => Pod::Unknown,
        };
        (lid, pod(self.pods[0]), pod(self.pods[1]))
    }

    #[cfg(test)]
    fn clock(&self) -> f32 {
        self.clock
    }
}

#[cfg(test)]
#[path = "case_pose_tests.rs"]
mod tests;
