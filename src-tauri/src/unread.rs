//! Unread counts per panel, as X itself shows them (panel.js reads the
//! badges on the 알림 / 쪽지 links, or the "(3)" in the page title). Nothing
//! is requested from X for this. The host UI shows them on the tabs and the
//! strip, the tray in its tooltip, a red "!" badge on the tray icon and the
//! taskbar button (Windows), the number on the Dock icon and next to the
//! menu bar icon (macOS). The badges can be turned off in settings.

use serde::Serialize;

/// One panel's counts as X shows them: "" none, "3", "20+".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Unread {
    /// 알림 (notifications).
    pub n: String,
    /// 쪽지 (direct messages).
    pub dm: String,
}

impl Unread {
    pub fn new(n: &str, dm: &str) -> Self {
        Self { n: clean(n), dm: clean(dm) }
    }

    pub fn is_empty(&self) -> bool {
        self.n.is_empty() && self.dm.is_empty()
    }

    /// "알림 3 · 쪽지 1", "" when nothing.
    pub fn text(&self) -> String {
        let mut parts = vec![];
        if !self.n.is_empty() {
            parts.push(format!("알림 {}", self.n));
        }
        if !self.dm.is_empty() {
            parts.push(format!("쪽지 {}", self.dm));
        }
        parts.join(" · ")
    }
}

/// Only 1-4 digits with an optional "+" (X shows "20+"); anything else,
/// and 0, is "nothing". The page sends this, so it is not trusted.
pub fn clean(s: &str) -> String {
    let s = s.trim();
    let digits = s.strip_suffix('+').unwrap_or(s);
    if digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return String::new();
    }
    let n: u32 = digits.parse().unwrap_or(0);
    if n == 0 {
        return String::new();
    }
    if s.ends_with('+') {
        format!("{n}+")
    } else {
        n.to_string()
    }
}

/// Sum of counts ("3" + "20+" = "23+"), "" when zero.
pub fn total<'a>(counts: impl Iterator<Item = &'a str>) -> String {
    let mut sum = 0u32;
    let mut plus = false;
    for c in counts {
        let d = c.strip_suffix('+');
        plus |= d.is_some();
        sum = sum.saturating_add(d.unwrap_or(c).parse().unwrap_or(0));
    }
    match (sum, plus) {
        (0, _) => String::new(),
        (n, true) => format!("{n}+"),
        (n, false) => n.to_string(),
    }
}

/// Where the "!" badge sits and how big it is, as fractions of the image.
struct Badge {
    /// Centre.
    cx: f64,
    cy: f64,
    /// Radius of the red disc.
    r: f64,
    /// Width of the white ring around it, so it stands out on the pink
    /// half of the icon (a plain red dot there was hard to see).
    ring: f64,
}

/// Is (x, y) inside the "!" of a badge centred at (0, 0) with radius r?
fn in_bang(x: f64, y: f64, r: f64) -> bool {
    // Stem: a rounded bar from the top to a little below the centre.
    let hw = r * 0.15;
    let (top, bottom) = (-r * 0.58, r * 0.16);
    let stem = if y < top + hw {
        x * x + (y - top - hw).powi(2) <= hw * hw
    } else if y > bottom - hw {
        x * x + (y - bottom + hw).powi(2) <= hw * hw
    } else {
        x.abs() <= hw
    };
    // Dot below it.
    let dr = r * 0.17;
    let dot = x * x + (y - r * 0.47).powi(2) <= dr * dr;
    stem || dot
}

/// Colour at (x, y) for a badge, None outside it.
fn badge_px(b: &Badge, x: f64, y: f64) -> Option<[u8; 3]> {
    let (dx, dy) = (x - b.cx, y - b.cy);
    let d2 = dx * dx + dy * dy;
    if d2 > (b.r + b.ring).powi(2) {
        None
    } else if d2 > b.r * b.r || in_bang(dx, dy, b.r) {
        Some([0xff, 0xff, 0xff])
    } else {
        Some([0xf4, 0x21, 0x2e])
    }
}

