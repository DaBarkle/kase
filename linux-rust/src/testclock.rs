//! A frame-exact clock for the visual test suite (`tools/visual-qa`).
//!
//! Off unless `KASE_TEST_STEP_MS` is set, and then only in the process that reads it:
//! normal runs never see it. While it is on, "now" is `start + frame * step` instead of
//! the wall clock, and the app moves it forward one frame at a time after each screenshot
//! (see `Recorder`). However slow the renderer, every frame of every transition is
//! captured at exactly the same moment on every run.
//!
//! `KASE_SHOT_DIR` is where frames go (`f00001.png`, `f00001-quick.png`, ...), with one
//! line per frame in `frames.jsonl` saying when it was taken and what was on screen.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
static FRAME: AtomicU64 = AtomicU64::new(0);
/// The frame that counts as time zero (after a warm-up).
static ZERO: AtomicU64 = AtomicU64::new(0);

fn step_ms() -> Option<f64> {
    static S: OnceLock<Option<f64>> = OnceLock::new();
    *S.get_or_init(|| {
        std::env::var("KASE_TEST_STEP_MS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|ms| *ms > 0.0 && ms.is_finite())
    })
}

/// The test clock is on.
pub fn stepping() -> bool {
    step_ms().is_some()
}

fn start() -> Instant {
    *START.get_or_init(Instant::now)
}

/// The current instant: the wall clock, or the test clock's frame time.
pub fn now() -> Instant {
    match step_ms() {
        Some(ms) => start() + Duration::from_secs_f64(frame() as f64 * ms / 1000.0),
        None => Instant::now(),
    }
}

/// Frames taken so far.
pub fn frame() -> u64 {
    FRAME.load(Ordering::Relaxed)
}

/// Test time in milliseconds since time zero (negative during a warm-up).
pub fn elapsed_ms() -> f64 {
    (frame() as f64 - ZERO.load(Ordering::Relaxed) as f64) * step()
}

/// Makes the current frame time zero.
pub fn set_zero() {
    ZERO.store(frame(), Ordering::Relaxed);
}

/// One frame forward.
pub fn advance() {
    FRAME.fetch_add(1, Ordering::Relaxed);
}

/// Jumps forward to the first frame at or after `at` (nothing moves until then, so the
/// frames in between would all be the same). Never goes back.
pub fn skip_to(at: Instant) {
    let Some(ms) = step_ms() else { return };
    let ahead = at.saturating_duration_since(start()).as_secs_f64() * 1000.0;
    let target = (ahead / ms).ceil() as u64;
    FRAME.fetch_max(target, Ordering::Relaxed);
}

/// The frame-time step in milliseconds (0 when off).
pub fn step() -> f64 {
    step_ms().unwrap_or(0.0)
}

/// Where frames are written, if anywhere.
pub fn shot_dir() -> Option<&'static PathBuf> {
    static D: OnceLock<Option<PathBuf>> = OnceLock::new();
    D.get_or_init(|| {
        let d = PathBuf::from(std::env::var_os("KASE_SHOT_DIR")?);
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    })
    .as_ref()
}

/// Saves one surface's screenshot for the current frame; returns the file name.
pub fn save(shot: &iced::window::Screenshot, surface: &str) -> Option<String> {
    let dir = shot_dir()?;
    let name = if surface.is_empty() {
        format!("f{:05}.png", frame())
    } else {
        format!("f{:05}-{surface}.png", frame())
    };
    let img = image::RgbaImage::from_raw(shot.size.width, shot.size.height, shot.rgba.to_vec())?;
    match img.save(dir.join(&name)) {
        Ok(()) => Some(name),
        Err(e) => {
            log::warn!("cannot save frame {name}: {e}");
            None
        }
    }
}

/// Appends one line to `frames.jsonl`.
pub fn log_frame(line: &serde_json::Value) {
    let Some(dir) = shot_dir() else { return };
    let path: &Path = dir.as_ref();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.join("frames.jsonl"));
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{line}");
    }
}

/// The most frames a run may take before it stops by itself (`KASE_TEST_MAX_FRAMES`,
/// default 6000: 50 s at 120 fps).
pub fn max_frames() -> u64 {
    std::env::var("KASE_TEST_MAX_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6000)
}

/// Merges `patch` into `base`: objects key by key, anything else replaced. A `null`
/// in the patch removes the key, so a script can clear an optional field.
pub fn merge(base: &mut serde_json::Value, patch: &serde_json::Value) {
    use serde_json::Value;
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else {
                    merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patches_objects_and_clears_nulls() {
        let mut s = json!({"left": {"level": 80, "present": true}, "mic": "airpods"});
        merge(
            &mut s,
            &json!({"left": {"present": false}, "mic": null, "in_case": true}),
        );
        assert_eq!(
            s,
            json!({"left": {"level": 80, "present": false}, "in_case": true})
        );
    }

    #[test]
    fn off_by_default() {
        // The test runner never sets KASE_TEST_STEP_MS.
        assert!(!stepping());
        let a = now();
        assert!(now() >= a);
    }
}
