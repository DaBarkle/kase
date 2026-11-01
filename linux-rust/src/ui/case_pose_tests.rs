use super::Seated as Baked;
use super::*;
use crate::core::case_state::CaseSource;

const DT: f32 = 1.0 / 144.0;

fn lens(c: LiveClip) -> usize {
    match c {
        LiveClip::LidOpen(_) => 99,
        LiveClip::LidClose(_) => 48,
        LiveClip::SpinLidOpen => 188,
        LiveClip::Case(_) => 1,
        LiveClip::Rise(_) => 194,
        LiveClip::Sink(_) => 99,
    }
}

fn st(lid: Lid, left: Pod, right: Pod) -> CaseState {
    CaseState {
        lid,
        left,
        right,
        source: CaseSource::Aacp,
        seq: 0,
    }
}

fn run<F: Fn(LiveClip) -> usize>(p: &mut CasePose<F>, secs: f32) {
    for _ in 0..(secs * 144.0) as usize {
        p.tick(DT, true);
        check(p);
    }
}

/// Invariants at every frame: frames inside their clip, pods never move while the lid
/// does, and a pod that's out of a closed case only ever hovers.
fn check<F: Fn(LiveClip) -> usize>(p: &CasePose<F>) {
    let layers = p.layers();
    for s in layers.iter().flatten() {
        assert!(s.frame < lens(s.clip), "{s:?} past its clip");
    }
    let lid_moving = matches!(p.lid, LidShow::Moving { .. });
    let pod_moving = p.pods.iter().any(|x| matches!(x, PodShow::Moving { .. }));
    assert!(!(lid_moving && pod_moving), "a pod moved while the lid did");
}

use Lid::{Closed, Open};
use Pod::{Out, Seated, Unknown};

#[test]
fn opens_then_each_pod_rises_then_back_and_shut() {
    let mut p = CasePose::new(lens, st(Closed, Seated, Seated));
    assert_eq!(p.shown(), (Closed, Seated, Seated));
    p.set_target(st(Open, Seated, Seated));
    run(&mut p, 0.1);
    assert_eq!(p.layers()[0].unwrap().clip, LiveClip::LidOpen(Baked::Both));
    run(&mut p, 1.0);
    assert_eq!(p.shown(), (Open, Seated, Seated));

    p.set_target(st(Open, Seated, Out));
    run(&mut p, 0.2);
    assert_eq!(
        p.layers()[0].unwrap().clip,
        LiveClip::Case(Baked::L),
        "right pod left the case layer"
    );
    assert_eq!(p.layers()[2].unwrap().clip, LiveClip::Rise(Side::R));
    run(&mut p, 1.5);
    assert_eq!(p.shown(), (Open, Seated, Out));

    p.set_target(st(Open, Out, Out));
    run(&mut p, 1.6);
    assert_eq!(p.shown(), (Open, Out, Out));
    assert_eq!(p.layers()[0].unwrap().clip, LiveClip::Case(Baked::None));

    p.set_target(st(Open, Seated, Seated));
    run(&mut p, 1.0);
    assert_eq!(p.shown(), (Open, Seated, Seated));
    p.set_target(st(Closed, Seated, Seated));
    run(&mut p, 0.5);
    assert_eq!(p.shown(), (Closed, Seated, Seated));
}

#[test]
fn changing_its_mind_plays_back_from_where_it_is() {
    let mut p = CasePose::new(lens, st(Open, Seated, Seated));
    p.set_target(st(Open, Out, Seated));
    run(&mut p, 0.5);
    let before = p.layers()[1].unwrap();
    assert_eq!(before.clip, LiveClip::Rise(Side::L));
    p.set_target(st(Open, Seated, Seated));
    run(&mut p, SETTLE + 2.0 * DT);
    let after = p.layers()[1].unwrap();
    assert_eq!(after.clip, LiveClip::Rise(Side::L), "same clip, backwards");
    assert!(
        after.frame < before.frame + 20 && after.frame + 40 > before.frame,
        "no jump"
    );
    run(&mut p, 1.5);
    assert_eq!(p.shown(), (Open, Seated, Seated));
}

#[test]
fn pods_wait_for_the_lid() {
    let mut p = CasePose::new(lens, st(Closed, Seated, Seated));
    // The pod report beats the lid: the lid still opens first.
    p.set_target(st(Unknown_lid(), Out, Seated));
    run(&mut p, 0.3);
    assert!(matches!(p.lid, LidShow::Moving { opening: true, .. }));
    assert!(p.layers()[1].is_none(), "the pod waits inside");
    run(&mut p, 2.5);
    assert_eq!(p.shown(), (Open, Out, Seated));
}

#[allow(non_snake_case)]
fn Unknown_lid() -> Lid {
    Lid::Unknown
}