/// Paints the badge over `out` (RGBA, w x h), 4x4 samples per pixel so the
/// edges stay smooth at 16-32 px.
fn paint(out: &mut [u8], w: u32, h: u32, b: &Badge) {
    const N: u32 = 4;
    let x0 = ((b.cx - b.r - b.ring).floor().max(0.0)) as u32;
    let x1 = ((b.cx + b.r + b.ring).ceil() as u32).min(w);
    let y0 = ((b.cy - b.r - b.ring).floor().max(0.0)) as u32;
    let y1 = ((b.cy + b.r + b.ring).ceil() as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let (mut sum, mut hits) = ([0.0f64; 3], 0u32);
            for sy in 0..N {
                for sx in 0..N {
                    let fx = x as f64 + (sx as f64 + 0.5) / N as f64;
                    let fy = y as f64 + (sy as f64 + 0.5) / N as f64;
                    if let Some(c) = badge_px(b, fx, fy) {
                        for i in 0..3 {
                            sum[i] += c[i] as f64;
                        }
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let a = hits as f64 / (N * N) as f64;
            let c = sum.map(|v| v / hits as f64);
            let i = ((y * w + x) * 4) as usize;
            let px = &mut out[i..i + 4];
            let da = px[3] as f64 / 255.0;
            let oa = a + da * (1.0 - a);
            for k in 0..3 {
                let v = (c[k] * a + px[k] as f64 * da * (1.0 - a)) / oa;
                px[k] = v.round().clamp(0.0, 255.0) as u8;
            }
            px[3] = (oa * 255.0).round() as u8;
        }
    }
}

/// The tray icon with a red "!" badge in the bottom right corner (where
/// Windows puts taskbar badges too). `rgba` of a bad size is returned as is.
pub fn with_badge(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    if out.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return out;
    }
    let size = w.min(h) as f64;
    let ring = (size * 0.06).max(1.0);
    let r = size * 0.31;
    let b = Badge { cx: w as f64 - r - ring, cy: h as f64 - r - ring, r, ring };
    paint(&mut out, w, h, &b);
    out
}

/// Windows taskbar overlay: the badge alone on a transparent square
/// (Windows draws it over the bottom right of the app's taskbar button).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn overlay(size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let s = size as f64;
    let ring = (s * 0.06).max(1.0);
    let b = Badge { cx: s / 2.0, cy: s / 2.0, r: s / 2.0 - ring, ring };
    paint(&mut out, size, size, &b);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning() {
        assert_eq!(clean("3"), "3");
        assert_eq!(clean(" 20+ "), "20+");
        assert_eq!(clean("0"), "");
        assert_eq!(clean(""), "");
        assert_eq!(clean("abc"), "");
        assert_eq!(clean("12345"), "");
        assert_eq!(clean("<b>1</b>"), "");
        assert_eq!(clean("007"), "7");
    }

    #[test]
    fn totals_and_text() {
        assert_eq!(total(["3", "", "20+"].into_iter()), "23+");
        assert_eq!(total(["", ""].into_iter()), "");
        assert_eq!(total(["2", "1"].into_iter()), "3");
        assert_eq!(Unread::new("3", "1").text(), "알림 3 · 쪽지 1");
        assert_eq!(Unread::new("", "1").text(), "쪽지 1");
        assert!(Unread::new("0", "x").is_empty());
    }

    #[test]
    fn badge_in_the_corner() {
        let (w, h) = (32u32, 32u32);
        let base = vec![0u8; (w * h * 4) as usize];
        let d = with_badge(&base, w, h);
        let at = |x: u32, y: u32| d[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize].to_vec();
        // Left of the "!" inside the disc: opaque red.
        assert_eq!(at(w - 16, h - 12), vec![0xf4, 0x21, 0x2e, 255]);
        // The "!" stem in the middle of the badge: white.
        let (cx, cy) = ((w as f64 - 9.92 - 1.92) as u32, (h as f64 - 9.92 - 1.92) as u32);
        assert_eq!(at(cx, cy - 2), vec![0xff, 0xff, 0xff, 255]);
        // Top left corner: untouched.
        assert_eq!(at(0, 0), vec![0, 0, 0, 0]);
        // Bad size: returned as is.
        assert_eq!(with_badge(&[1, 2, 3], 4, 4), vec![1, 2, 3]);
    }

    #[test]
    fn overlay_is_a_badge_on_nothing() {
        let o = overlay(32);
        assert_eq!(o.len(), 32 * 32 * 4);
        assert_eq!(&o[0..4], &[0, 0, 0, 0]); // corner transparent
        let mid = ((16 * 32 + 16) * 4) as usize; // the "!" stem
        assert_eq!(&o[mid..mid + 4], &[0xff, 0xff, 0xff, 255]);
        let red = ((16 * 32 + 8) * 4) as usize;
        assert_eq!(&o[red..red + 4], &[0xf4, 0x21, 0x2e, 255]);
    }
}
