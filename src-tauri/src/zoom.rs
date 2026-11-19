//! Per-panel page zoom: the steps Ctrl/Cmd + / - walk through (the same
//! ones Chrome and Edge use, cut to 50%..200%).

pub const LEVELS: [f64; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
pub const MIN: f64 = 0.5;
pub const MAX: f64 = 2.0;

/// A stored or sent value, made safe to hand to the webview.
pub fn clamp(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(MIN, MAX)
    } else {
        1.0
    }
}

/// Next level up (`dir` > 0) or down (`dir` < 0) from `cur`; 0 = 100%.
/// A value between two levels (hand-edited settings.json) goes to the
/// nearest one in that direction.
pub fn step(cur: f64, dir: i32) -> f64 {
    let cur = clamp(cur);
    const EPS: f64 = 1e-6;
    if dir > 0 {
        LEVELS.iter().copied().find(|l| *l > cur + EPS).unwrap_or(MAX)
    } else if dir < 0 {
        LEVELS.iter().rev().copied().find(|l| *l < cur - EPS).unwrap_or(MIN)
    } else {
        1.0
    }
}

/// 0.9 -> 90, for the panel toast and the tab.
pub fn percent(z: f64) -> u32 {
    (z * 100.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps() {
        assert_eq!(step(1.0, 1), 1.1);
        assert_eq!(step(1.0, -1), 0.9);
        assert_eq!(step(0.5, -1), 0.5);
        assert_eq!(step(2.0, 1), 2.0);
        assert_eq!(step(0.85, 1), 0.9);
        assert_eq!(step(0.85, -1), 0.8);
        assert_eq!(step(0.7, 0), 1.0);
        assert_eq!(step(f64::NAN, 1), 1.1);
        assert_eq!(clamp(9.0), 2.0);
        assert_eq!(percent(0.67), 67);
    }
}
