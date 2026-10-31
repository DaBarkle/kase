//! The AirPods 4 case-open animation: pre-rendered WebP frames, played by elapsed time.
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
pub const COUNT: usize = 231;
/// Frames decoded ahead of playback (each 440x440 RGBA is about 0.8 MB). A decode takes
/// a few ms, so 8 frames (55 ms) is plenty of lead at 144 fps.
const AHEAD: usize = 8;
/// How long the frames stay decoded after the art is hidden.
pub const LINGER: Duration = Duration::from_secs(5);

/// Whether these AirPods get the animation (AirPods 4 and AirPods 4 with ANC).
pub fn fits(kind: &str) -> bool {
    kind.starts_with("airpods4")
}

/// The folder holding f000.webp.. when every frame is there.
pub fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let last = format!("f{:03}.webp", COUNT - 1);
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
        .find(|d| d.join(&last).is_file())
    })
    .as_ref()
}

/// Which frame to show `elapsed` after the start: plays once, then holds the last.
pub fn frame_at(elapsed: Duration, still: bool) -> usize {
    if still {
        return COUNT - 1;
    }
    ((elapsed.as_secs_f32() * FPS) as usize).min(COUNT - 1)
}

pub struct Player {
    start: Instant,
    still: bool,
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
        let dir = dir()?.clone();
        let (tx, rx) = sync_channel(AHEAD);
        let want = Arc::new(AtomicUsize::new(if still { COUNT - 1 } else { 0 }));
        let stop = Arc::new(AtomicBool::new(false));
        let (w, s) = (want.clone(), stop.clone());
        std::thread::Builder::new()
            .name("kase-anim".into())
            .spawn(move || {
                let mut i = 0;
                while i < COUNT && !s.load(Ordering::Relaxed) {
                    // Behind playback: skip to what is due instead of decoding stale frames.
                    i = i.max(w.load(Ordering::Relaxed));
                    let Some(h) = decode(&dir.join(format!("f{i:03}.webp"))) else {
                        log::warn!("animation frame {i} failed to decode; stopping");
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
            start: Instant::now(),
            still,
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
        let due = frame_at(now.saturating_duration_since(self.start), self.still);
        self.want.store(due, Ordering::Relaxed);
        loop {
            let f = match self.next.take() {
                Some(f) => f,
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
    hidden: Option<Instant>,
}

impl Art {
    /// Call on every tick with whether the art is on screen.
    pub fn sync(&mut self, want: bool, still: bool, now: Instant) {
        if want && !self.want {
            self.player = Player::start(still);
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
        assert_eq!(frame_at(Duration::ZERO, false), 0);
        assert_eq!(frame_at(Duration::from_millis(500), false), 72);
        assert_eq!(frame_at(Duration::from_secs(60), false), COUNT - 1);
        assert_eq!(frame_at(Duration::ZERO, true), COUNT - 1, "reduce motion");
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
        assert_eq!(p.shown.as_ref().map(|f| f.0), Some(COUNT - 1));
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
        assert_eq!(p.shown.as_ref().map(|f| f.0), Some(COUNT - 1));
    }

    #[test]
    fn frames_are_contiguous() {
        // Skipped until the frames are in the repo.
        let Some(d) = dir() else { return };
        for i in 0..COUNT {
            assert!(d.join(format!("f{i:03}.webp")).is_file(), "missing f{i:03}");
        }
        assert!(
            !d.join(format!("f{COUNT:03}.webp")).exists(),
            "extra frames"
        );
    }
}
