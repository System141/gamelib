//! Data shapes shared with the frontend (serialized as camelCase JSON).
//!
//! Keep in sync with `src/lib/types.ts`.

use serde::{Deserialize, Serialize};

use crate::ErrorInfo;

// ---------------------------------------------------------------------------
// Catalog queries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// Exact name match first, then prefix matches, then popularity. Only meaningful with a search.
    Relevance,
    #[default]
    Popular,
    Rating,
    Newest,
    Oldest,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Win,
    Mac,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeckFilter {
    /// Playable or verified.
    Playable,
    Verified,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameQuery {
    pub search: Option<String>,
    /// Every listed tag must match.
    pub tags: Vec<u32>,
    /// Every listed platform must be supported.
    pub platforms: Vec<Platform>,
    pub deck: Option<DeckFilter>,
    pub free_only: bool,
    /// Minimum Steam review score (1–9).
    pub min_review_score: Option<u8>,
    pub show_adult: bool,
    pub released_within_days: Option<u32>,
    pub has_links: bool,
    /// Only games matched to a product in any of these stores.
    pub stores: Vec<Store>,
    /// Only games matched to a store product the user owns.
    pub owned: bool,
    pub sort: SortKey,
    pub offset: u32,
    /// Clamped to 1..=200; 0 means the default page size.
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameCard {
    pub appid: u32,
    pub name: String,
    /// Portrait library capsule (300×450).
    pub capsule: Option<String>,
    /// Portrait library capsule at 2x (600×900).
    pub capsule_2x: Option<String>,
    /// Landscape header (460×215), used as fallback art.
    pub header: Option<String>,
    pub release_date: Option<i64>,
    pub is_free: bool,
    pub is_early_access: bool,
    pub price: Option<String>,
    pub original_price: Option<String>,
    pub discount_pct: u8,
    /// Steam review score: 0 = none, 1 (overwhelmingly negative) … 9 (overwhelmingly positive).
    pub review_score: u8,
    pub review_pct: u8,
    pub review_count: u32,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    /// Steam Deck compatibility: 0 unknown, 1 unsupported, 2 playable, 3 verified.
    pub deck: u8,
    pub top_tags: Vec<u32>,
    pub link_count: u32,
    /// Stores that sell this game (confident matches only).
    pub stores: Vec<Store>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    #[serde(flatten)]
    pub card: GameCard,
    pub short_description: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub franchises: Vec<String>,
    pub tags: Vec<u32>,
    /// Steam content descriptor ids (1 some nudity, 2 violence, 3 adult sexual, 4 frequent nudity, 5 mature).
    pub descriptors: Vec<u32>,
    pub original_release_date: Option<i64>,
    /// Wide library hero image (1920×620), if the game has one.
    pub hero: Option<String>,
    pub store_url: String,
    pub adult: bool,
    pub delisted: bool,
    pub first_seen_at: i64,
    pub synced_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePage {
    pub total: u32,
    pub items: Vec<GameCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub tagid: u32,
    pub name: String,
    pub game_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMedia {
    /// Turkish short description, when the developer provided one.
    pub description_tr: Option<String>,
    pub screenshots: Vec<Screenshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    /// 600×338 thumbnail.
    pub thumb: String,
    /// 1920×1080 image.
    pub full: String,
    /// Listed by Steam as containing mature content.
    pub mature: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    pub game_count: u32,
    pub tag_count: u32,
    pub linked_game_count: u32,
    pub last_sync_at: Option<i64>,
    pub last_new_releases_at: Option<i64>,
    /// An interrupted full sync can be resumed.
    pub resumable: bool,
    pub store_counts: StoreCounts,
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Full,
    NewReleases,
    /// Matching other stores (GOG, itch.io) to Steam games.
    Stores,
    /// Reading the signed-in accounts' libraries.
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    Starting,
    Tags,
    Featured,
    Catalog,
    NewReleases,
    /// Reading GOG's catalog.
    GogCatalog,
    /// Matching store products to Steam games by title.
    Matching,
    /// Checking matches against GOG's GamesDB id cross-reference.
    GogIds,
    /// Reading the signed-in accounts' libraries.
    Library,
    Finalizing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub kind: WorkerKind,
    pub phase: SyncPhase,
    pub fetched: u32,
    /// 0 when unknown (new releases).
    pub total: u32,
    pub page: u32,
    pub pages: u32,
    pub started_at: i64,
    pub resumed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// Games Steam reported for the query.
    pub total: u32,
    /// Games stored or refreshed during this run.
    pub seen: u32,
    /// Games stored for the first time during this run.
    pub inserted: u32,
    /// Games no longer on the store, now hidden.
    pub delisted: u32,
    /// Store items that could not be parsed.
    pub skipped: u32,
    pub requests: u32,
    pub retries: u32,
    pub duration_ms: u64,
    /// Delisting was skipped because the run did not see enough of the catalog.
    pub prune_skipped: bool,
    pub resumed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewReleasesReport {
    /// Distinct games returned by Steam and stored.
    pub fetched: u32,
    /// Games that were not in the catalog before.
    pub inserted: u32,
    pub updated: u32,
    pub pages: u32,
    /// Stopped at the page limit before reaching the previous check; a full sync is recommended.
    pub partial: bool,
    /// Releases on or after this Unix time were requested.
    pub since: i64,
    pub watermark: Option<i64>,
    pub requests: u32,
    pub retries: u32,
    pub duration_ms: u64,
}

/// What `get_status` returns: catalog counts plus the running job, if any.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    #[serde(flatten)]
    pub catalog: CatalogStatus,
    pub worker: Option<WorkerKind>,
    /// Latest progress of the running job, so a reloaded UI catches up without waiting.
    pub progress: Option<SyncProgress>,
    pub db_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Cancelled,
    Failed,
}

/// Payload of the `sync:finished` event, sent exactly once per job.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFinished {
    pub kind: WorkerKind,
    pub outcome: Outcome,
    pub report: Option<SyncReport>,
    pub new_releases: Option<NewReleasesReport>,
    pub stores: Option<StoresReport>,
    pub library: Option<LibraryReport>,
    pub error: Option<ErrorInfo>,
}

/// Where `open_in_steam` opens a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenTarget {
    Web,
    Client,
    /// The Steam client's install dialog (for games the user owns there).
    Install,
}

// ---------------------------------------------------------------------------
// External (non-Steam) links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    #[default]
    Download,
    Page,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Download => "download",
            LinkKind::Page => "page",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "page" => LinkKind::Page,
            _ => LinkKind::Download,
        }
    }
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Win => "win",
            Platform::Mac => "mac",
            Platform::Linux => "linux",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "win" => Some(Platform::Win),
            "mac" => Some(Platform::Mac),
            "linux" => Some(Platform::Linux),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub id: String,
    pub name: String,
    pub homepage: Option<String>,
    pub domains: Vec<String>,
    /// Badge colour (#rrggbb).
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInput {
    /// Set to update an existing link.
    #[serde(default)]
    pub id: Option<i64>,
    pub appid: u32,
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub kind: LinkKind,
    #[serde(default)]
    pub platform: Option<Platform>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The final response was 2xx.
    Ok,
    /// The final response was 4xx/5xx or a redirect had no target.
    Broken,
    Loop,
    TooManyRedirects,
    Timeout,
    Network,
    Tls,
    /// A redirect pointed to a non-HTTP scheme (e.g. a custom app protocol).
    UnsupportedScheme,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Broken => "broken",
            CheckStatus::Loop => "loop",
            CheckStatus::TooManyRedirects => "too_many_redirects",
            CheckStatus::Timeout => "timeout",
            CheckStatus::Network => "network",
            CheckStatus::Tls => "tls",
            CheckStatus::UnsupportedScheme => "unsupported_scheme",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "ok" => CheckStatus::Ok,
            "loop" => CheckStatus::Loop,
            "too_many_redirects" => CheckStatus::TooManyRedirects,
            "timeout" => CheckStatus::Timeout,
            "network" => CheckStatus::Network,
            "tls" => CheckStatus::Tls,
            "unsupported_scheme" => CheckStatus::UnsupportedScheme,
            _ => CheckStatus::Broken,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub url: String,
    pub status: u16,
}

