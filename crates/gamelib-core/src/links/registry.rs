use reqwest::Url;
use reqwest::blocking::Client;

use super::SiteHandler;
use super::find::{self, FindQuery};
use super::sites::{self, generic::GenericSite};
use crate::model::{FoundLink, SiteInfo};

/// Picks the handler for a link: the first site-specific handler that matches, else the generic one.
pub struct SiteRegistry {
    handlers: Vec<Box<dyn SiteHandler>>,
    fallback: GenericSite,
}

impl SiteRegistry {
    /// Registry with every handler listed in [`sites::builtin`].
    pub fn with_builtin_sites() -> Self {
        Self::new(sites::builtin())
    }

    pub fn new(handlers: Vec<Box<dyn SiteHandler>>) -> Self {
        Self {
            handlers,
            fallback: GenericSite::new(),
        }
    }

    pub fn detect(&self, url: &Url) -> &dyn SiteHandler {
        self.handlers
            .iter()
            .map(|h| h.as_ref())
            .find(|h| h.matches(url))
            .unwrap_or(&self.fallback)
    }

    pub fn get(&self, id: &str) -> Option<&dyn SiteHandler> {
        if id == self.fallback.info().id {
            return Some(&self.fallback);
        }
        self.handlers
            .iter()
            .map(|h| h.as_ref())
            .find(|h| h.info().id == id)
    }

    /// Known sites, the generic fallback last.
    pub fn list(&self) -> Vec<SiteInfo> {
        self.handlers
            .iter()
            .map(|h| h.info().clone())
            .chain(std::iter::once(self.fallback.info().clone()))
            .collect()
    }

    /// Asks every handler to search for the game. Sites that cannot search return nothing.
    pub fn find_all(&self, query: &FindQuery, http: &Client) -> Vec<FoundLink> {
        find::find_all(&self.handlers, query, http)
    }
}

impl Default for SiteRegistry {
    fn default() -> Self {
        Self::with_builtin_sites()
    }
}
