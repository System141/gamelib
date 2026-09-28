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

export type CheckStatus = "ok" | "broken" | "loop" | "too_many_redirects" | "timeout" | "network" | "tls" | "unsupported_scheme";

export interface SiteInfo {
  id: string;
  name: string;
  homepage: string | null;
  domains: string[];
  color: string;
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

// --- other stores ---------------------------------------------------------------------------

export type Store = "gog" | "itch";
export type MatchMethod = "gamesdb" | "title" | "manual";
export type MatchState = "auto" | "confirmed" | "rejected";

export interface StoreMatch {
  store: Store;
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
  store: Store;
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

export interface Accounts {
  gog: Account | null;
  itch: Account | null;
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