/// Result of following a link's HTTP redirects without downloading it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkCheck {
    pub status: CheckStatus,
    pub http_status: Option<u16>,
    pub final_url: Option<String>,
    pub final_host: Option<String>,
    pub hops: Vec<Hop>,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub content_type: Option<String>,
    /// The final response looks like a downloadable file rather than a web page.
    pub is_file: bool,
    pub checked_at: i64,
    pub message: Option<String>,
}

impl LinkCheck {
    /// Number of redirects followed.
    pub fn redirects(&self) -> u32 {
        self.hops.len().saturating_sub(1) as u32
    }
}

/// The last [`LinkCheck`] as stored with the link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkCheckSummary {
    pub status: CheckStatus,
    pub http_status: Option<u16>,
    pub resolved_url: Option<String>,
    pub final_host: Option<String>,
    pub redirects: u32,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub content_type: Option<String>,
    pub is_file: bool,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLink {
    pub id: i64,
    pub appid: u32,
    pub site_id: String,
    pub url: String,
    pub host: String,
    pub label: Option<String>,
    pub kind: LinkKind,
    pub platform: Option<Platform>,
    pub version: Option<String>,
    pub notes: Option<String>,
    /// Plain `http://` link.
    pub insecure: bool,
    pub last_check: Option<LinkCheckSummary>,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Other stores (GOG, itch.io)
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    #[default]
    Gog,
    Itch,
}

