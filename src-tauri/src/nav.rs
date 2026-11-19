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

/// Hosts whose links a pasted link may point at. All open as x.com.
/// fxtwitter/vxtwitter/fixupx/fixvx: embed-fixing mirrors people share
/// instead of the real link; same paths as x.com.
const LINK_HOSTS: &[&str] = &[
    "x.com",
    "twitter.com",
    "mobile.x.com",
    "mobile.twitter.com",
    "m.twitter.com",
    "fxtwitter.com",
    "vxtwitter.com",
    "fixupx.com",
    "fixvx.com",
];

/// Share-link tracking parameters, dropped from a pasted link.
fn is_tracking(key: &str) -> bool {
    matches!(key, "s" | "t" | "ref_src" | "ref_url") || key.starts_with("utm_")
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkError {
    /// Not a link at all (plain text, image, empty): ignore silently.
    NotLink,
    /// A link, but not to X.
    OtherSite,
    /// t.co short link: where it leads is only known after a request.
    ShortLink,
}

/// Clipboard text -> the x.com page to open. Only a single link counts
/// (surrounding whitespace allowed), with or without `https://`.
pub fn x_link(text: &str) -> Result<Url, LinkError> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) || t.len() > 4096 {
        return Err(LinkError::NotLink);
    }
    let lower = t.to_ascii_lowercase();
    let with_scheme = if lower.starts_with("https://") || lower.starts_with("http://") {
        t.to_string()
    } else if lower.contains("://") {
        return Err(LinkError::NotLink);
    } else {
        // "x.com/foo" as copied from some apps. Bare words ("hello") have
        // no dot in the host part and are not links.
        let host_part = lower.split(['/', '?', '#']).next().unwrap_or("");
        if !host_part.contains('.') {
            return Err(LinkError::NotLink);
        }
        format!("https://{t}")
    };
    let Ok(mut url) = Url::parse(&with_scheme) else {
        return Err(LinkError::NotLink);
    };
    let Some(host) = url.host_str() else {
        return Err(LinkError::NotLink);
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host == "t.co" {
        return Err(LinkError::ShortLink);
    }
    if !LINK_HOSTS.contains(&host) {
        return Err(LinkError::OtherSite);
    }
    // No user:pass@ or odd ports on what we open.
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(LinkError::OtherSite);
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let _ = url.set_scheme("https");
    let _ = url.set_host(Some("x.com"));
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    Ok(url)
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
    #[test]
    fn pasted_links() {
        let ok = |s: &str| x_link(s).map(|u| u.to_string());
        assert_eq!(ok("https://x.com/jack/status/20").unwrap(), "https://x.com/jack/status/20");
        assert_eq!(
            ok("  https://twitter.com/jack/status/20?s=20&t=AbC_d  \n").unwrap(),
            "https://x.com/jack/status/20"
        );
        assert_eq!(ok("https://mobile.twitter.com/jack").unwrap(), "https://x.com/jack");
        assert_eq!(ok("http://www.x.com/jack?ref_src=twsrc%5Etfw").unwrap(), "https://x.com/jack");
        assert_eq!(ok("x.com/jack/status/20?s=46").unwrap(), "https://x.com/jack/status/20");
        assert_eq!(ok("HTTPS://X.COM/Jack").unwrap(), "https://x.com/Jack");
        assert_eq!(ok("https://fxtwitter.com/jack/status/20/photo/1").unwrap(), "https://x.com/jack/status/20/photo/1");
        assert_eq!(
            ok("https://x.com/search?q=%23rust&src=typed_query&utm_source=a").unwrap(),
            "https://x.com/search?q=%23rust&src=typed_query"
        );
        assert_eq!(ok("https://x.com/jack#top").unwrap(), "https://x.com/jack#top");

        assert_eq!(x_link("hello"), Err(LinkError::NotLink));
        assert_eq!(x_link(""), Err(LinkError::NotLink));
        assert_eq!(x_link("look https://x.com/jack"), Err(LinkError::NotLink));
        assert_eq!(x_link("ftp://x.com/jack"), Err(LinkError::NotLink));
        assert_eq!(x_link("javascript:alert(1)"), Err(LinkError::NotLink));
        assert_eq!(x_link("https://t.co/abc"), Err(LinkError::ShortLink));
        assert_eq!(x_link("https://example.com/"), Err(LinkError::OtherSite));
        assert_eq!(x_link("https://x.com.evil.io/"), Err(LinkError::OtherSite));
        assert_eq!(x_link("https://evilx.com/"), Err(LinkError::OtherSite));
        assert_eq!(x_link("https://pbs.twimg.com/media/a.jpg"), Err(LinkError::OtherSite));
        assert_eq!(x_link("https://user:pw@x.com/"), Err(LinkError::OtherSite));
        assert_eq!(x_link("https://x.com:8443/"), Err(LinkError::OtherSite));
        assert_eq!(x_link("youtube.com/watch?v=1"), Err(LinkError::OtherSite));
    }
}
