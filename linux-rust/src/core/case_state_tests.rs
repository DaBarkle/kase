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
    t.on_link_lost();
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
        ..Default::default()
    };
    assert!(
        t.on_advert(&adv, now),
        "the advert opens the lid before the link is up"
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
    assert_eq!(pods(&t), (Lid::Unknown, Pod::Out, Pod::Seated));
}

#[test]
fn matching_pods_need_no_order() {
    let mut t = CaseTracker::default();
    assert!(t.on_ear(InCase, InCase, Instant::now()));
    assert_eq!(pods(&t), (Lid::Unknown, Pod::Seated, Pod::Seated));
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

#[test]
fn adverts_fill_in_when_ear_reports_are_quiet() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_battery(&both_charging(), now);
    t.on_ear(InCase, InCase, now);
    let adv = BleBatteryReading {
        left_in_case: false,
        right_in_case: true,
        lid_open: Some(true),
        ..Default::default()
    };
    assert!(
        t.on_advert(&adv, now + Duration::from_secs(1)),
        "the lid bit still counts"
    );
    assert_eq!(
        pods(&t),
        (Lid::Open, Pod::Seated, Pod::Seated),
        "the ear report keeps the pods"
    );
    assert!(t.on_advert(&adv, now + ADVERT_HOLD_OFF));
    assert_eq!(pods(&t), (Lid::Open, Pod::Out, Pod::Seated));
    assert_eq!(t.state().source, CaseSource::Advert);
}

#[test]
fn link_lost_with_both_in_means_the_lid_shut() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InCase, InCase, now);
    assert!(t.on_link_lost());
    assert_eq!(pods(&t), (Lid::Closed, Pod::Seated, Pod::Seated));
}

#[test]
fn link_lost_while_worn_forgets_the_pods() {
    let now = Instant::now();
    let mut t = CaseTracker::default();
    t.on_ear(InEar, InEar, now);
    t.on_link_lost();
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
    t.on_link_lost();
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
            Some("linklost") => changes += usize::from(t.on_link_lost()),
            _ => {}
        }
    }
    eprintln!("replay: {checked} pod readings checked, {wrong} wrong, {changes} state changes");
    assert!(checked > 1000, "the replay should cover the log");
    assert_eq!(wrong, 0);
}
