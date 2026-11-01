//! Pure presentation logic: what the island shows, when, and how it moves there.
//! No iced or Wayland types here, so all of it is unit tested.

use std::time::{Duration, Instant};

use crate::island::{
    ALERT_MAX_W, Alert, Content, EXPANDED_H, EXPANDED_W_BUDS, Motion, Parts, display_name,
    elsewhere_label, glow_for, shape,
};
use kase::overlay_protocol::{
    ComponentState, OverlayCommand, OverlayConfig, OverlayMode, OverlayState, OverlayStyle,
};

use crate::settle::{Settler, band, meaningful};
use kase::ui::anim::Clip;

fn present(c: &Option<ComponentState>) -> bool {
    c.as_ref().is_some_and(|c| c.present)
}

/// What `next` is worth showing, compared with `prev`: the full card, a one-line alert,
/// or nothing. When several things change at once the most important one wins.
pub fn event(prev: Option<&OverlayState>, next: &OverlayState) -> Option<Content> {
    let was_connected = prev.is_some_and(|p| p.connected);
    if !next.connected {
        return was_connected.then_some(Content::Alert(Alert::Disconnected));
    }
    let Some(prev) = prev.filter(|_| was_connected) else {
        return Some(Content::Expanded);
    };
    if prev.elsewhere != next.elsewhere {
        return Some(Content::Alert(if next.elsewhere {
            Alert::Elsewhere(next.elsewhere_name.clone())
        } else {
            Alert::Back
        }));
    }
    let pairs = [
        (&prev.left, &next.left),
        (&prev.right, &next.right),
        (&prev.case, &next.case),
        (&prev.headphone, &next.headphone),
    ];
    // A pod came or went: the whole picture changed. The case coming or going is not
    // news: it reads "unknown" for a while whenever a pod goes in or both come out.
    if pairs
        .iter()
        .enumerate()
        .any(|(i, (a, b))| i != 2 && present(a) != present(b))
    {
        return Some(Content::Expanded);
    }
    let mut low = Parts::default();
    let mut charging = Parts::default();
    for (i, (a, b)) in pairs.iter().enumerate() {
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !b.present {
            continue;
        }
        let flag = |p: &mut Parts| match i {
            0 => p.left = true,
            1 => p.right = true,
            2 => p.case = true,
            _ => p.headphone = true,
        };
        if !b.charging && band(b.level) < band(a.level) {
            flag(&mut low);
        }
        if b.charging && !a.charging {
            flag(&mut charging);
        }
    }
    if low.any() {
        return Some(Content::Alert(Alert::Low(low)));
    }
    if charging.any() {
        return Some(Content::Alert(Alert::Charging(charging)));
    }
    // Asked for with a stem hold, so say what happened before anything else routine.
    if next.mic_failures > prev.mic_failures {
        return Some(Content::Alert(Alert::MicUnavailable));
    }
    match (prev.mic, next.mic) {
        (Some(a), Some(b)) if a != b => return Some(Content::Alert(Alert::Mic(b))),
        _ => {}
    }
    match (prev.listening_mode, next.listening_mode) {
        (Some(a), Some(b)) if a != b => Some(Content::Alert(Alert::Listening(b))),
        _ => None,
    }
}

/// Shown by a preview before any battery data has arrived.
fn placeholder() -> OverlayState {
    let unknown = || {
        Some(ComponentState {
            present: false,
            ..Default::default()
        })
    };
    OverlayState {
        name: "AirPods".into(),
        connected: true,
        left: unknown(),
        right: unknown(),
        case: unknown(),
        ..Default::default()
    }
}

/// The content draws a charging ring: the full card, or a charging alert.
fn shows_charging(content: Option<&Content>, state: &OverlayState) -> bool {
    let charging = |c: &Option<ComponentState>| c.as_ref().is_some_and(|c| c.charging && c.present);
    match content {
        Some(Content::Expanded) => [&state.left, &state.right, &state.case, &state.headphone]
            .into_iter()
            .any(charging),
        Some(Content::Alert(Alert::Charging(_))) => true,
        _ => false,
    }
}

