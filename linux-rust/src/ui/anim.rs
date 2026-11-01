//! The AirPods 4 animations: pre-rendered WebP clips, played by elapsed time.
//!
//! Each clip is a folder of frames (`airpods4-<clip>/f000.webp`..). Which moment plays
//! which clip is in `docs/JOURNEYS.md`; a clip whose frames aren't installed falls back
//! to the case-open clip.
//!
//! Frames are read and decoded on a background thread a little ahead of playback (a
//! bounded queue), so only a few dozen are ever held decoded. Dropping the player stops
//! the thread and frees everything. Frames rendered from "AirPods 4" by Falah3D,
//! CC BY 4.0 (see README).

use iced::widget::image::Handle;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const FPS: f32 = 144.0;

/// What the art shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    /// The lid opens and the pods lift out (connected, live).
    Open,
    /// First connect of the day: the case turns 360 degrees, opens, the pods spiral out.
    SpinOpen,
    /// Both pods drop back in and the lid closes.
    PutAway,
    /// A still of the closed case (put away, out of range): the open clip's first frame.
    Closed,
}

impl Clip {
    fn folder(self) -> &'static str {
        match self {
            Clip::Open | Clip::Closed => "airpods4-open",
            Clip::SpinOpen => "airpods4-spin-open",
            Clip::PutAway => "airpods4-put-away",
        }
    }
}

/// Frames in `clip` (0 when not installed), counted once.
fn clip_len(clip: Clip) -> usize {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static LENS: OnceLock<Mutex<HashMap<Clip, usize>>> = OnceLock::new();
    let lens = LENS.get_or_init(Default::default);
    if let Some(n) = lens.lock().ok().and_then(|m| m.get(&clip).copied()) {
        return n;
    }
    let n = dir()
        .and_then(|d| d.parent())
        .map(|base| {
            let folder = base.join(clip.folder());
            (0..2000)
                .take_while(|i| folder.join(format!("f{i:03}.webp")).is_file())
                .count()
        })
        .unwrap_or(0);
    if let Ok(mut m) = lens.lock() {
        m.insert(clip, n);
    }
    n
}

/// The clip to play for `clip`: itself when installed, else the case-open clip.
fn resolve(clip: Clip) -> (PathBuf, usize, Clip) {
    let base = dir().and_then(|d| d.parent()).map(PathBuf::from);
    match (base, clip_len(clip)) {
        (Some(b), n) if n > 0 => (b.join(clip.folder()), n, clip),
        _ => (
            dir().cloned().unwrap_or_default(),
            clip_len(Clip::Open),
            Clip::Open,
        ),
    }
}
/// Frames decoded ahead of playback (each 440x440 RGBA is about 0.8 MB). A decode takes
/// a few ms, so 8 frames (55 ms) is plenty of lead at 144 fps.
const AHEAD: usize = 8;
/// How long the frames stay decoded after the art is hidden.
pub const LINGER: Duration = Duration::from_secs(5);

/// Whether these AirPods get the animation (AirPods 4 and AirPods 4 with ANC).
pub fn fits(kind: &str) -> bool {
    kind.starts_with("airpods4")
}

/// The case-open clip's folder (the one always installed), found by its first frame.
/// Clips are counted from their files, so a re-render with a different length just works.
pub fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        [
            std::env::var_os("KASE_ANIM_DIR").map(PathBuf::from),
            Some(PathBuf::from("/app/share/kase/anim/airpods4-open")),
            Some(PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/anim/airpods4-open"
            ))),
        ]
        .into_iter()
        .flatten()
        .find(|d| d.join("f000.webp").is_file())
    })
    .as_ref()
}

/// Which frame of a `count`-frame clip to show `elapsed` after the start: plays once,
/// then holds the last. `still` (Reduce motion) shows the last frame at once.
pub fn frame_at(elapsed: Duration, still: bool) -> usize {
    clip_frame_at(elapsed, still, clip_len(Clip::Open))
}

fn clip_frame_at(elapsed: Duration, still: bool, count: usize) -> usize {
    let last = count.saturating_sub(1);
    if still {
        return last;
    }
    ((elapsed.as_secs_f32() * FPS) as usize).min(last)
}

