//! Odometer wheels for the window's battery numbers.

use super::*;

#[test]
fn settled_numbers_sit_on_their_digits() {
    assert_eq!(odometer(82.0, 2), vec![(2, 0.0), (8, 0.0)]);
    // 0.82 * 100 in floats is not quite 82.
    assert_eq!(odometer(0.82 * 100.0, 2), vec![(2, 0.0), (8, 0.0)]);
    assert_eq!(odometer(100.0, 3), vec![(0, 0.0), (0, 0.0), (1, 0.0)]);
}

#[test]
fn tens_only_turn_while_the_ones_roll_over_from_nine() {
    let w = odometer(25.5, 2);
    assert_eq!(w[0].0, 5);
    assert!((w[0].1 - 0.5).abs() < 1e-3);
    assert_eq!(w[1], (2, 0.0), "tens stay put mid-decade");
    let w = odometer(29.5, 2);
    assert_eq!(w[1].0, 2);
    assert!((w[1].1 - 0.5).abs() < 1e-3, "tens roll with the 9 -> 0");
}
