use super::*;
use crate::bluetooth::aacp::BatteryStatus;
use EarDetectionStatus::{Disconnected, InCase, InEar, OutOfEar};

fn bat(order: &[(BatteryComponent, BatteryStatus)]) -> Vec<BatteryInfo> {
    order
        .iter()
        .map(|&(component, status)| BatteryInfo {
            component,
            level: 50,
            status,
        })
        .collect()
}

fn lr_first(left_first: bool) -> Vec<BatteryInfo> {
    use BatteryComponent::{Case, Left, Right};
    use BatteryStatus::NotCharging as N;
    if left_first {
        bat(&[(Left, N), (Right, N), (Case, N)])
    } else {
        bat(&[(Right, N), (Left, N), (Case, N)])
    }
}

fn both_charging() -> Vec<BatteryInfo> {
    use BatteryComponent::{Case, Left, Right};
    use BatteryStatus::{Charging as C, NotCharging as N};
    bat(&[(Left, C), (Right, C), (Case, N)])
}

/// Some connections start with a battery report and send no ear reports for a while
/// (24 of 67 on David's PC): the Charging flags place the pods meanwhile.
#[test]
fn battery_alone_places_pods_until_ear_reports_come() {
    use BatteryComponent::{Case, Left, Right};
    use BatteryStatus::{Charging as C, NotCharging as N};
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InCase, InCase, now);
    t.on_link_lost(now);
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
    assert!(t.on_battery(&bat(&[(Right, C), (Left, N), (Case, N)]), now));
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Out, Pod::Seated),
        "left came out while away"
    );
    // Once ear reports flow, charging flags no longer move pods (a flat case stops charging).
    t.on_ear(InCase, OutOfEar, now);
    assert!(!t.on_battery(&bat(&[(Right, N), (Left, N), (Case, N)]), now));
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
}

fn pods(t: &CaseTracker) -> (Lid, Pod, Pod) {
    let s = t.state();
    (s.lid, s.left, s.right)
}

/// 1 Nov 14:54:40-14:54:52 UTC on David's PC: the right pod goes into the case, the
/// primary swaps to the left pod (its ear report arrives before the battery report that
/// says so), then the left pod goes in and the lid shuts.
#[test]
fn real_put_away_with_primary_swap() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_battery(&lr_first(false), now);
    t.on_ear(InEar, InEar, now);
    assert_eq!(pods(&t), (Lid::Unknown, Pod::Out, Pod::Out));

    t.on_ear(OutOfEar, InEar, now);
    assert_eq!(pods(&t), (Lid::Unknown, Pod::Out, Pod::Out));

    assert!(t.on_ear(InCase, InEar, now));
    t.on_battery(&lr_first(false), now);
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Out, Pod::Seated),
        "right pod went in"
    );

    // The primary swaps: the report is the old one reversed, before the battery order.
    assert!(!t.on_ear(InEar, InCase, now), "a swap alone moves nothing");
    t.on_battery(&lr_first(true), now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));

    t.on_ear(OutOfEar, InCase, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
    t.on_ear(InCase, InCase, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Seated));

    // Lid shut: the secondary pod drops off.
    assert!(t.on_ear(InCase, Disconnected, now));
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
}

/// 1 Nov 14:55:04-14:55:12 UTC: the lid opens with both pods in, the link comes up, the
/// right pod comes out, then the left.
#[test]
fn real_lid_open_and_pods_out() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    let mut adv = BleBatteryReading {
        left_in_case: true,
        right_in_case: true,
        lid_open: Some(true),
        lid_trusted: true,
        ..Default::default()
    };
    assert!(!t.on_advert(&adv, now), "one advert alone moves nothing");
    assert!(
        t.on_advert(&adv, now + Duration::from_millis(200)),
        "two adverts open the lid before the link is up"
    );
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Seated));

    let now = now + Duration::from_secs(1);
    t.on_battery(&both_charging(), now);
    assert!(!t.on_ear(InCase, InCase, now));
    assert!(t.on_ear(InCase, OutOfEar, now));
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Seated, Pod::Out),
        "right pod out"
    );

    // A stale advert right after the ear report changes nothing.
    adv.lid_open = Some(true);
    assert!(!t.on_advert(&adv, now + Duration::from_millis(500)));

    t.on_ear(InCase, InEar, now);
    t.on_ear(OutOfEar, InEar, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Out));
}