impl Store {
    pub const ALL: [Store; 2] = [Store::Gog, Store::Itch];

    pub fn as_str(self) -> &'static str {
        match self {
            Store::Gog => "gog",
            Store::Itch => "itch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gog" => Some(Store::Gog),
            "itch" => Some(Store::Itch),
            _ => None,
        }
    }
}

/// How a store product was tied to a Steam game. A manual choice outranks GOG's GamesDB id
/// cross-reference, which outranks a title match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMethod {
    Gamesdb,
    Title,
    Manual,
}

impl MatchMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchMethod::Gamesdb => "gamesdb",
            MatchMethod::Title => "title",
            MatchMethod::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "gamesdb" => MatchMethod::Gamesdb,
            "manual" => MatchMethod::Manual,
            _ => MatchMethod::Title,
        }
    }
}

/// The user's verdict on a match. Refreshes never change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchState {
    Auto,
    Confirmed,
    Rejected,
}

impl MatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchState::Auto => "auto",
            MatchState::Confirmed => "confirmed",
            MatchState::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "confirmed" => MatchState::Confirmed,
            "rejected" => MatchState::Rejected,
            _ => MatchState::Auto,
        }
    }
}

/// A store product tied to a Steam game, as the game's detail shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMatch {
    pub store: Store,
    pub product_id: String,
    pub title: String,
    pub url: Option<String>,
    /// Portrait cover, when the store has one.
    pub cover: Option<String>,
    /// Landscape cover.
    pub cover_wide: Option<String>,
    pub price: Option<String>,
    pub is_free: bool,
    pub owned: bool,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    pub method: MatchMethod,
    pub score: f32,
    pub state: MatchState,
    /// Counts as a match (confirmed, or found with enough confidence); otherwise it is only a
    /// suggestion the user can confirm.
    pub confident: bool,
}

/// Result of a "match stores" job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoresReport {
    /// GOG products listed by the catalog in this run.
    pub catalog: u32,
    /// Products seen for the first time.
    pub inserted: u32,
    /// Distinct Steam games with a GOG match after the run.
    pub matched_games: u32,
    /// GamesDB lookups made in this run, and those still left for a later run.
    pub checked: u32,
    pub remaining: u32,
    pub requests: u32,
    pub retries: u32,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
    /// The signed-in accounts' libraries, read at the end of the job.
    pub library: Option<LibraryReport>,
}

/// Sidebar counts: Steam games with a GOG / itch.io match, and owned store products.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCounts {
    pub gog: u32,
    pub itch: u32,
    pub owned: u32,
    /// GOG products known, so the UI can tell whether stores were ever matched.
    pub gog_products: u32,
    pub last_store_sync_at: Option<i64>,
}

/// Signed-in store accounts, as the UI sees them (never any token).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accounts {
    pub gog: Option<Account>,
    pub itch: Option<Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub username: String,
}

/// Result of reading the signed-in accounts' libraries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryReport {
    /// Products owned on GOG / itch.io; `None` when not signed in there.
    pub gog_owned: Option<u32>,
    pub itch_owned: Option<u32>,
    /// Owned products tied to a Steam game.
    pub matched: u32,
    /// The GOG session had expired and the user was signed out.
    pub gog_signed_out: bool,
    pub warnings: Vec<String>,
}

/// A product the user owns, for the library view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub store: Store,
    pub product_id: String,
    pub title: String,
    pub url: Option<String>,
    pub cover: Option<String>,
    pub cover_wide: Option<String>,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    /// The Steam game it is (best confident match), for its artwork and details.
    pub appid: Option<u32>,
    pub steam_header: Option<String>,
    pub steam_capsule: Option<String>,
}

/// A store search result offered for tying to a Steam game by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSearchHit {
    pub store: Store,
    pub product_id: String,
    pub title: String,
    pub url: Option<String>,
    pub cover_wide: Option<String>,
    pub developer: Option<String>,
    pub price: Option<String>,
    pub is_free: bool,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    /// Title/company/year agreement with the Steam game (0 when the titles differ).
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Where games are installed (and downloads kept until then).
    pub library_dir: String,
    /// Keep installers after a successful install.
    pub keep_installers: bool,
    /// Look for app updates on start and every few hours.
    pub auto_update: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub library_dir: Option<String>,
    pub keep_installers: Option<bool>,
    pub auto_update: Option<bool>,
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

