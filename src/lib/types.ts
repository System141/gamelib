// Mirrors crates/gamelib-core/src/model.rs and error.rs (camelCase JSON).

export type SortKey = "relevance" | "popular" | "rating" | "newest" | "oldest" | "name";
export type Platform = "win" | "mac" | "linux";
export type DeckFilter = "playable" | "verified";

export interface GameQuery {
  search: string | null;
  tags: number[];
  platforms: Platform[];
  deck: DeckFilter | null;
  freeOnly: boolean;
  minReviewScore: number | null;
  showAdult: boolean;
  releasedWithinDays: number | null;
  hasLinks: boolean;
  /** Only games matched to a product in any of these stores. */
  stores: Store[];
  /** Only games matched to a store product the user owns. */
  owned: boolean;
  sort: SortKey;
  offset: number;
  limit: number;
}

/** 0 unknown, 1 unsupported, 2 playable, 3 verified. */
export type DeckCompat = 0 | 1 | 2 | 3;

export interface GameCard {
  appid: number;
  name: string;
  capsule: string | null;
  capsule2x: string | null;
  header: string | null;
  releaseDate: number | null;
  isFree: boolean;
  isEarlyAccess: boolean;
  price: string | null;
  originalPrice: string | null;
  discountPct: number;
  /** 0 none, 1 overwhelmingly negative … 9 overwhelmingly positive. */
  reviewScore: number;
  reviewPct: number;
  reviewCount: number;
  win: boolean;
  mac: boolean;
  linux: boolean;
  deck: DeckCompat;
  topTags: number[];
  linkCount: number;
  /** Stores that sell this game (confident matches only). */
  stores: Store[];
}

export interface GameDetail extends GameCard {
  shortDescription: string | null;
  developers: string[];
  publishers: string[];
  franchises: string[];
  tags: number[];
  descriptors: number[];
  originalReleaseDate: number | null;
  hero: string | null;
  storeUrl: string;
  adult: boolean;
  delisted: boolean;
  firstSeenAt: number;
  syncedAt: number;
}

export interface GamePage {
  total: number;
  items: GameCard[];
}

export interface TagInfo {
  tagid: number;
  name: string;
  gameCount: number;
}

export interface Screenshot {
  thumb: string;
  full: string;
  mature: boolean;
}

export interface GameMedia {
  descriptionTr: string | null;
  screenshots: Screenshot[];
  trailers: Trailer[];
  /** Steam's review summaries, when it sent them. */
  reviews: ReviewSummaries | null;
}

export interface Trailer {
  name: string;
  /** 600×337 still. */
  poster: string | null;
  /** HLS playlist. */
  stream: string;
  mature: boolean;
}

export interface ReviewScore {
  count: number;
  /** Share of positive reviews, 0–100. */
  percent: number;
  /** Steam's 0–9 summary score. */
  score: number;
}

export interface ReviewSummaries {
  /** All languages, without reviews Steam marked as off-topic. */
  all: ReviewScore | null;
  turkish: ReviewScore | null;
}

export interface Review {
  id: string;
  /** Steam's language name: "turkish", "english". */
  language: string;
  positive: boolean;
  text: string;
  helpful: number;
  hoursAtReview: number;
  hoursTotal: number;
  created: number;
  earlyAccess: boolean;
  receivedForFree: boolean;
}

/** The latest reviews (up to 100) and the time they span. */
export interface RecentReviews {
  count: number;
  positive: number;
  from: number;
  to: number;
}

export interface GameReviews {
  /** The most helpful reviews of the past year: Turkish ones first, then English. */
  top: Review[];
  recent: RecentReviews | null;
}

export type SearchSite = "youtube" | "google";

// --- prices (IsThereAnyDeal) -----------------------------------------------------------------

export interface Money {
  amount: number;
  /** ISO 4217 code: "USD", "TRY". */
  currency: string;
}

export interface Deal {
  shop: string;
  price: Money;
  regular: Money;
  /** Discount in percent. */
  cut: number;
  /** The shop's own lowest price for the game. */
  storeLow: Money | null;
  drm: string[];
  /** When the discount ends (Unix seconds). */
  expiry: number | null;
  url: string;
}

export interface LowestPrice {
  shop: string;
  price: Money;
  regular: Money;
  cut: number;
  at: number;
}