#[test]
fn ear_reports_wait_for_the_battery_order() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    assert!(
        !t.on_ear(InCase, OutOfEar, now),
        "can't tell left from right yet"
    );
    assert!(t.on_battery(&lr_first(false), now));
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
}

#[test]
fn matching_pods_need_no_order() {
    let mut t = CaseTracker::default();
    assert!(t.on_ear(InCase, InCase, Instant::now()));
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Seated, Pod::Seated),
        "pods in a shut case don't talk to the PC"
    );
}

#[test]
fn lid_opens_when_a_pod_comes_back_online() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_battery(&lr_first(true), now);
    t.on_ear(InEar, InCase, now);
    t.on_ear(InEar, Disconnected, now);
    assert_eq!(
        pods(&t),
        (Lid::Closed, Pod::Out, Pod::Seated),
        "shut on the right pod"
    );
    t.on_ear(InEar, InCase, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
}

fn advert(
    left_in_case: bool,
    right_in_case: bool,
    lid_open: bool,
    trusted: bool,
) -> BleBatteryReading {
    BleBatteryReading {
        left_in_case,
        right_in_case,
        lid_open: Some(lid_open),
        lid_trusted: trusted,
        ..Default::default()
    }
}

#[test]
fn adverts_fill_in_when_ear_reports_are_quiet() {
    let now = Instant::now();
    let ms = |n| now + Duration::from_millis(n);
    let mut t = CaseTracker::default();
    t.on_battery(&both_charging(), now);
    t.on_ear(InCase, InCase, now);
    let adv = advert(false, true, true, true);
    t.on_advert(&adv, ms(1000));
    t.on_advert(&adv, ms(1200));
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Seated, Pod::Seated),
        "the ear report keeps the pods"
    );
    assert!(t.on_advert(&adv, now + ADVERT_HOLD_OFF));
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
    assert_eq!(t.state().source, CaseSource::Advert);
}

/// 8 Nov: the lid shut while ear reports were still repeating; the adverts said so ~4 s
/// before the AirPods did. It must close at once, not after the ear reports go quiet.
#[test]
fn the_lid_shuts_from_adverts_while_ear_reports_flow() {
    let now = Instant::now();
    let ms = |n| now + Duration::from_millis(n);
    let mut t = CaseTracker::default();
    t.on_battery(&both_charging(), now);
    t.on_ear(InCase, InCase, now);
    t.on_advert(&advert(true, true, true, true), ms(100));
    t.on_advert(&advert(true, true, true, true), ms(200));
    t.on_ear(InCase, InCase, ms(300));
    // The two pods disagree for a moment as the lid moves.
    assert!(!t.on_advert(&advert(true, true, false, true), ms(400)));
    assert!(!t.on_advert(&advert(true, true, true, true), ms(410)));
    assert!(!t.on_advert(&advert(true, true, false, true), ms(560)));
    assert!(t.on_advert(&advert(true, true, false, true), ms(700)));
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
}

/// A pod in a hand keeps repeating the lid it last heard; only a pod in the case counts.
#[test]
fn a_pod_out_of_the_case_has_no_say_about_the_lid() {
    let now = Instant::now();
    let ms = |n| now + Duration::from_millis(n);
    let mut t = CaseTracker::default();
    for n in [0, 100] {
        t.on_advert(&advert(true, false, true, true), ms(n));
    }
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Out));
    for n in [200, 300] {
        t.on_advert(&advert(true, false, false, true), ms(n));
    }
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Out));
    // The pod in the hand still says "open", and with the lid shut nobody moves pods.
    for n in [400, 500, 600] {
        t.on_advert(&advert(false, false, true, false), ms(n));
    }
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Out));
}

