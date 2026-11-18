//! Pure layout math: window size + ratios -> panel rects (logical px).

use serde::Serialize;

pub const TOP_BAR: f64 = 36.0;
/// Gap between panels. The host webview underneath shows through and
/// receives the drag events for resizing.
pub const GAP: f64 = 6.0;
pub const MIN_PANEL: f64 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn panels(win_w: f64, win_h: f64, ratios: &[f64]) -> Vec<Rect> {
    let n = ratios.len();
    if n == 0 {
        return vec![];
    }
    let h = (win_h - TOP_BAR).max(0.0);
    let usable = (win_w - GAP * (n as f64 - 1.0)).max(0.0);
    let sum: f64 = ratios.iter().sum();
    let mut x = 0.0;
    let mut out = Vec::with_capacity(n);
    for (i, r) in ratios.iter().enumerate() {
        // last panel takes the remainder so rounding never leaves a sliver
        let w = if i + 1 == n {
            (win_w - x).max(0.0)
        } else {
            (usable * r / sum).floor()
        };
        out.push(Rect { x, y: TOP_BAR, w, h });
        x += w + GAP;
    }
    out
}

/// Clamp ratios so no panel is narrower than MIN_PANEL when the window
/// is wide enough for that. Returns normalised ratios.
pub fn clamp_ratios(win_w: f64, ratios: &[f64]) -> Vec<f64> {
    let n = ratios.len();
    if n == 0 {
        return vec![];
    }
    let usable = (win_w - GAP * (n as f64 - 1.0)).max(1.0);
    let min = (MIN_PANEL / usable).min(1.0 / n as f64);
    let sum: f64 = ratios.iter().sum();
    let mut r: Vec<f64> = ratios.iter().map(|v| v / sum).collect();
    // Pin undersized panels to `min`, shrink the rest proportionally.
    // Each pass pins at least one more panel or finishes, so n passes suffice.
    let mut pinned = vec![false; n];
    for _ in 0..n {
        let mut changed = false;
        for i in 0..n {
            if !pinned[i] && r[i] < min {
                pinned[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        let free: f64 = (0..n).filter(|&i| !pinned[i]).map(|i| r[i]).sum();
        let budget = 1.0 - min * pinned.iter().filter(|p| **p).count() as f64;
        for i in 0..n {
            r[i] = if pinned[i] { min } else { r[i] / free * budget };
        }
    }
    r
}

/// Ratios after appending one panel: the new panel gets an equal share
/// (1/(n+1)), existing panels keep their proportions to each other.
pub fn with_added(ratios: &[f64]) -> Vec<f64> {
    let n = ratios.len() as f64;
    let sum: f64 = ratios.iter().sum();
    let mut r: Vec<f64> = if sum > 0.0 {
        ratios.iter().map(|v| v / sum * n / (n + 1.0)).collect()
    } else {
        vec![1.0 / (n + 1.0); ratios.len()]
    };
    r.push(1.0 / (n + 1.0));
    r
}

/// Ratios after removing panel `idx`: the others keep their proportions.
pub fn with_removed(ratios: &[f64], idx: usize) -> Vec<f64> {
    let mut r = ratios.to_vec();
    if idx < r.len() {
        r.remove(idx);
    }
    let sum: f64 = r.iter().sum();
    if sum > 0.0 {
        for v in &mut r {
            *v /= sum;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn add_keeps_proportions() {
        assert!(close(&with_added(&[0.75, 0.25]), &[0.5, 1.0 / 6.0, 1.0 / 3.0]));
        assert!(close(&with_added(&[]), &[1.0]));
    }

    #[test]
    fn remove_keeps_proportions() {
        assert!(close(&with_removed(&[0.5, 0.3, 0.2], 0), &[0.6, 0.4]));
        assert!(close(&with_removed(&[1.0], 0), &[]));
    }

    #[test]
    fn two_equal() {
        let p = panels(1206.0, 836.0, &[0.5, 0.5]);
        assert_eq!(p[0], Rect { x: 0.0, y: TOP_BAR, w: 600.0, h: 800.0 });
        assert_eq!(p[1].x, 606.0);
        assert_eq!(p[1].x + p[1].w, 1206.0);
    }

    #[test]
    fn clamp_keeps_min() {
        let r = clamp_ratios(1206.0, &[0.95, 0.05]);
        let p = panels(1206.0, 800.0, &r);
        assert!(p[1].w >= MIN_PANEL - 1.0);
    }
}