pub struct Player {
    start: Instant,
    still: bool,
    /// Positions in the playback (frames from `from`, in `reverse` or forwards).
    count: usize,
    /// Only this frame is shown (the closed still).
    hold: Option<usize>,
    rx: Receiver<(usize, Handle)>,
    /// Decoded but not yet due.
    next: Option<(usize, Handle)>,
    shown: Option<(usize, Handle)>,
    want: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    /// The decode thread is gone and every frame it sent has been shown.
    done: bool,
}

impl Player {
    /// Starts playing now. `still` (Reduce motion) decodes and shows only the last frame.
    /// None when the frames aren't installed.
    pub fn start(still: bool) -> Option<Self> {
        Self::play(Clip::Open, still)
    }

    /// Starts `clip` now (falls back to the case-open clip when it isn't installed).
    pub fn play(clip: Clip, still: bool) -> Option<Self> {
        dir()?;
        let (dir, count, clip) = resolve(clip);
        let hold = (clip == Clip::Closed).then_some(0);
        Self::spawn(dir, count, 0, false, hold, still)
    }

    /// Plays the frames of `dir` from frame `from`, forwards to the last of `len` or
    /// backwards to the first, as `advance_to` asks. The live case card drives one per layer.
    pub fn segment(dir: PathBuf, len: usize, from: usize, reverse: bool) -> Option<Self> {
        let from = from.min(len.checked_sub(1)?);
        let count = if reverse { from + 1 } else { len - from };
        Self::spawn(dir, count, from, reverse, None, false)
    }

    fn spawn(
        dir: PathBuf,
        count: usize,
        from: usize,
        reverse: bool,
        hold: Option<usize>,
        still: bool,
    ) -> Option<Self> {
        let still = still || hold.is_some();
        let first = hold.unwrap_or(if still { count - 1 } else { 0 });
        let frame_of = move |pos: usize| {
            if reverse {
                from.saturating_sub(pos)
            } else {
                from + pos
            }
        };
        let (tx, rx) = sync_channel(AHEAD);
        let want = Arc::new(AtomicUsize::new(first));
        let stop = Arc::new(AtomicBool::new(false));
        let (w, s) = (want.clone(), stop.clone());
        std::thread::Builder::new()
            .name("kase-anim".into())
            .spawn(move || {
                let mut i = first;
                let end = if hold.is_some() { first + 1 } else { count };
                while i < end && !s.load(Ordering::Relaxed) {
                    // Behind playback: skip to what is due instead of decoding stale frames.
                    i = i.max(w.load(Ordering::Relaxed));
                    let file = dir.join(format!("f{:03}.webp", frame_of(i)));
                    let Some(h) = decode(&file) else {
                        log::warn!(
                            "animation frame {} failed to decode; stopping",
                            file.display()
                        );
                        return;
                    };
                    // Blocks while AHEAD frames wait; errors once the player is dropped.
                    if tx.send((i, h)).is_err() {
                        return;
                    }
                    i += 1;
                }
            })
            .ok()?;
        Some(Self {
            start: crate::testclock::now(),
            still,
            count,
            hold,
            rx,
            next: None,
            shown: None,
            want,
            stop,
            done: false,
        })
    }

    /// Moves to the newest decoded frame that is due at `now`. Holds the last one shown
    /// when decoding falls behind.
    pub fn advance(&mut self, now: Instant) {
        let due = match self.hold {
            Some(f) => f,
            None => clip_frame_at(
                now.saturating_duration_since(self.start),
                self.still,
                self.count,
            ),
        };
        self.advance_to(due);
    }