#[test]
fn a_link_drop_with_the_lid_seen_open_is_not_a_close() {
    let now = Instant::now();
    let ms = |n| now + Duration::from_millis(n);
    let mut t = CaseTracker::default();
    t.on_ear(InCase, InCase, now);
    t.on_advert(&advert(true, true, true, true), ms(100));
    t.on_advert(&advert(true, true, true, true), ms(200));
    t.on_link_lost(ms(1000));
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Seated));
    // Without a fresh advert, a drop with both pods in still means the lid shut.
    t.on_ear(InCase, InCase, ms(10_000));
    t.on_link_lost(ms(10_000));
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
}

#[test]
fn link_lost_with_both_in_means_the_lid_shut() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InCase, InCase, now);
    assert!(t.on_link_lost(now));
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
}

#[test]
fn link_lost_while_worn_forgets_the_pods() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InEar, InEar, now);
    t.on_link_lost(now);
    assert_eq!(pods(&t), (Lid::Unknown, Pod::Unknown, Pod::Unknown));
}

#[test]
fn seq_counts_changes_only() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InCase, InCase, now);
    let seq = t.state().seq;
    t.on_ear(InCase, InCase, now);
    assert_eq!(t.state().seq, seq);
    t.on_link_lost(now);
    assert_eq!(t.state().seq, seq + 1);
}

/// Replays every ear and battery report from David's PC, 29 Oct to 1 Nov 2026 (extracted
/// from kase.log; no addresses), and checks each pod's mapped place against its Charging
/// flag in the battery report that follows: a pod in the case charges, one out doesn't.
#[test]
fn replay_of_real_reports_maps_every_pod_correctly() {
    let text = include_str!("../../tests/fixtures/case-events-2026-11-01.txt");
    let status = |s: &str| match s {
        "InEar" => InEar,
        "OutOfEar" => OutOfEar,
        "InCase" => InCase,
        _ => Disconnected,
    };
    let mut t = CaseTracker::default();
    let now = Instant::now();
    let (mut checked, mut wrong, mut changes) = (0, 0, 0);
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let (_time, kind) = (f.next(), f.next());
        match kind {
            Some("ear") => {
                let (p, s) = (f.next().unwrap(), f.next().unwrap());
                changes += usize::from(t.on_ear(status(p), status(s), now));
            }
            Some("bat") => {
                let mut info = Vec::new();
                for c in f {
                    let (comp, st) = c.split_once(':').unwrap();
                    let component = match comp {
                        "Left" => BatteryComponent::Left,
                        "Right" => BatteryComponent::Right,
                        "Case" => BatteryComponent::Case,
                        _ => BatteryComponent::Headphone,
                    };
                    let status = match st {
                        "Charging" => BatteryStatus::Charging,
                        "NotCharging" => BatteryStatus::NotCharging,
                        "Disconnected" => BatteryStatus::Disconnected,
                        _ => BatteryStatus::Unknown,
                    };
                    if info.iter().any(|i: &BatteryInfo| i.component == component) {
                        break; // the log line repeats the list
                    }
                    info.push(BatteryInfo {
                        component,
                        level: 50,
                        status,
                    });
                }
                changes += usize::from(t.on_battery(&info, now));
                let s = t.state();
                for i in &info {
                    let pod = match i.component {
                        BatteryComponent::Left => s.left,
                        BatteryComponent::Right => s.right,
                        _ => continue,
                    };
                    let charging = match i.status {
                        BatteryStatus::Charging => true,
                        BatteryStatus::NotCharging => false,
                        _ => continue,
                    };
                    if pod == Pod::Unknown {
                        continue;
                    }
                    checked += 1;
                    if (pod == Pod::Seated) != charging {
                        wrong += 1;
                        eprintln!("mismatch at {line}: {:?}", s);
                    }
                }
            }
            Some("linklost") => changes += usize::from(t.on_link_lost(now)),
            _ => {}
        }
    }
    eprintln!("replay: {checked} pod readings checked, {wrong} wrong, {changes} state changes");
    assert!(checked > 1000, "the replay should cover the log");
    assert_eq!(wrong, 0);
}

