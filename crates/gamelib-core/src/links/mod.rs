//! Links to non-Steam sources (other stores, official sites, direct downloads).
//!
//! The user adds links by hand. Every site redirects its links differently, so site-specific
//! behaviour lives behind [`SiteHandler`]: one handler per site recognises its URLs, cleans them
//! up and knows how to follow its redirects. Unknown sites fall back to
//! [`sites::generic::GenericSite`], which follows standard HTTP redirects only.
//!
//! Nothing here downloads or runs files: a check sends HEAD (or a 1-byte ranged GET) requests
//! and reads headers, and opening a link hands it to the user's browser.

pub mod registry;
pub mod resolve;
pub mod sites;
pub mod validate;

use reqwest::Url;
use reqwest::blocking::Client;

use crate::model::{LinkCheck, SiteInfo};

pub use registry::SiteRegistry;

pub trait SiteHandler: Send + Sync {
    fn info(&self) -> &SiteInfo;

    /// Whether this handler is responsible for `url`. By default: the host is one of
    /// `info().domains` or a subdomain of one.
    fn matches(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.info()
            .domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }

    /// Canonical form stored for a link. By default drops tracking parameters (`utm_*`, …).
    fn normalize(&self, url: Url) -> Url {
        validate::strip_tracking(url)
    }

    /// Follows the link to see where it ends up, without downloading it.
    fn resolve(&self, url: &Url, http: &Client) -> LinkCheck {
        resolve::follow_redirects(url, http)
    }
}