    /// Moves to the newest decoded position up to `due` (positions count from the
    /// segment's first frame).
    pub fn advance_to(&mut self, due: usize) {
        let due = due.min(self.count.saturating_sub(1));
        self.want.store(due, Ordering::Relaxed);
        loop {
            let f = match self.next.take() {
                Some(f) => f,
                // On the test clock, wait for the due frame so every run shows the same one.
                None if crate::testclock::stepping()
                    && self.shown.as_ref().is_none_or(|(i, _)| *i < due) =>
                {
                    match self.rx.recv_timeout(Duration::from_secs(2)) {
                        Ok(f) => f,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            self.done = true;
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    }
                }
                None => match self.rx.try_recv() {
                    Ok(f) => f,
                    // After the last frame, or a frame that failed to decode.
                    Err(TryRecvError::Disconnected) => {
                        self.done = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                },
            };
            if f.0 > due {
                self.next = Some(f);
                break;
            }
            self.shown = Some(f);
        }
    }

    /// The frame to draw (None until the first one is decoded).
    pub fn current(&self) -> Option<&Handle> {
        self.shown.as_ref().map(|(_, h)| h)
    }

    /// Still moving: frames are needed at the display's rate.
    pub fn playing(&self) -> bool {
        !self.done
    }
}

/// One place the animation shows: plays it each time the place starts showing it,
/// holds the last frame, and frees the frames `LINGER` after it stops.
#[derive(Default)]
pub struct Art {
    player: Option<Player>,
    want: bool,
    clip: Option<Clip>,
    hidden: Option<Instant>,
}

impl Art {
    /// Call on every tick with whether the art is on screen (the case-open clip).
    pub fn sync(&mut self, want: bool, still: bool, now: Instant) {
        self.show(want.then_some(Clip::Open), still, now);
    }

    /// Call on every tick with the clip on screen, if any. A new clip starts from its
    /// first frame; the same clip keeps playing (or holding its last frame).
    pub fn show(&mut self, clip: Option<Clip>, still: bool, now: Instant) {
        let want = clip.is_some();
        if let Some(c) = clip
            && (!self.want || self.clip != Some(c))
        {
            self.player = Player::play(c, still);
            self.clip = Some(c);
        }
        if want {
            self.hidden = None;
        } else if self.want {
            self.hidden = Some(now);
        }
        self.want = want;
        if self.drop_at().is_some_and(|t| now >= t) {
            self.player = None;
            self.hidden = None;
        }
        if let Some(p) = &mut self.player {
            p.advance(now);
        }
    }

    /// When the frames will be freed, if the art was hidden and they still are held.
    pub fn drop_at(&self) -> Option<Instant> {
        self.hidden
            .filter(|_| self.player.is_some())
            .map(|t| t + LINGER)
    }

    pub fn current(&self) -> Option<&Handle> {
        self.player.as_ref().and_then(Player::current)
    }