/// David's test on 1 Nov 2026 at 22:49, as the battery service gets it (each packet
/// merged into the running list first). The right pod went back in, but a stale order
/// in the merged list put the left one in.
#[test]
fn real_right_pod_back_in_through_the_merged_list() {
    use crate::bluetooth::aacp::merge_battery_info;
    use BatteryComponent::{Case, Left, Right};
    use BatteryStatus::{Charging as C, Disconnected as D, NotCharging as N};
    let mut t = CaseTracker::default();
    let mut merged = Vec::new();
    let now = Instant::now();
    let mut battery = |t: &mut CaseTracker, raw: Vec<BatteryInfo>| {
        merge_battery_info(&mut merged, &raw);
        t.on_battery(&merged, now);
    };
    // Lid open, right pod taken out first (right is primary).
    battery(&mut t, bat(&[(Right, N), (Left, C), (Case, N)]));
    t.on_ear(InEar, InCase, now);
    assert_eq!((pods(&t).1, pods(&t).2), (Pod::Seated, Pod::Out));
    t.on_ear(InEar, OutOfEar, now);
    battery(&mut t, bat(&[(Right, N), (Left, N), (Case, D)]));
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Out));
    // Both worn; the left becomes primary.
    t.on_ear(InEar, InEar, now);
    t.on_ear(OutOfEar, InEar, now);
    t.on_ear(InEar, OutOfEar, now);
    battery(&mut t, bat(&[(Left, N), (Right, N), (Case, D)]));
    t.on_ear(InEar, OutOfEar, now);
    // The right pod goes back in.
    battery(&mut t, bat(&[(Left, N), (Right, N), (Case, D)]));
    t.on_ear(InEar, InCase, now);
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Out, Pod::Seated),
        "the right pod went in"
    );
}

/// The same evening at 22:34: right out, then the right goes into an ear while the left
/// stays in the case, with the primary swapping on the way.
#[test]
fn real_right_out_with_a_primary_swap_through_the_merged_list() {
    use crate::bluetooth::aacp::merge_battery_info;
    use BatteryComponent::{Case, Left, Right};
    use BatteryStatus::{Charging as C, NotCharging as N};
    let mut t = CaseTracker::default();
    let mut merged = Vec::new();
    let now = Instant::now();
    let mut battery = |t: &mut CaseTracker, raw: Vec<BatteryInfo>| {
        merge_battery_info(&mut merged, &raw);
        t.on_battery(&merged, now);
    };
    battery(&mut t, bat(&[(Left, C), (Right, C), (Case, N)]));
    t.on_ear(InCase, InCase, now);
    t.on_ear(InCase, OutOfEar, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Out));
    battery(&mut t, bat(&[(Left, C), (Right, N), (Case, N)]));
    t.on_ear(OutOfEar, InCase, now);
    battery(&mut t, bat(&[(Right, N), (Left, C), (Case, N)]));
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Seated, Pod::Out),
        "nothing moved"
    );
    t.on_ear(InEar, InCase, now);
    assert_eq!(pods(&t), (Lid::Open, Pod::Seated, Pod::Out));
}