/// How long the card stays up when the AirPods connect from the open case.
const CASE_CARD: Duration = Duration::from_secs(6);
/// How long it stays after a pod comes out of the case.
const CASE_CARD_AFTER_POD_OUT: Duration = Duration::from_secs(2);
/// The case spins again on a connect this long after the last spin (once a day, in
/// practice: the AirPods sit in the case overnight).
const SPIN_AGAIN_AFTER: Duration = Duration::from_secs(6 * 3600);
/// How long the live case card stays once the lid shuts (the lid closing plays first).
const CASE_CARD_LID_SHUT: Duration = Duration::from_millis(1500);
/// How long the case card stays after both pods are put away.
const CASE_CARD_PUT_AWAY: Duration = Duration::from_secs(3);
/// A presentation stays at least this long before a different one replaces it.
pub const MIN_DISPLAY: Duration = Duration::from_millis(1200);
/// An open card stays at least this long after its last change.
const CARD_AFTER_CHANGE: Duration = Duration::from_secs(3);

/// Two alerts of the same kind told as one: left then right charging becomes "AirPods
/// charging"; a newer noise control mode or mic replaces the older one.
fn merge(cur: &Content, new: &Content) -> Option<Content> {
    let (Content::Alert(a), Content::Alert(b)) = (cur, new) else {
        return None;
    };
    let union = |x: &Parts, y: &Parts| Parts {
        left: x.left || y.left,
        right: x.right || y.right,
        case: x.case || y.case,
        headphone: x.headphone || y.headphone,
    };
    Some(Content::Alert(match (a, b) {
        (Alert::Charging(x), Alert::Charging(y)) => Alert::Charging(union(x, y)),
        (Alert::Low(x), Alert::Low(y)) => Alert::Low(union(x, y)),
        (Alert::Listening(_), Alert::Listening(_))
        | (Alert::Mic(_), Alert::Mic(_))
        | (Alert::Elsewhere(_), Alert::Elsewhere(_)) => b.clone(),
        _ => return None,
    }))
}

/// What the iced side has to do with the layer surface after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAction {
    None,
    /// Create the layer surface.
    Map,
    /// Destroy the layer surface (the island folded away).
    Unmap,
}

#[derive(Debug)]
pub struct Controller {
    pub config: OverlayConfig,
    pub state: Option<OverlayState>,
    /// Content popped up by an event or `show`, until the instant.
    pop: Option<(Content, Instant)>,
    /// `hide` received: stay hidden until the next show trigger.
    forced_hidden: bool,
    mapped: bool,
    last_tick: Option<Instant>,
    /// `show` with reason "preview" (settings button): visible until this instant
    /// whatever the mode or link state, with a placeholder when there is no data.
    preview_until: Option<Instant>,
    /// A fullscreen window is active on the desktop (reported by the compositor).
    fullscreen: bool,
    /// Preview started from the opacity slider: show the fullscreen look.
    opacity_preview: bool,
    /// Just disconnected: say so until this instant, then fold away.
    farewell_until: Option<Instant>,
    pub motion: Motion,
    /// Frames are still needed (springs or content transitions in flight).
    moving: bool,
    /// The state the island last reacted to (`state` is the newest, shown at once).
    settled: Option<OverlayState>,
    /// Gathers a burst of updates into one moment.
    settler: Settler,
    /// React to every state at once (tests of the judging rules).
    pub settle_at_once: bool,
    /// When the current pop-up started showing (for the minimum time on screen).
    pop_since: Option<Instant>,
    /// A presentation waiting for the current one's minimum time; the newest wins.
    queued: Option<(Content, Instant)>,
    /// News told in the open card's second line instead of replacing the card.
    news: Option<(Alert, Instant)>,
    /// The case card was cut short (a pod came out): later changes can't stretch it.
    card_cap: Option<Instant>,
    /// Lowest battery band already announced per part (left, right, case, headphone).
    low_told: [Option<u8>; 4],
    /// The clip the case card plays for the moment that opened it.
    pub case_clip: Clip,
    /// When the case last played its first-connect spin.
    last_spin: Option<Instant>,
    /// The case card follows the case live (setting on, clips installed).
    live: bool,
}

impl Controller {
    /// When the case card last chose the first-of-the-day spin (the live card spins its
    /// next lid opening when this changes).
    pub fn last_spin(&self) -> Option<Instant> {
        self.last_spin
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new(OverlayConfig::default())
    }
}

