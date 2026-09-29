use crate::links::SiteHandler;
use crate::model::SiteInfo;

pub const GENERIC_SITE_ID: &str = "generic";

/// Fallback for sites without a dedicated handler: default normalisation and plain HTTP redirects.
pub struct GenericSite {
    info: SiteInfo,
}

impl GenericSite {
    pub fn new() -> Self {
        Self {
            info: SiteInfo {
                id: GENERIC_SITE_ID.into(),
                name: "Other site".into(),
                homepage: None,
                domains: Vec::new(),
                color: "#8b93a7".into(),
                browser_required: false,
            },
        }
    }
}

impl Default for GenericSite {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for GenericSite {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    /// Matches anything; the registry only uses it when no specific handler does.
    fn matches(&self, _url: &reqwest::Url) -> bool {
        true
    }
}
