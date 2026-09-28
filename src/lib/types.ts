// Mirrors crates/gamelib-core/src/model.rs and src-tauri/src/commands.rs (camelCase JSON).

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

export type WorkerKind = "full" | "new_releases";
export type SyncPhase = "starting" | "tags" | "featured" | "catalog" | "new_releases" | "finalizing";

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
  | "network"
  | "timeout"
  | "rate_limited"
  | "http"
  | "parse"
  | "database"
  | "cancelled"
  | "invalid"
  | "not_found"
  | "busy"
  | "other";

export interface CmdError {
  kind: ErrorKind;
  /** For `invalid`: a stable code such as `url_scheme`. */
  message: string;
}

export interface SyncFinished {
  kind: WorkerKind;
  outcome: "completed" | "cancelled" | "failed";
  report: SyncReport | null;
  newReleases: NewReleasesReport | null;
  error: CmdError | null;
}

export interface AppStatus {
  gameCount: number;
  tagCount: number;
  linkedGameCount: number;
  lastSyncAt: number | null;
  lastNewReleasesAt: number | null;
  resumable: boolean;
  worker: WorkerKind | null;
  progress: SyncProgress | null;
  dbPath: string;
}

export type LinkKind = "download" | "page";

export type CheckStatus =
  | "ok"
  | "broken"
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
