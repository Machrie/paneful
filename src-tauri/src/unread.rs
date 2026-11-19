//! Unread counts per panel, as X itself shows them (panel.js reads the
//! badges on the 알림 / 쪽지 links, or the "(3)" in the page title). Nothing
//! is requested from X for this. The host UI shows them on the tabs and the
//! strip, the tray in its tooltip, a red dot on the icon and (macOS) the
//! number next to the menu bar icon.

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

/// The tray icon with a red dot in the top right corner.
pub fn with_dot(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    if out.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return out;
    }
    let size = w.min(h) as f64;
    let r = size * 0.24; // red disc
    let ring = r + (size * 0.06).max(1.0); // dark edge so it reads on any icon
    let (cx, cy) = (w as f64 - ring, ring);
    let blend = |px: &mut [u8], c: [u8; 3], a: f64| {
        let a = a.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let da = px[3] as f64 / 255.0;
        let oa = a + da * (1.0 - a);
        for i in 0..3 {
            let v = (c[i] as f64 * a + px[i] as f64 * da * (1.0 - a)) / oa;
            px[i] = v.round().clamp(0.0, 255.0) as u8;
        }
        px[3] = (oa * 255.0).round() as u8;
    };
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * w + x) * 4) as usize;
            let px = &mut out[i..i + 4];
            blend(px, [0, 0, 0], ring + 0.5 - d);
            blend(px, [0xf4, 0x21, 0x2e], r + 0.5 - d);
        }
    }
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
    fn dot_is_red_in_the_corner() {
        let (w, h) = (32, 32);
        let base = vec![0u8; (w * h * 4) as usize];
        let d = with_dot(&base, w, h);
        let at = |x: u32, y: u32| &d[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        // near the dot's centre: opaque red
        assert_eq!(at(w - 10, 9), &[0xf4, 0x21, 0x2e, 255]);
        // bottom left corner: untouched
        assert_eq!(at(0, h - 1), &[0, 0, 0, 0]);
        // bad size: returned as is
        assert_eq!(with_dot(&[1, 2, 3], 4, 4), vec![1, 2, 3]);
    }
}