impl Controller {
    pub fn new(config: OverlayConfig) -> Self {
        let live = config.live_case && kase::ui::anim::live_ready();
        // The drawing side reads the same switch.
        crate::island::set_live(live);
        Self {
            config,
            state: None,
            pop: None,
            forced_hidden: false,
            mapped: false,
            last_tick: None,
            preview_until: None,
            fullscreen: false,
            opacity_preview: false,
            farewell_until: None,
            motion: Motion::default(),
            moving: false,
            settled: None,
            settler: Settler::default(),
            settle_at_once: false,
            pop_since: None,
            queued: None,
            news: None,
            card_cap: None,
            low_told: [None; 4],
            case_clip: Clip::Open,
            last_spin: None,
            live,
        }
    }

    fn pop_up(&mut self, content: Content, now: Instant) {
        self.forced_hidden = false;
        let secs = match &content {
            Content::Alert(a) => a.duration(),
            _ => self.config.auto_hide_secs.clamp(0.5, 600.0),
        };
        let until = now + Duration::from_secs_f32(secs);
        let cap = self.card_cap.filter(|t| now < *t);
        let since = self.pop_since;
        if let Some((cur, cur_until)) = &mut self.pop
            && now < *cur_until
        {
            // The same kind of alert is up: tell both as one, in place.
            if let Some(merged) = merge(cur, &content) {
                *cur = merged;
                *cur_until = (*cur_until).max(until);
                self.queued = None;
                return;
            }
            // The card is open: it stays, says the news on its second line and gives a
            // little bounce, and stays up a while after the change.
            if *cur == Content::Expanded {
                let keep = (*cur_until).max(match &content {
                    Content::Expanded => until,
                    _ => now + CARD_AFTER_CHANGE,
                });
                *cur_until = cap.map_or(keep, |c| keep.min(c));
                if let Content::Alert(a) = content {
                    self.news = Some((a, now + Duration::from_secs_f32(secs)));
                }
                if self.mapped && self.motion.shown == Some(Content::Expanded) {
                    self.motion.bump();
                    self.moving = true;
                }
                self.queued = None;
                return;
            }
            // Something else is up and hasn't been seen long enough: wait for it.
            if *cur != content
                && let Some(since) = since
                && now < since + MIN_DISPLAY
            {
                self.queued = Some((content, since + MIN_DISPLAY));
                return;
            }
        }
        let card_stays = self.config.mode == OverlayMode::Always
            || self.previewing(now)
            || content == Content::Expanded;
        if self.mapped && card_stays && self.motion.shown == Some(Content::Expanded) {
            // The card stays; its headline changes and it gives a little bounce.
            self.motion.bump();
            self.moving = true;
        }
        if self.popped(now) != Some(&content) {
            self.pop_since = Some(now);
        }
        self.news = None;
        self.queued = None;
        self.pop = Some((content, until));
    }

    /// Announces each low battery band once per discharge. A part re-arms when it charges
    /// or rises 5 points above the line it crossed.
    fn tell_low(&mut self, parts: Parts, state: &OverlayState) -> Parts {
        let comps = [&state.left, &state.right, &state.case, &state.headphone];
        let wanted = [parts.left, parts.right, parts.case, parts.headphone];
        let mut out = [false; 4];
        for i in 0..4 {
            let Some(c) = comps[i] else { continue };
            let b = band(c.level);
            if wanted[i] && self.low_told[i].is_none_or(|told| b < told) {
                self.low_told[i] = Some(b);
                out[i] = true;
            }
        }
        Parts {
            left: out[0],
            right: out[1],
            case: out[2],
            headphone: out[3],
        }
    }

    fn rearm_low(&mut self, state: &OverlayState) {
        let comps = [&state.left, &state.right, &state.case, &state.headphone];
        for (told, c) in self.low_told.iter_mut().zip(comps) {
            let Some(c) = c.as_ref().filter(|c| c.present) else {
                continue;
            };
            *told = match *told {
                _ if c.charging => None,
                Some(0) if c.level >= 25 => None,
                Some(0) if c.level >= 15 => Some(1),
                Some(1) if c.level >= 25 => None,
                t => t,
            };
        }
    }

    /// Lights the border for `content` (purist style only; the classic island has no
    /// key line, so it would only spend frames on nothing).
    fn announce(&mut self, content: &Content, connected: bool, now: Instant) {
        if self.config.style != OverlayStyle::Purist
            || self.config.mode == OverlayMode::Off
            || self.config.reduce_motion
        {
            return;
        }
        let state = self.display_state(now).unwrap_or_else(placeholder);
        if let Some(glow) = glow_for(content, &state, connected) {
            self.motion.light(glow);
            self.moving = true;
        }
        if content == &Content::Alert(Alert::MicUnavailable) {
            self.motion.shake();
        }
    }

