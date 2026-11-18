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
}