    /// Frames are needed at the display's rate.
    pub fn playing(&self) -> bool {
        self.player.as_ref().is_some_and(Player::playing)
    }
}

// --- The live case card ---------------------------------------------------------

use crate::core::case_state::CaseState;
use crate::ui::case_pose::{CasePose, LiveClip, Shot};

/// Where the live card's clips are: `airpods4-live/<clip>/f000.webp`, next to the others.
fn live_base() -> Option<PathBuf> {
    let base = dir()?.parent()?.join("airpods4-live");
    base.is_dir().then_some(base)
}

/// Frames in a live clip (0 when not installed), counted once.
pub fn live_len(clip: LiveClip) -> usize {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static LENS: OnceLock<Mutex<HashMap<LiveClip, usize>>> = OnceLock::new();
    let lens = LENS.get_or_init(Default::default);
    if let Some(n) = lens.lock().ok().and_then(|m| m.get(&clip).copied()) {
        return n;
    }
    let n = live_base()
        .map(|b| {
            let folder = b.join(clip.name());
            (0..2000)
                .take_while(|i| folder.join(format!("f{i:03}.webp")).is_file())
                .count()
        })
        .unwrap_or(0);
    if let Ok(mut m) = lens.lock() {
        m.insert(clip, n);
    }
    n
}

/// Every live clip is installed, so the card can follow the case live.
pub fn live_ready() -> bool {
    static READY: OnceLock<bool> = OnceLock::new();
    *READY.get_or_init(|| LiveClip::all().into_iter().all(|c| live_len(c) > 0))
}

struct LayerPlay {
    key: (LiveClip, usize, bool),
    player: Player,
}

impl LayerPlay {
    fn new(shot: &Shot) -> Option<Self> {
        let dir = live_base()?.join(shot.clip.name());
        let player = Player::segment(dir, live_len(shot.clip), shot.from, shot.reverse)?;
        Some(Self {
            key: (shot.clip, shot.from, shot.reverse),
            player,
        })
    }
}

/// The pose machine, with clip lengths read from the installed art.
type LivePose = CasePose<fn(LiveClip) -> usize>;

/// The live case card's three layers (case, left pod, right pod), played from the
/// pose in `case_pose`. A change of clip on any layer is swapped in only once every
/// layer's new frame is decoded, so the layers never disagree on screen (a pod leaving
/// the case layer and appearing on its own layer happen in the same frame).
#[derive(Default)]
pub struct LiveArt {
    pose: Option<LivePose>,
    shown: [Option<LayerPlay>; 3],
    /// New segments waiting for their first frame: Some(None) means "remove this layer".
    next: [Option<Option<LayerPlay>>; 3],
    last: Option<Instant>,
    hidden: Option<Instant>,
    spin_seen: Option<Instant>,
}

impl LiveArt {
    /// Call on every tick. `truth` is the AirPods' latest case state; `visible` whether
    /// the art is on screen; `spin_at` when the overlay last chose the first-of-the-day
    /// spin (a new value makes the next lid opening spin).
    pub fn sync(
        &mut self,
        truth: Option<CaseState>,
        visible: bool,
        reduce_motion: bool,
        spin_at: Option<Instant>,
        now: Instant,
    ) {
        // Hidden, the whole gap counts (the overlay may not tick for an hour); on screen,
        // a long gap (a stall) is not played as a jump.
        let gap = self
            .last
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        let dt = if visible { gap.min(0.1) } else { gap };
        self.last = Some(now);
        if let Some(pose) = &mut self.pose {
            // Time passes on what was known before this news, so old news is skipped
            // while hidden but this news still animates when the card opens for it.
            pose.set_reduce_motion(reduce_motion);
            pose.tick(dt, visible);
        }
        if let Some(truth) = truth {
            let pose = self.pose.get_or_insert_with(|| {
                // Start from the case shut, so the first card shows it opening.
                let start = CaseState {
                    lid: crate::core::case_state::Lid::Closed,
                    ..truth
                };
                CasePose::new(live_len as fn(LiveClip) -> usize, start)
            });
            pose.set_target(truth);
        }
        let Some(pose) = &mut self.pose else { return };
        if spin_at.is_some() && spin_at != self.spin_seen {
            self.spin_seen = spin_at;
            pose.spin_next = true;
        }
        if !visible {
            let since = *self.hidden.get_or_insert(now);
            if now.saturating_duration_since(since) >= LINGER {
                self.shown = Default::default();
                self.next = Default::default();
            }
            return;
        }
        self.hidden = None;
        let want = pose.layers();
        for (i, w) in want.iter().enumerate() {
            let key = w.map(|s| (s.clip, s.from, s.reverse));
            let shown_key = self.shown[i].as_ref().map(|l| l.key);
            let next_key = self.next[i].as_ref().map(|n| n.as_ref().map(|l| l.key));
            if key == shown_key {
                self.next[i] = None;
            } else if next_key != Some(key) {
                self.next[i] = Some(w.as_ref().and_then(LayerPlay::new));
            }
        }
        for (i, w) in want.iter().enumerate() {
            let Some(w) = w else { continue };
            let pos = w.pos();
            if let Some(Some(n)) = &mut self.next[i] {
                n.player.advance_to(pos);
            } else if let Some(l) = &mut self.shown[i] {
                l.player.advance_to(pos);
            }
        }
        let ready = self
            .next
            .iter()
            .all(|n| !matches!(n, Some(Some(l)) if l.player.current().is_none()));
        if ready || self.shown.iter().all(Option::is_none) {
            for i in 0..3 {
                if let Some(n) = self.next[i].take() {
                    self.shown[i] = n;
                }
            }
        }
    }

    /// The layers to draw, bottom first.
    pub fn current(&self) -> [Option<&Handle>; 3] {
        let f = |i: usize| self.shown[i].as_ref().and_then(|l| l.player.current());
        [f(0), f(1), f(2)]
    }

    /// Frames are needed at the display's rate.
    pub fn playing(&self) -> bool {
        self.pose.as_ref().is_some_and(|p| p.moving())
            || self.next.iter().any(Option::is_some)
            || self
                .shown
                .iter()
                .flatten()
                .any(|l| l.player.current().is_none())
    }