    /// Applies one protocol command. `Quit` is handled by the caller.
    pub fn apply(&mut self, cmd: OverlayCommand, now: Instant) {
        match cmd {
            OverlayCommand::Config(cfg) => {
                let was = self.config.mode;
                self.config = cfg;
                self.live = self.config.live_case && kase::ui::anim::live_ready();
                crate::island::set_live(self.live);
                if was == OverlayMode::Always && self.config.mode != OverlayMode::Always {
                    // Leaving `always`: keep the card up for one more period, then go.
                    if self.mapped {
                        self.pop_up(Content::Expanded, now);
                    } else {
                        self.pop = None;
                    }
                }
            }
            OverlayCommand::State { state } => {
                if self.motion.update_levels(&state, self.mapped) {
                    self.moving = true;
                }
                if meaningful(self.state.as_ref(), &state) {
                    self.settler.changed(now);
                }
                self.state = Some(state);
                if self.settle_at_once && self.settler.flush() {
                    self.judge(now);
                }
            }
            OverlayCommand::Fullscreen { active } => self.fullscreen = active,
            OverlayCommand::Show { reason } if reason == "preview" || reason == "opacity" => {
                self.opacity_preview = reason == "opacity";
                self.forced_hidden = false;
                let secs = self.config.auto_hide_secs.clamp(2.0, 600.0);
                let fresh = !self.previewing(now);
                self.preview_until = Some(now + Duration::from_secs_f32(secs));
                // Show off the border light when a preview opens (not on every slider step).
                if fresh && !self.opacity_preview {
                    self.announce(&Content::Expanded, true, now);
                }
            }
            OverlayCommand::Show { .. } => {
                self.announce(&Content::Expanded, false, now);
                self.pop_up(Content::Expanded, now)
            }
            OverlayCommand::Hide => {
                self.pop = None;
                self.farewell_until = None;
                self.forced_hidden = true;
            }
            // The quick look has its own surface (`main.rs`); nothing changes here.
            OverlayCommand::QuickLook { .. } | OverlayCommand::Quit => {}
        }
    }