export interface Subscription {
  name: string;
  /** When the game leaves it (Unix seconds), when announced. */
  leaving: number | null;
}

export interface Bundle {
  title: string;
  /** Who sells it. */
  store: string;
  /** The cheapest tier that includes the game. */
  price: Money | null;
  expiry: number | null;
  /** IsThereAnyDeal's page about the bundle. */
  url: string | null;
}

export interface PricePoint {
  at: number;
  price: number;
  regular: number;
  cut: number;
}

export interface GamePrices {
  /** Whether IsThereAnyDeal knows the game. */
  found: boolean;
  url: string | null;
  /** Current prices, cheapest first. */
  deals: Deal[];
  lowest: LowestPrice | null;
  lowestYear: Money | null;
  lowestMonths: Money | null;
  subscriptions: Subscription[];
  bundles: Bundle[];
  /** Steam's price changes over the past two years, oldest first. */
  history: PricePoint[];
}

// --- system requirements ----------------------------------------------------------------------

export type RequirementKind = "os" | "processor" | "memory" | "graphics" | "directx" | "storage" | "sound" | "network" | "notes" | "other";

export interface RequirementLine {
  kind: RequirementKind;
  /** The store's own label, for lines of a kind GameLib does not know ("VR Support"). */
  label: string | null;
  text: string;
}

export type CheckKind = "memory" | "video_memory" | "storage" | "ssd" | "directx" | "windows" | "bits64";
export type Verdict = "ok" | "short" | "unknown";

/** `need`/`have`: bytes for sizes, the version for DirectX and Windows, 1/0 for yes-or-no checks. */
export interface RequirementCheck {
  kind: CheckKind;
  need: number;
  have: number | null;
  verdict: Verdict;
}

export interface RequirementList {
  lines: RequirementLine[];
  checks: RequirementCheck[];
}

export interface ThisPc {
  os: string;
  windows: number | null;
  bits64: boolean;
  cpu: string | null;
  cores: number | null;
  memory: number | null;
  gpu: string | null;
  videoMemory: number | null;
  directx: number | null;
  diskFree: number | null;
  diskSsd: boolean | null;
  diskPath: string;
}

export interface GameRequirements {
  /** The system these requirements are for. */
  platform: Platform;
  minimum: RequirementList;
  recommended: RequirementList;
  pc: ThisPc;
}

export type WorkerKind = "full" | "new_releases" | "stores" | "library";
export type SyncPhase =
  "starting" | "tags" | "featured" | "catalog" | "new_releases" | "gog_catalog" | "matching" | "gog_ids" | "library" | "finalizing";

export interface SyncProgress {
  kind: WorkerKind;
  phase: SyncPhase;
  fetched: number;
  /** 0 when unknown (new releases). */
  total: number;
  page: number;
  pages: number;
  startedAt: number;
  resumed: boolean;
}

export interface SyncReport {
  total: number;
  seen: number;
  inserted: number;
  delisted: number;
  skipped: number;
  requests: number;
  retries: number;
  durationMs: number;
  pruneSkipped: boolean;
  resumed: boolean;
  warnings: string[];
}

export interface NewReleasesReport {
  fetched: number;
  inserted: number;
  updated: number;
  pages: number;
  partial: boolean;
  since: number;
  watermark: number | null;
  requests: number;
  retries: number;
  durationMs: number;
}

export type ErrorKind =
  "network" | "timeout" | "rate_limited" | "http" | "parse" | "database" | "cancelled" | "invalid" | "not_found" | "busy" | "other";

export interface CmdError {
  kind: ErrorKind;
  /** For `invalid`: a stable code such as `url_scheme`. */
  message: string;
}

export interface StoresReport {
  catalog: number;
  inserted: number;
  matchedGames: number;
  checked: number;
  remaining: number;
  requests: number;
  retries: number;
  durationMs: number;
  warnings: string[];
  library: LibraryReport | null;
}

export interface LibraryReport {
  /** Owned products; null when not signed in there. */
  gogOwned: number | null;
  itchOwned: number | null;
  matched: number;
  gogSignedOut: boolean;
  warnings: string[];
}

export interface SyncFinished {
  kind: WorkerKind;
  outcome: "completed" | "cancelled" | "failed";
  report: SyncReport | null;
  newReleases: NewReleasesReport | null;
  stores: StoresReport | null;
  library: LibraryReport | null;
  error: CmdError | null;
}