#[test]
fn lid_closes_over_a_hovering_pod() {
    let mut p = CasePose::new(lens, st(Open, Out, Seated));
    p.set_target(st(Closed, Out, Seated));
    run(&mut p, 0.1);
    assert_eq!(p.layers()[0].unwrap().clip, LiveClip::LidClose(Baked::R));
    assert_eq!(
        p.layers()[1].unwrap().clip,
        LiveClip::Rise(Side::L),
        "left pod hovers"
    );
    run(&mut p, 0.5);
    assert_eq!(p.shown(), (Closed, Out, Seated));
}

#[test]
fn unknown_pods_keep_their_pose() {
    let mut p = CasePose::new(lens, st(Open, Out, Seated));
    p.set_target(st(Open, Unknown, Unknown));
    run(&mut p, 2.0);
    assert_eq!(p.shown(), (Open, Out, Seated));
}

#[test]
fn a_blip_shorter_than_settle_never_plays() {
    let mut p = CasePose::new(lens, st(Open, Seated, Seated));
    p.set_target(st(Open, Out, Seated));
    run(&mut p, SETTLE / 2.0);
    p.set_target(st(Open, Seated, Seated));
    run(&mut p, 1.0);
    assert!(p.layers()[1].is_none(), "nothing rose");
}

#[test]
fn two_pods_together_are_staggered() {
    let mut p = CasePose::new(lens, st(Open, Seated, Seated));
    p.set_target(st(Open, Out, Out));
    run(&mut p, SETTLE + 2.0 * DT);
    let n = p.layers()[1..].iter().flatten().count();
    assert_eq!(n, 1, "one pod first");
    run(&mut p, STAGGER + 2.0 * DT);
    assert_eq!(p.layers()[1..].iter().flatten().count(), 2);
}

#[test]
fn first_open_of_the_day_spins() {
    let mut p = CasePose::new(lens, st(Closed, Seated, Seated));
    p.spin_next = true;
    p.set_target(st(Open, Seated, Seated));
    run(&mut p, 0.1);
    assert_eq!(p.layers()[0].unwrap().clip, LiveClip::SpinLidOpen);
    run(&mut p, 1.5);
    assert_eq!(p.shown(), (Open, Seated, Seated));
}

#[test]
fn hidden_card_stops_the_clock_then_snaps() {
    let mut p = CasePose::new(lens, st(Open, Seated, Seated));
    p.set_target(st(Open, Out, Seated));
    p.tick(0.5, false);
    assert_eq!(
        p.shown(),
        (Open, Seated, Seated),
        "nothing plays off screen"
    );
    p.tick(SNAP_AFTER, false);
    assert_eq!(p.shown(), (Open, Out, Seated));
}

#[test]
fn reduce_motion_jumps() {
    let mut p = CasePose::new(lens, st(Closed, Seated, Seated));
    p.set_reduce_motion(true);
    p.set_target(st(Open, Out, Seated));
    p.tick(DT, true);
    assert_eq!(p.shown(), (Open, Out, Seated));
}

/// Thousands of random event streams with late, missing, repeated, contradictory and
/// out-of-order reports: the invariants hold every frame, and once the truth stops
/// changing the card always arrives at it (within the longest chain of clips).
#[test]
fn random_streams_always_converge() {
    let mut seed: u64 = 0x5eed_cafe;
    let mut rnd = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let lid = |r: u64| [Lid::Open, Lid::Closed, Lid::Unknown][r as usize];
    let pod = |r: u64| [Pod::Seated, Pod::Out, Pod::Unknown][r as usize];
    for _ in 0..3000 {
        let mut p = CasePose::new(lens, st(lid(rnd(3)), pod(rnd(3)), pod(rnd(3))));
        p.spin_next = rnd(2) == 0;
        for _ in 0..rnd(12) {
            p.set_target(st(lid(rnd(3)), pod(rnd(3)), pod(rnd(3))));
            let frames = rnd(300);
            for _ in 0..frames {
                let before = p.layers();
                p.tick(DT, rnd(50) != 0);
                check(&p);
                // No jumps: a layer that keeps its clip moves at most a frame or two.
                for (a, b) in before.iter().zip(p.layers().iter()) {
                    if let (Some(a), Some(b)) = (a, b)
                        && a.clip == b.clip
                    {
                        assert!(a.frame.abs_diff(b.frame) <= 2, "jump {a:?} -> {b:?}");
                    }
                }
            }
        }
        // A final truth; missed lid events are made up for (open, move, close).
        let fin = st(lid(rnd(2)), pod(rnd(2)), pod(rnd(2)));
        p.set_target(fin);
        run(&mut p, 6.0);
        let (l, a, b) = p.shown();
        assert_eq!((a, b), (fin.left, fin.right), "pods converge to {fin:?}");
        assert_eq!(l, fin.lid, "lid converges to {fin:?}");
        assert!(!p.moving(), "settled");
        let _ = p.clock();
    }
}
