//! Design tokens shared by the window and the island (`kase-overlay`): one palette and one
//! set of springs. The values and the reasons for them are in `docs/DESIGN.md`.

/// Apple's system colours, as 0xRRGGBB. The island is always dark.
pub mod color {
    pub const GREEN_LIGHT: u32 = 0x34C759;
    pub const GREEN_DARK: u32 = 0x30D158;
    pub const ORANGE_LIGHT: u32 = 0xFF8D28;
    pub const ORANGE_DARK: u32 = 0xFF9230;
    pub const RED_LIGHT: u32 = 0xFF383C;
    pub const RED_DARK: u32 = 0xFF4245;
    pub const BLUE_LIGHT: u32 = 0x0088FF;
    pub const BLUE_DARK: u32 = 0x0091FF;
    pub const GRAY: u32 = 0x8E8E93;

    /// 0xRRGGBB as 0..1 floats.
    pub const fn rgb(hex: u32) -> [f32; 3] {
        [
            ((hex >> 16) & 0xFF) as f32 / 255.0,
            ((hex >> 8) & 0xFF) as f32 / 255.0,
            (hex & 0xFF) as f32 / 255.0,
        ]
    }
}

/// A spring as Apple describes it: how long it takes and how much it bounces.
/// stiffness = (2π / duration)², damping = 4π (1 - bounce) / duration, mass 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub duration: f32,
    pub bounce: f32,
}

const fn spring(duration: f32, bounce: f32) -> Spring {
    Spring { duration, bounce }
}

/// Position and size changes with no overshoot.
pub const SMOOTH: Spring = spring(0.50, 0.0);
/// Battery rings filling.
pub const FILL: Spring = spring(0.80, 0.0);
/// Selection highlights, segmented thumbs, page moves.
pub const SNAPPY: Spring = spring(0.32, 0.15);
/// The island growing into a card or alert; the quick look opening.
pub const GROW: Spring = spring(0.50, 0.22);
/// The island folding back.
pub const SHRINK: Spring = spring(0.42, 0.04);
pub const FADE_IN: Spring = spring(0.26, 0.0);
pub const FADE_OUT: Spring = spring(0.30, 0.0);
/// A value changing inside an open card.
pub const POP: Spring = spring(0.40, 0.30);
/// Buttons pressed.
pub const PRESS: Spring = spring(0.26, 0.30);
/// "Not possible" (the AirPods mic unavailable).
pub const SHAKE: Spring = spring(0.22, 0.40);

/// Apple's advice: springs bounce at most this much.
pub const MAX_BOUNCE: f32 = 0.4;
/// Base-layer animations finish within this many seconds (the rich layer may run longer).
pub const MAX_BASE_ANIMATION: f32 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn springs_follow_apples_limits() {
        for s in [
            SMOOTH, FILL, SNAPPY, GROW, SHRINK, FADE_IN, FADE_OUT, POP, PRESS, SHAKE,
        ] {
            assert!(s.bounce <= MAX_BOUNCE, "{s:?} bounces too much");
            assert!(s.duration <= MAX_BASE_ANIMATION, "{s:?} is too slow");
        }
    }

    #[test]
    fn hex_to_floats() {
        assert_eq!(color::rgb(0xFF0000), [1.0, 0.0, 0.0]);
    }
}
