//! Per-site link handlers.
//!
//! Each download site structures and redirects its links differently, so every site gets its
//! own hand-written handler. A handler recognises that site's URLs and may also know how to
//! *find* a game on it. To add one:
//!
//! 1. Create `sites/<site>.rs` with a type implementing [`SiteHandler`]:
//!    - `info()`: a stable `id` (stored with each link, never rename it), display name, home page,
//!      the domains it owns and a badge colour;
//!    - optionally `normalize()` to canonicalise pasted URLs (force https, drop session params...);
//!    - optionally `resolve()` when the site needs more than plain HTTP redirects to reach the
//!      final link (e.g. an intermediate page with a known structure);
//!    - optionally `find()` to search the site for a game and return the links it offers.
//! 2. Add it to [`builtin`] below. Order matters only if domains overlap.
//! 3. Add a test with a few real URL shapes of that site.
//!
//! ```ignore
//! use reqwest::Url;
//! use reqwest::blocking::Client;
//! use crate::links::{SiteHandler, resolve::follow_redirects};
//! use crate::model::{LinkCheck, SiteInfo};
//!
//! pub struct ExampleSite { info: SiteInfo }
//!
//! impl ExampleSite {
//!     pub fn new() -> Self {
//!         Self { info: SiteInfo {
//!             id: "example".into(), name: "Example".into(),
//!             homepage: Some("https://example.com".into()),
//!             domains: vec!["example.com".into()], color: "#4f8cff".into(),
//!             browser_required: false,
//!         } }
//!     }
//! }
//!
//! impl SiteHandler for ExampleSite {
//!     fn info(&self) -> &SiteInfo { &self.info }
//!     fn normalize(&self, mut url: Url) -> Url { let _ = url.set_scheme("https"); url }
//!     fn resolve(&self, url: &Url, http: &Client) -> LinkCheck { follow_redirects(url, http) }
//! }
//! ```

pub mod ankergames;
pub mod astralgames;
pub mod fitgirl;
pub mod gamebounty;
pub mod generic;
pub mod gog_rev;
pub mod steamrip;

use super::SiteHandler;

/// Site-specific handlers shipped with the app. The generic handler is always the fallback and
/// is not listed here.
pub fn builtin() -> Vec<Box<dyn SiteHandler>> {
    vec![
        Box::new(ankergames::AnkerGames::new()),
        Box::new(fitgirl::FitGirl::new()),
        Box::new(astralgames::AstralGames::new()),
        Box::new(gamebounty::GameBounty::new()),
        Box::new(steamrip::SteamRIP::new()),
        Box::new(gog_rev::GoGRevived::new()),
    ]
}