export interface StoreCounts {
  /** Steam games with a GOG / itch.io match. */
  gog: number;
  itch: number;
  owned: number;
  /** GOG products known; 0 until stores were matched once. */
  gogProducts: number;
  lastStoreSyncAt: number | null;
}

export interface AppStatus {
  gameCount: number;
  tagCount: number;
  linkedGameCount: number;
  lastSyncAt: number | null;
  lastNewReleasesAt: number | null;
  resumable: boolean;
  storeCounts: StoreCounts;
  worker: WorkerKind | null;
  progress: SyncProgress | null;
  dbPath: string;
}

export type LinkKind = "download" | "page";

export type CheckStatus =
  | "ok"
  | "broken"
  | "restricted"
  | "not_found"
  | "server_error"
  | "loop"
  | "too_many_redirects"
  | "timeout"
  | "network"
  | "tls"
  | "unsupported_scheme";

export interface SiteInfo {
  id: string;
  name: string;
  homepage: string | null;
  domains: string[];
  color: string;
  /** The site's pages hand the download out only through a browser click-through. */
  browserRequired: boolean;
}

export interface LinkInput {
  id?: number | null;
  appid: number;
  url: string;
  label?: string | null;
  kind: LinkKind;
  platform?: Platform | null;
  version?: string | null;
  notes?: string | null;
}

export interface Hop {
  url: string;
  status: number;
}

export interface LinkCheck {
  status: CheckStatus;
  httpStatus: number | null;
  finalUrl: string | null;
  finalHost: string | null;
  hops: Hop[];
  fileName: string | null;
  sizeBytes: number | null;
  contentType: string | null;
  isFile: boolean;
  checkedAt: number;
  message: string | null;
}

export interface LinkCheckSummary {
  status: CheckStatus;
  httpStatus: number | null;
  resolvedUrl: string | null;
  finalHost: string | null;
  redirects: number;
  /** Empty for checks stored before chains were kept. */
  hops: Hop[];
  fileName: string | null;
  sizeBytes: number | null;
  contentType: string | null;
  isFile: boolean;
  checkedAt: number;
}

export interface GameLink {
  id: number;
  appid: number;
  siteId: string;
  url: string;
  host: string;
  label: string | null;
  kind: LinkKind;
  platform: Platform | null;
  version: string | null;
  notes: string | null;
  insecure: boolean;
  lastCheck: LinkCheckSummary | null;
  createdAt: number;
  updatedAt: number;
}

/** A link a site search turned up for a Steam game, offered for the user to save. */
export interface FoundLink {
  siteId: string;
  url: string;
  label: string;
  kind: LinkKind;
  version: string | null;
  size: string | null;
  notes: string | null;
  score: number;
  /** The link has to be opened in a browser to finish (a verification step or a login). */
  needsBrowser: boolean;
  /** The URL is the download itself (a magnet link) rather than a page to click through. */
  direct: boolean;
}

// --- other stores ---------------------------------------------------------------------------

/** Every store GameLib knows: the two with accounts, and `web` for captured downloads. */
export type Store = "gog" | "itch" | "web";
export type MatchMethod = "gamesdb" | "title" | "manual";
export type MatchState = "auto" | "confirmed" | "rejected";

export interface StoreMatch {
  /** Matches are only looked up for stores with an account. */
  store: AccountStore;
  productId: string;
  title: string;
  url: string | null;
  cover: string | null;
  coverWide: string | null;
  price: string | null;
  isFree: boolean;
  owned: boolean;
  win: boolean;
  mac: boolean;
  linux: boolean;
  method: MatchMethod;
  score: number;
  state: MatchState;
  /** Counts as a match; otherwise it is a suggestion to confirm. */
  confident: boolean;
}

export interface StoreSearchHit {
  /** itch.io is the only store that can be searched by hand. */
  store: AccountStore;
  productId: string;
  title: string;
  url: string | null;
  coverWide: string | null;
  developer: string | null;
  price: string | null;
  isFree: boolean;
  win: boolean;
  mac: boolean;
  linux: boolean;
  score: number;
}

// --- accounts, library, settings --------------------------------------------------------------

export interface Account {
  username: string;
}

/** The stores that have an account, a product page and a download picker. */
export type AccountStore = "gog" | "itch";