/// A downloadable variant of a store product (a GOG installer, an itch.io upload).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOption {
    pub id: String,
    pub label: String,
    pub platform: Option<Platform>,
    /// Installer language code (GOG), e.g. "tr", "en".
    pub language: Option<String>,
    pub version: Option<String>,
    /// Total bytes (0 if unknown).
    pub size: u64,
    pub files: u32,
    pub demo: bool,
    /// The best choice for this computer.
    pub recommended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl DownloadState {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadState::Queued => "queued",
            DownloadState::Downloading => "downloading",
            DownloadState::Paused => "paused",
            DownloadState::Completed => "completed",
            DownloadState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "downloading" => DownloadState::Downloading,
            "paused" => DownloadState::Paused,
            "completed" => DownloadState::Completed,
            "failed" => DownloadState::Failed,
            _ => DownloadState::Queued,
        }
    }
}

/// A download in the queue (also the payload of `download:state`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub store: Store,
    pub product_id: String,
    pub appid: Option<u32>,
    pub title: String,
    pub option_id: String,
    pub option_label: Option<String>,
    pub platform: Option<Platform>,
    pub state: DownloadState,
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// Folder the files are downloaded to.
    pub dir: String,
    pub files: u32,
    /// Why it failed: an error kind and message, as for commands.
    pub error: Option<ErrorInfo>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// Installing the finished download (`None` for downloads from before installs existed).
    pub install_state: Option<InstallState>,
    /// What the downloaded file turned out to be (`inno_setup`, `zip`, `rar`, …).
    pub install_kind: Option<String>,
    pub install_error: Option<ErrorInfo>,
}

/// Live progress of the running download (payload of `download:progress`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: i64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Bytes per second, averaged over the last seconds.
    pub speed: u64,
    /// Seconds left at the current speed.
    pub eta: Option<u64>,
    /// "downloading" or "verifying".
    pub stage: String,
}

/// The download list with the running download's live progress.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadList {
    /// Newest first.
    pub items: Vec<Download>,
    pub live: Option<DownloadProgress>,
    /// Progress of the running install.
    pub installing: Option<InstallProgress>,
}

// ---------------------------------------------------------------------------
// Installs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    /// Waiting for the install thread.
    Waiting,
    Installing,
    Installed,
    Failed,
    /// Someone else's installer: it only runs once the user agrees.
    Confirm,
    /// Agreed; waiting for the install thread.
    Approved,
    /// GameLib cannot install this file (a RAR, a macOS package, another system's installer);
    /// it stays in the downloads folder.
    Manual,
}

impl InstallState {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallState::Waiting => "waiting",
            InstallState::Installing => "installing",
            InstallState::Installed => "installed",
            InstallState::Failed => "failed",
            InstallState::Confirm => "confirm",
            InstallState::Approved => "approved",
            InstallState::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            InstallState::Waiting,
            InstallState::Installing,
            InstallState::Installed,
            InstallState::Failed,
            InstallState::Confirm,
            InstallState::Approved,
            InstallState::Manual,
        ]
        .into_iter()
        .find(|state| state.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    /// A GOG installer, run silently.
    Gog,
    /// An archive GameLib unpacked.
    Archive,
    /// A single program copied into the library.
    Portable,
    /// Someone else's installer.
    Installer,
    /// Installed outside GameLib (GOG Galaxy or a GOG installer run by hand).
    Galaxy,
}

impl InstallMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMethod::Gog => "gog",
            InstallMethod::Archive => "archive",
            InstallMethod::Portable => "portable",
            InstallMethod::Installer => "installer",
            InstallMethod::Galaxy => "galaxy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            InstallMethod::Gog,
            InstallMethod::Archive,
            InstallMethod::Portable,
            InstallMethod::Installer,
            InstallMethod::Galaxy,
        ]
        .into_iter()
        .find(|m| m.as_str() == s)
    }
}

/// An installed game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub store: Store,
    pub product_id: String,
    /// The Steam game it is, if matched.
    pub appid: Option<u32>,
    pub title: String,
    /// The install folder, when known (someone else's installer may not say).
    pub dir: Option<String>,
    /// What "Oyna" starts; `None` until one is chosen.
    pub exe: Option<String>,
    /// Arguments, as one command line.
    pub args: String,
    pub workdir: Option<String>,
    pub method: InstallMethod,
    /// Other programs in the folder that could be the game.
    pub candidates: Vec<String>,
    /// The variant installed (e.g. "Windows · Türkçe · 1.6.2").
    pub option_label: Option<String>,
    pub installed_at: i64,
    /// Found in GOG's registry entries rather than installed by GameLib.
    pub external: bool,
    /// The matched Steam game's header image.
    pub steam_header: Option<String>,
}

/// Progress of the running install (payload of `install:progress`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub download_id: i64,
    /// "checking", "unpacking", "installing" or "cleaning".
    pub stage: String,
    /// Bytes unpacked so far (0 while an installer runs).
    pub done: u64,
    pub total: u64,
}