    /// When the frames will be freed, if the art is hidden and still holds them.
    pub fn drop_at(&self) -> Option<Instant> {
        self.hidden
            .filter(|_| self.shown.iter().any(Option::is_some))
            .map(|t| t + LINGER)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn decode(path: &std::path::Path) -> Option<Handle> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    Some(Handle::from_rgba(w, h, img.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_once_then_holds() {
        assert_eq!(clip_frame_at(Duration::ZERO, false, 185), 0);
        assert_eq!(clip_frame_at(Duration::from_millis(500), false, 185), 72);
        assert_eq!(clip_frame_at(Duration::from_secs(60), false, 185), 184);
        assert_eq!(
            clip_frame_at(Duration::ZERO, true, 185),
            184,
            "reduce motion"
        );
    }

    #[test]
    fn every_clip_is_found_whatever_its_length() {
        // The frames are in the repo, so this always runs. 1.0.1 shipped with no 3D art
        // because the lookup expected the old clip's exact frame count.
        assert!(dir().is_some(), "case-open clip not found");
        for clip in [Clip::Open, Clip::SpinOpen, Clip::PutAway] {
            assert!(
                clip_len(clip) > 30,
                "{clip:?} has {} frames",
                clip_len(clip)
            );
            assert_eq!(resolve(clip).2, clip, "{clip:?} fell back");
        }
    }

    #[test]
    fn only_airpods_4() {
        assert!(fits("airpods4") && fits("airpods4anc"));
        assert!(!fits("airpods3") && !fits("airpods_pro2") && !fits("airpods_max"));
    }

    #[test]
    fn reduce_motion_shows_the_last_frame_then_stops() {
        let Some(mut p) = Player::start(true) else {
            return;
        };
        let until = Instant::now() + Duration::from_secs(5);
        while p.playing() && Instant::now() < until {
            p.advance(Instant::now());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!p.playing(), "never finished");
        assert_eq!(
            p.shown.as_ref().map(|f| f.0),
            Some(clip_len(Clip::Open) - 1)
        );
    }

    #[test]
    fn plays_through_to_the_last_frame() {
        let Some(mut p) = Player::start(false) else {
            return;
        };
        let until = Instant::now() + Duration::from_secs(10);
        while p.playing() && Instant::now() < until {
            p.advance(Instant::now());
            std::thread::sleep(Duration::from_millis(7));
        }
        assert_eq!(
            p.shown.as_ref().map(|f| f.0),
            Some(clip_len(Clip::Open) - 1)
        );
    }

    #[test]
    fn frames_are_contiguous() {
        let base = dir().and_then(|d| d.parent()).expect("clips in the repo");
        for clip in [Clip::Open, Clip::SpinOpen, Clip::PutAway] {
            let d = base.join(clip.folder());
            let files = std::fs::read_dir(&d).unwrap().count();
            assert_eq!(
                files,
                clip_len(clip),
                "{clip:?}: a gap in the frame numbers"
            );
        }
    }

    /// Every live clip is installed, without gaps, and as long as the pose machine's
    /// tests assume (the lid clips line up with each other, and sink starts where rise
    /// ends).
    #[test]
    fn live_clips_are_complete() {
        let base = live_base().expect("live clips in the repo");
        for clip in LiveClip::all() {
            let files = std::fs::read_dir(base.join(clip.name())).unwrap().count();
            assert!(files > 0, "{} missing", clip.name());
            assert_eq!(files, live_len(clip), "{}: a gap", clip.name());
        }
        assert!(live_ready());
        use crate::ui::case_pose::{Seated, Side};
        for s in [Seated::Both, Seated::L, Seated::R] {
            assert_eq!(
                live_len(LiveClip::LidOpen(s)),
                live_len(LiveClip::LidOpen(Seated::Both))
            );
            assert_eq!(
                live_len(LiveClip::LidClose(s)),
                live_len(LiveClip::LidClose(Seated::Both))
            );
            assert_eq!(live_len(LiveClip::Case(s)), 1);
        }
        assert_eq!(
            live_len(LiveClip::Rise(Side::L)),
            live_len(LiveClip::Rise(Side::R))
        );
        assert_eq!(
            live_len(LiveClip::Sink(Side::L)),
            live_len(LiveClip::Sink(Side::R))
        );
    }
}