/// Replays David's guided test on 8 Nov 2026 (btmon capture: both pods' adverts and every
/// ear and battery report; 5 rounds, 7 lid closes, 8 pod moves). Before the fix Kase moved
/// a pod to the wrong side 14 times and heard the lid shut 3.5 to 4.2 s late.
#[test]
fn replay_of_the_8_nov_lid_test() {
    use crate::bluetooth::aacp::merge_battery_info;
    use crate::bluetooth::le::decode_proximity;
    let text = include_str!("../../tests/fixtures/lid-test-2026-11-08.txt");
    let status = |s: &str| match s {
        "InEar" => InEar,
        "OutOfEar" => OutOfEar,
        "InCase" => InCase,
        _ => Disconnected,
    };
    let t0 = Instant::now();
    let mut t = CaseTracker::default();
    let mut merged = Vec::new();
    // What David had done by each step, and the steps themselves.
    let mut want = (Lid::Closed, Pod::Seated, Pod::Seated);
    let mut steps = vec![(0u64, want)];
    let mut changes = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let ms: u64 = f[0].parse().unwrap();
        let at = t0 + Duration::from_millis(ms);
        let changed = match f[1] {
            "adv" => {
                let st = u8::from_str_radix(f[2], 16).unwrap();
                let lid = u8::from_str_radix(f[3], 16).unwrap();
                let data = [0x07, 0x19, 0x01, 0x1B, 0x20, st, 0, 0, lid, 0, 0];
                let reading = decode_proximity(&data, None).unwrap();
                t.on_advert(&reading, at)
            }
            "ear" => t.on_ear(status(f[2]), status(f[3]), at),
            "bat" => {
                let info: Vec<BatteryInfo> = f[2..]
                    .iter()
                    .map(|c| {
                        let (comp, st) = c.split_once(':').unwrap();
                        BatteryInfo {
                            component: match comp {
                                "Left" => BatteryComponent::Left,
                                "Right" => BatteryComponent::Right,
                                "Case" => BatteryComponent::Case,
                                _ => BatteryComponent::Headphone,
                            },
                            level: 50,
                            status: match st {
                                "Charging" => BatteryStatus::Charging,
                                "NotCharging" => BatteryStatus::NotCharging,
                                "Disconnected" => BatteryStatus::Disconnected,
                                _ => BatteryStatus::Unknown,
                            },
                        }
                    })
                    .collect();
                merge_battery_info(&mut merged, &info);
                t.on_battery(&merged, at)
            }
            "linklost" => t.on_link_lost(at),
            "step" => {
                match f[2] {
                    "o" => want.0 = Lid::Open,
                    "c" => want.0 = Lid::Closed,
                    "r" => want.2 = Pod::Out,
                    "l" => want.1 = Pod::Out,
                    "R" => want.2 = Pod::Seated,
                    "L" => want.1 = Pod::Seated,
                    _ => {}
                }
                steps.push((ms, want));
                false
            }
            _ => false,
        };
        if changed {
            changes.push((ms, pods(&t)));
        }
    }

    // Every pod Kase placed matches what David had done just before or just after.
    let around = |ms: u64| {
        let i = steps.iter().rposition(|(s, _)| *s <= ms).unwrap();
        // Steps done in one go share a time: take the last of them.
        let next = steps.get(i + 1).map_or(steps[i], |n| {
            *steps.iter().rev().find(|s| s.0 == n.0).unwrap()
        });
        (steps[i].1, next.1)
    };
    let mut wrong = 0;
    for &(ms, (_, l, r)) in &changes {
        let (a, b) = around(ms);
        let fits = |got: Pod, x: Pod, y: Pod| got == Pod::Unknown || got == x || got == y;
        if !fits(l, a.1, b.1) || !fits(r, a.2, b.2) {
            wrong += 1;
            eprintln!("wrong pod at {ms} ms: L {l:?} R {r:?} (around {a:?} / {b:?})");
        }
    }
    assert_eq!(wrong, 0, "no pod on the wrong side");

    // The lid reads shut by the time David pressed Enter after closing it (the adverts
    // said so 0.4 to 1 s before), and open halfway to his next step (the AirPods took up
    // to 2.8 s after his Enter to say the lid opened, and he sometimes shut it again
    // before his next Enter).
    let lid_at = |ms: u64| {
        changes
            .iter()
            .rev()
            .find(|(c, _)| *c <= ms)
            .map_or(Lid::Unknown, |(_, s)| s.0)
    };
    let mut closes = 0;
    for (i, w) in steps.windows(2).enumerate() {
        let (ms, after) = w[1];
        if after.0 != w[0].1.0 {
            let check = match (after.0, steps.get(i + 2)) {
                (Lid::Open, Some(&(next, _))) => (ms + next) / 2,
                _ => ms,
            };
            assert_eq!(lid_at(check), after.0, "lid at {check} ms");
            closes += usize::from(after.0 == Lid::Closed);
        }
    }
    assert_eq!(closes, 7);
}
