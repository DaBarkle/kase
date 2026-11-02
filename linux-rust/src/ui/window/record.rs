//! `kase --ui-demo` on the test clock: records every frame of the window, the same way
//! `kase-overlay --script` records the island (see `kase::testclock`). Used by
//! `tools/visual-qa/showcase.py` for the README demos.
//!
//! Off unless the demo runs with `KASE_TEST_STEP_MS` and `KASE_SHOT_DIR` set. Then:
//!
//! - `KASE_UI_SCRIPT` lists timed steps, `ms:action=value` separated by `;`, for example
//!   `600:scenario=airpods4;3200:tab=island`. Actions: `tab` (AirPods, Island, Settings),
//!   `scenario` (re-seeds the demo hub, as `--ui-demo=NAME` would), `appearance`
//!   (system, light, dark), `mode` (off, transparency, adaptive, noise_cancellation),
//!   `window` (close: fold into the pill; open: grow out of it again).
//! - `KASE_TEST_END_MS` is when the recording stops (default 4000).

use std::collections::VecDeque;

/// One scripted step.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Tab(String),
    Scenario(String),
    Appearance(String),
    Mode(String),
    /// `window=close` folds the body into the pill; `window=open` grows it out again.
    Window(String),
}

pub struct Recorder {
    events: VecDeque<(f64, Step)>,
    end_ms: f64,
    /// The first frame is time zero.
    pub started: bool,
}

impl Recorder {
    pub fn from_env() -> Option<Self> {
        if !crate::testclock::stepping() || crate::testclock::shot_dir().is_none() {
            return None;
        }
        let script = std::env::var("KASE_UI_SCRIPT").unwrap_or_default();
        let end_ms = std::env::var("KASE_TEST_END_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4000.0);
        Some(Self {
            events: parse(&script).into(),
            end_ms,
            started: false,
        })
    }

    /// Steps due at test time `t_ms`, in order.
    pub fn due(&mut self, t_ms: f64) -> Vec<Step> {
        let mut out = vec![];
        while self
            .events
            .front()
            .is_some_and(|(at, _)| *at <= t_ms + 1e-6)
        {
            if let Some((_, s)) = self.events.pop_front() {
                out.push(s);
            }
        }
        out
    }

    /// Test time of the next step, or the end.
    pub fn next_ms(&self) -> f64 {
        self.events
            .front()
            .map(|(at, _)| *at)
            .unwrap_or(self.end_ms)
            .min(self.end_ms)
    }

    pub fn past_end(&self, t_ms: f64) -> bool {
        t_ms >= self.end_ms - 1e-6
    }
}

fn parse(script: &str) -> Vec<(f64, Step)> {
    let mut steps: Vec<(f64, Step)> = script
        .split(';')
        .filter_map(|item| {
            let (at, action) = item.trim().split_once(':')?;
            let (key, value) = action.split_once('=')?;
            let at: f64 = at.trim().parse().ok()?;
            let value = value.trim().to_string();
            let step = match key.trim() {
                "tab" => Step::Tab(value),
                "scenario" => Step::Scenario(value),
                "appearance" => Step::Appearance(value),
                "mode" => Step::Mode(value),
                "window" => Step::Window(value),
                other => {
                    log::warn!("KASE_UI_SCRIPT: unknown action {other}");
                    return None;
                }
            };
            Some((at, step))
        })
        .collect();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_steps() {
        let s = parse("3200:tab=Island; 600:scenario=airpods4;bad;9:nope=1");
        assert_eq!(
            s,
            vec![
                (600.0, Step::Scenario("airpods4".into())),
                (3200.0, Step::Tab("Island".into())),
            ]
        );
    }
}