    /// Reacts to the settled state: what changed since the last moment, and what (if
    /// anything) is worth showing for it.
    fn judge(&mut self, now: Instant) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let prev = self.settled.replace(state.clone());
        let mut ev = event(prev.as_ref(), &state);
        let just_connected = state.connected && !prev.as_ref().is_some_and(|p| p.connected);
        let was_in_case = prev.as_ref().is_some_and(|p| p.connected && p.in_case);
        self.rearm_low(&state);
        if let Some(Content::Alert(Alert::Low(parts))) = ev {
            let told = self.tell_low(parts, &state);
            ev = told.any().then_some(Content::Alert(Alert::Low(told)));
        }
        // Pods sitting in the open case: the case card or nothing. Their routine news (a
        // pod charging, the case level arriving) and the lid closing aren't worth a pop;
        // the case going on a charger is.
        let case_charging = matches!(
            &ev,
            Some(Content::Alert(Alert::Charging(p))) if p.case && !p.left && !p.right
        );
        if (state.in_case && !case_charging) || (was_in_case && !state.connected) {
            ev = None;
        }
        // Live case card: the 3D case follows the lid and each pod, so every change to the
        // case while it is in play is worth the card, for a while after the last one.
        let live = self.live && state.case_state.is_some();
        let case_key = |s: &OverlayState| s.case_state.map(|c| (c.lid, c.left, c.right));
        let case_moved = live
            && !just_connected
            && state.connected
            && prev
                .as_ref()
                .is_some_and(|p| case_key(p) != case_key(&state));
        let lid_shut = live
            && state
                .case_state
                .is_some_and(|c| c.lid == kase::overlay_protocol::Lid::Closed);
        if !live && was_in_case && state.connected && !state.in_case {
            // A pod came out: the case card has done its job.
            let cap = now + CASE_CARD_AFTER_POD_OUT;
            if let Some((Content::Expanded, until)) = &mut self.pop {
                *until = (*until).min(cap);
                self.card_cap = Some(cap);
            }
        }
        let card_ok = self.config.case_card && !self.fullscreen;
        let put_away = !just_connected && !was_in_case && state.in_case && state.connected;
        if card_ok && ((just_connected && state.in_case) || put_away) {
            ev = Some(Content::Expanded);
        }
        let live_card = card_ok && crate::island::has_art_with(&state, self.live);
        if live && live_card && (case_moved || just_connected) {
            ev = Some(Content::Expanded);
        }
        if !state.connected {
            self.pop = None;
            self.queued = None;
            self.news = None;
        }
        match ev {
            Some(Content::Alert(Alert::Disconnected)) => {
                let secs = Alert::Disconnected.duration();
                self.farewell_until = Some(now + Duration::from_secs_f32(secs));
                self.announce(&Content::Alert(Alert::Disconnected), false, now);
            }
            Some(c) => {
                self.farewell_until = None;
                let case_card =
                    c == Content::Expanded && if live { live_card } else { state.in_case };
                if case_card && live {
                    let both_in = state.case_state.is_some_and(|c| {
                        use kase::overlay_protocol::Pod::Seated;
                        c.left == Seated && c.right == Seated
                    });
                    if just_connected
                        && both_in
                        && self.last_spin.is_none_or(|t| now >= t + SPIN_AGAIN_AFTER)
                    {
                        // The first open of the day spins before the lid opens.
                        self.last_spin = Some(now);
                    }
                } else if case_card {
                    self.case_clip = if put_away {
                        Clip::PutAway
                    } else if self.last_spin.is_none_or(|t| now >= t + SPIN_AGAIN_AFTER) {
                        // The first connect of the day gets the spin; later ones just open.
                        self.last_spin = Some(now);
                        Clip::SpinOpen
                    } else {
                        Clip::Open
                    };
                }
                self.announce(&c, just_connected, now);
                self.card_cap = None;
                self.pop_up(c, now);
                if case_card && let Some((_, until)) = &mut self.pop {
                    *until = now
                        + if live {
                            // Long enough to see the lid shut, else a while after the
                            // last change.
                            if lid_shut {
                                CASE_CARD_LID_SHUT
                            } else {
                                CASE_CARD
                            }
                        } else if put_away {
                            CASE_CARD_PUT_AWAY
                        } else {
                            CASE_CARD
                        };
                }
            }
            None if state.connected => self.farewell_until = None,
            None => {}
        }
    }

    fn previewing(&self, now: Instant) -> bool {
        self.preview_until.is_some_and(|t| now < t)
    }

    fn popped(&self, now: Instant) -> Option<&Content> {
        self.pop.as_ref().filter(|(_, t)| now < *t).map(|(c, _)| c)
    }

    /// What to draw: the real state, or during a preview without data a placeholder
    /// (dashes), so the user can always see where the island sits.
    pub fn display_state(&self, now: Instant) -> Option<OverlayState> {
        match &self.state {
            Some(s) if s.connected || !self.previewing(now) => Some(s.clone()),
            Some(s) => Some(OverlayState {
                connected: true,
                ..s.clone()
            }),
            None if self.previewing(now) || self.mapped => Some(placeholder()),
            None => None,
        }
    }

    /// What the island should show now; None folds it away.
    pub fn target(&self, now: Instant) -> Option<Content> {
        if self.previewing(now) {
            return Some(Content::Expanded);
        }
        if self.config.mode == OverlayMode::Off || self.forced_hidden {
            return None;
        }
        if self.farewell_until.is_some_and(|t| now < t) {
            return Some(Content::Alert(Alert::Disconnected));
        }
        if !self.state.as_ref().is_some_and(|s| s.connected) {
            return None;
        }
        let popped = self.popped(now).cloned();
        match self.config.mode {
            OverlayMode::Always => Some(Content::Expanded),
            OverlayMode::Compact => Some(popped.unwrap_or(Content::Compact)),
            OverlayMode::Auto => popped,
            OverlayMode::Off => None,
        }
    }

    /// The second line of the full card: the latest news while it is fresh, otherwise
    /// where the audio is and the noise control mode.
    pub fn headline(&self, now: Instant) -> String {
        let Some(state) = self.display_state(now) else {
            return String::new();
        };
        if let Some(Content::Alert(a)) = self.popped(now) {
            return a.title(&state);
        }
        if let Some((a, until)) = &self.news
            && now < *until
            && self.popped(now) == Some(&Content::Expanded)
        {
            return a.title(&state);
        }
        if state.in_case {
            return "Case open".into();
        }
        if state.elsewhere {
            return elsewhere_label(state.elsewhere_name.as_deref());
        }
        match state.listening_mode {
            Some(m) => m.label().to_string(),
            None if self.state.is_none() => "Waiting for battery".into(),
            None => "Connected".into(),
        }
    }

    /// The device's name for the card's first line.
    pub fn name(&self, now: Instant) -> String {
        self.display_state(now)
            .map(|s| display_name(&s))
            .unwrap_or_default()
    }

    /// The user's opacity applies while a fullscreen window is in front (or while
    /// they drag the slider); otherwise the island is solid.
    pub fn effective_opacity(&self) -> f32 {
        if self.fullscreen || self.opacity_preview {
            self.config.opacity.clamp(0.2, 1.0)
        } else {
            1.0
        }
    }

    /// Advances the animation to `now` and says what to do with the surface.
    pub fn tick(&mut self, now: Instant) -> SurfaceAction {
        if self.settler.take(now) {
            self.judge(now);
        }
        if let Some((c, at)) = self.queued.clone()
            && now >= at
        {
            self.queued = None;
            self.pop_up(c, now);
        }
        let target = self.target(now);
        if self.pop.as_ref().is_some_and(|(_, t)| now >= *t) {
            self.pop = None;
            self.pop_since = None;
            self.card_cap = None;
        }
        if self.news.as_ref().is_some_and(|(_, t)| now >= *t) {
            self.news = None;
        }
        if self.farewell_until.is_some_and(|t| now >= t) {
            self.farewell_until = None;
        }
        if self.preview_until.is_some_and(|t| now >= t) {
            self.preview_until = None;
            self.opacity_preview = false;
        }
        let dt = self
            .last_tick
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or_default()
            // A long gap (no ticks while idle) must not skip an animation.
            .min(Duration::from_millis(50));
        self.last_tick = Some(now);

        let state = self.display_state(now).unwrap_or_else(placeholder);
        self.motion.reduced = self.config.reduce_motion;
        // The light on charging rings runs on the purist island, never over a game.
        self.motion.sheen = self.config.style == OverlayStyle::Purist
            && !self.fullscreen
            && shows_charging(target.as_ref(), &state);
        let action = match (&target, self.mapped) {
            (Some(_), false) => {
                self.mapped = true;
                self.motion.reset();
                self.motion.update_levels(&state, false);
                SurfaceAction::Map
            }
            _ => SurfaceAction::None,
        };
        let caption = self.headline(now);
        self.motion
            .set_caption(&caption, self.mapped && action == SurfaceAction::None);
        let target_shape = target.as_ref().map(|c| shape(c, &state));
        if self.motion.shown != target {
            self.motion.switch(target, target_shape);
        } else if let Some(s) = target_shape {
            self.motion.resize(s);
        }
        self.moving = self.motion.step(dt.as_secs_f32());
        if action == SurfaceAction::None && self.mapped && self.motion.gone() {
            self.mapped = false;
            self.moving = false;
            return SurfaceAction::Unmap;
        }
        action
    }

    #[cfg(test)]
    pub fn is_mapped(&self) -> bool {
        self.mapped
    }

    /// The compositor closed our surface (e.g. its output went away).
    pub fn surface_lost(&mut self) {
        self.mapped = false;
        self.motion.reset();
    }

    /// True while frames must be produced.
    pub fn is_animating(&self, now: Instant) -> bool {
        let target = self.target(now);
        if target.is_some() != self.mapped || self.motion.shown != target {
            return true;
        }
        self.mapped && self.moving
    }

    /// The next instant at which the island changes by itself (a pop-up ends, a
    /// preview ends), so the caller can sleep until exactly then instead of polling.
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.pop.as_ref().map(|(_, t)| *t),
            self.preview_until,
            self.farewell_until,
            self.settler.due(),
            self.queued.as_ref().map(|(_, t)| *t),
            self.news.as_ref().map(|(_, t)| *t),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Room the surface needs: the biggest island plus headroom for a spring's
    /// overshoot and the bump. It is fixed, so the surface is never resized while
    /// shown: a resize lands a frame apart from the drawing and makes the island jump.
    pub fn extent(&self) -> (f32, f32) {
        let k = self.config.size.scale();
        let art = if kase::ui::anim::dir().is_some() {
            crate::island::ART_BLOCK
        } else {
            0.0
        };
        (
            (ALERT_MAX_W.max(EXPANDED_W_BUDS) * 1.1 + 8.0) * k,
            // The case card can grow by the animation box.
            ((EXPANDED_H + art) * 1.1 + 6.0) * k,
        )
    }
}

#[cfg(test)]
#[path = "logic_tests.rs"]
mod tests;
