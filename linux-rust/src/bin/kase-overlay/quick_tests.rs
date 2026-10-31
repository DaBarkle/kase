//! The tray quick look: hit areas, closing on its own, following the mode.

use super::*;

fn state() -> OverlayState {
    OverlayState {
        connected: true,
        modes: vec![
            ListeningMode::Off,
            ListeningMode::Transparency,
            ListeningMode::Adaptive,
            ListeningMode::NoiseCancellation,
        ],
        listening_mode: Some(ListeningMode::Adaptive),
        ..Default::default()
    }
}

fn run(q: &mut Quick, secs: f32) {
    for _ in 0..(secs / 0.01) as usize {
        q.step(0.01);
    }
}

#[test]
fn taps_land_on_the_mode_or_the_open_button() {
    let s = state();
    let y = MODES_Y + 10.0;
    assert_eq!(hit(&s, INSET + 1.0, y), Some(Hit::Mode(0)));
    assert_eq!(hit(&s, W - INSET - 1.0, y), Some(Hit::Mode(3)));
    assert_eq!(hit(&s, W / 2.0, button_y(&s) + 5.0), Some(Hit::Open));
    assert_eq!(hit(&s, W / 2.0, 10.0), None);
    // No noise control: the button moves up, no mode row.
    let plain = OverlayState::default();
    assert_eq!(
        hit(&plain, W / 2.0, button_y(&plain) + 5.0),
        Some(Hit::Open)
    );
    assert!(size(&plain).1 < size(&s).1);
}

#[test]
fn it_closes_by_itself_when_left_alone_or_left() {
    let mut q = Quick::new(OverlayPosition::BottomRight, false);
    run(&mut q, 1.0);
    assert!(!q.closing());
    run(&mut q, 6.0);
    assert!(q.closing());
    run(&mut q, 1.0);
    assert!(q.gone());

    let mut q = Quick::new(OverlayPosition::BottomRight, false);
    q.pointer(Some(Hit::Open), true);
    run(&mut q, 10.0);
    assert!(!q.closing(), "stays while the pointer is on it");
    q.pointer(None, false);
    run(&mut q, 0.5);
    assert!(!q.closing());
    run(&mut q, 1.0);
    assert!(q.closing());
}

#[test]
fn a_tapped_mode_shows_until_the_airpods_confirm() {
    let mut s = state();
    let mut q = Quick::new(OverlayPosition::BottomRight, false);
    q.follow(&s);
    assert_eq!(q.selected.value, 2.0);
    q.pending = Some(ListeningMode::Off);
    q.follow(&s);
    run(&mut q, 1.0);
    assert!((q.selected.value - 0.0).abs() < 0.01);
    s.listening_mode = Some(ListeningMode::Off);
    q.follow(&s);
    assert_eq!(q.pending, None);
}

#[test]
fn reduce_motion_only_fades() {
    let q = Quick::new(OverlayPosition::TopLeft, true);
    assert_eq!(q.presence().0, 1.0);
}

#[test]
fn slow_ticks_still_count_real_time() {
    // At rest only a 250 ms timer ticks; the close timer must still see real seconds.
    let mut q = Quick::new(OverlayPosition::BottomRight, false);
    for _ in 0..25 {
        q.step(0.25);
    }
    assert!(q.closing());
}

#[test]
fn a_click_while_it_folds_away_brings_it_back() {
    let mut q = Quick::new(OverlayPosition::BottomRight, false);
    q.close();
    q.reopen();
    assert!(!q.closing());
    run(&mut q, 1.0);
    assert!(q.presence().1 > 0.9);
}
