//! Navigation allow-list for x.com panels (filter layer 1).
//! Anything not allowed is cancelled in the panel and opened in the
//! system browser instead.

use url::Url;

/// Hosts (and their subdomains) a panel may navigate to.
const ALLOWED: &[&str] = &[
    "x.com",
    "twitter.com",
    "twimg.com",
    // login / challenge flows observed on x.com (추론: list to be confirmed in M0)
    "arkoselabs.com",
    "google.com",
    "gstatic.com",
    "apple.com",
];

/// Sign-in popups that must open as real popup windows: the provider page
/// reports back to x.com through `window.opener`, which only exists in a
/// real popup (추론: confirm with "Google/Apple로 로그인" in M0).
/// Exact hosts only, https only.
const AUTH_POPUPS: &[&str] = &["accounts.google.com", "appleid.apple.com"];

pub fn is_auth_popup(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
            .is_some_and(|h| AUTH_POPUPS.contains(&h.as_str()))
}

pub enum Decision {
    Allow,
    OpenExternal,
    Block,
}

pub fn decide(url: &Url) -> Decision {
    match url.scheme() {
        "https" => {}
        "about" | "blob" | "data" => return Decision::Allow,
        "http" | "mailto" => return Decision::OpenExternal,
        _ => return Decision::Block,
    }
    let Some(host) = url.host_str() else {
        return Decision::Block;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if ALLOWED
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        Decision::Allow
    } else {
        Decision::OpenExternal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> &'static str {
        match decide(&Url::parse(s).unwrap()) {
            Decision::Allow => "allow",
            Decision::OpenExternal => "external",
            Decision::Block => "block",
        }
    }

    #[test]
    fn rules() {
        assert_eq!(d("https://x.com/home"), "allow");
        assert_eq!(d("https://pbs.twimg.com/a.jpg"), "allow");
        assert_eq!(d("https://t.co/abc"), "external");
        assert_eq!(d("https://evilx.com/"), "external");
        assert_eq!(d("https://x.com.evil.io/"), "external");
        assert_eq!(d("file:///etc/passwd"), "block");
        assert_eq!(d("about:blank"), "allow");
    }

    #[test]
    fn auth_popups() {
        let p = |s: &str| is_auth_popup(&Url::parse(s).unwrap());
        assert!(p("https://accounts.google.com/o/oauth2/auth?x=1"));
        assert!(p("https://appleid.apple.com/auth/authorize"));
        assert!(!p("http://accounts.google.com/"));
        assert!(!p("https://evil.accounts.google.com/"));
        assert!(!p("https://x.com/"));
    }
}