export type Accounts = Record<AccountStore, Account | null> & {
  /** The IsThereAnyDeal API key (for prices), when one is saved; the key itself stays in Rust. */
  itad: ApiKeyStatus | null;
};

export interface ApiKeyStatus {
  savedAt: number;
}

export interface LibraryItem {
  store: Store;
  productId: string;
  title: string;
  url: string | null;
  cover: string | null;
  coverWide: string | null;
  win: boolean;
  mac: boolean;
  linux: boolean;
  /** The Steam game it is, if matched. */
  appid: number | null;
  steamHeader: string | null;
  steamCapsule: string | null;
}

export interface Settings {
  libraryDir: string;
  keepInstallers: boolean;
  autoUpdate: boolean;
}

export type SettingsPatch = Partial<Settings>;

// --- downloads --------------------------------------------------------------------------------

/** A downloadable variant of a store product (a GOG installer, an itch.io upload). */
export interface FileOption {
  id: string;
  label: string;
  platform: Platform | null;
  /** Installer language code (GOG), e.g. "tr", "en". */
  language: string | null;
  version: string | null;
  /** Total bytes (0 if unknown). */
  size: number;
  files: number;
  demo: boolean;
  /** The best choice for this computer. */
  recommended: boolean;
}

export type DownloadState = "queued" | "downloading" | "paused" | "completed" | "failed";

/** Where a download's bytes come from: an HTTP(S) address or a BitTorrent swarm. */
export type DownloadSourceKind = "http" | "torrent";

export interface Download {
  id: number;
  store: Store;
  sourceKind: DownloadSourceKind;
  productId: string;
  appid: number | null;
  title: string;
  optionId: string;
  optionLabel: string | null;
  platform: Platform | null;
  state: DownloadState;
  totalBytes: number;
  doneBytes: number;
  /** Folder the files are downloaded to. */
  dir: string;
  files: number;
  error: CmdError | null;
  createdAt: number;
  finishedAt: number | null;
  /** Installing the finished download; null for downloads from before installs existed. */
  installState: InstallState | null;
  /** What the file turned out to be ("inno_setup", "zip", "rar", …). */
  installKind: string | null;
  installError: CmdError | null;
}

/** Live progress of the running download (`download:progress`). */
export interface DownloadProgress {
  id: number;
  doneBytes: number;
  totalBytes: number;
  /** Bytes per second. */
  speed: number;
  /** Seconds left. */
  eta: number | null;
  stage: "downloading" | "verifying";
}

export interface DownloadList {
  /** Newest first. */
  items: Download[];
  live: DownloadProgress | null;
  /** The running install's progress. */
  installing: InstallProgress | null;
}

/** `download:state` payload when a download was removed. */
export interface DownloadRemoved {
  id: number;
  removed: true;
}

// --- installs ---------------------------------------------------------------------------------

export type InstallState = "waiting" | "installing" | "installed" | "failed" | "confirm" | "approved" | "manual";
export type InstallMethod = "gog" | "archive" | "portable" | "installer" | "galaxy";

export interface Installed {
  store: Store;
  productId: string;
  appid: number | null;
  title: string;
  /** Install folder, when known. */
  dir: string | null;
  /** What "Oyna" starts; null until one is chosen. */
  exe: string | null;
  args: string;
  workdir: string | null;
  method: InstallMethod;
  /** Other programs in the folder that could be the game. */
  candidates: string[];
  optionLabel: string | null;
  installedAt: number;
  /** Installed outside GameLib (GOG Galaxy). */
  external: boolean;
  /** The matched Steam game's header image. */
  steamHeader: string | null;
}

/** `install:progress` payload. */
export interface InstallProgress {
  downloadId: number;
  stage: "checking" | "unpacking" | "installing" | "cleaning";
  done: number;
  total: number;
}

/** `install:changed` payload. */
export interface InstallChanged {
  store: Store;
  productId: string;
}

// --- app updates ------------------------------------------------------------------------------

export interface UpdateInfo {
  version: string;
}

export interface UpdateStatus {
  /** Whether this build can update itself (it knows the releases' public key). */
  configured: boolean;
  currentVersion: string;
  /** When GitHub was last asked (unix seconds), since the app started. */
  checkedAt: number | null;
  update: UpdateInfo | null;
}

/** `update:progress` payload. */
export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}
