import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Accounts,
  AppStatus,
  CmdError,
  Download,
  DownloadList,
  DownloadProgress,
  DownloadRemoved,
  FileOption,
  FoundLink,
  InstallChanged,
  Installed,
  InstallProgress,
  GameDetail,
  GameLink,
  GameMedia,
  GameRequirements,
  GameReviews,
  GamePage,
  GameQuery,
  LibraryItem,
  LinkCheck,
  LinkInput,
  MatchState,
  Settings,
  SettingsPatch,
  SiteInfo,
  Store,
  StoreMatch,
  StoreSearchHit,
  SyncFinished,
  SyncProgress,
  TagInfo,
  UpdateProgress,
  UpdateStatus,
  SearchSite,
} from "./types";

/** Typed wrappers around the Rust commands (src-tauri/src/commands.rs; `gamelib-cli serve` in the browser preview). */
export const api = {
  getStatus: () => invoke<AppStatus>("get_status"),
  /** The version and what the last check found, without asking GitHub. */
  getUpdateStatus: () => invoke<UpdateStatus>("get_update_status"),
  checkUpdate: () => invoke<UpdateStatus>("check_update"),
  /** Desktop only: downloads and installs the update found by the last check, then restarts. */
  installUpdate: () => invoke<void>("install_update"),
  /** A release's page on GitHub (what is new). */
  openReleasePage: (version: string | null) => invoke<void>("open_release_page", { version }),
  startSync: (fresh = false) => invoke<void>("start_sync", { fresh }),
  fetchNewReleases: (days: number | null = null) => invoke<void>("fetch_new_releases", { days }),
  cancelSync: () => invoke<void>("cancel_sync"),
  queryGames: (params: GameQuery) => invoke<GamePage>("query_games", { params }),
  getGame: (appid: number) => invoke<GameDetail | null>("get_game", { appid }),
  listTags: () => invoke<TagInfo[]>("list_tags"),
  getGameMedia: (appid: number) => invoke<GameMedia>("get_game_media", { appid }),
  getGameReviews: (appid: number) => invoke<GameReviews>("get_game_reviews", { appid }),
  getGameRequirements: (appid: number) => invoke<GameRequirements>("get_game_requirements", { appid }),
  startStoreSync: () => invoke<void>("start_store_sync"),
  getStoreMatches: (appid: number) => invoke<StoreMatch[]>("get_store_matches", { appid }),
  refreshStoreMatches: (appid: number) => invoke<StoreMatch[]>("refresh_store_matches", { appid }),
  setMatchState: (store: Store, productId: string, appid: number, state: MatchState) =>
    invoke<void>("set_match_state", { store, productId, appid, state }),
  openStorePage: (store: Store, productId: string) => invoke<void>("open_store_page", { store, productId }),
  searchStore: (store: Store, appid: number) => invoke<StoreSearchHit[]>("search_store", { store, appid }),
  linkStoreProduct: (store: Store, productId: string, appid: number) => invoke<void>("link_store_product", { store, productId, appid }),
  getAccounts: () => invoke<Accounts>("get_accounts"),
  gogLoginUrl: () => invoke<string>("gog_login_url"),
  openGogLoginPage: () => invoke<void>("open_gog_login_page"),
  /** Desktop only: GOG's login page in a separate window. */
  gogLogin: () => invoke<Accounts>("gog_login"),
  gogLoginWithCode: (redirect: string) => invoke<Accounts>("gog_login_with_code", { redirect }),
  itchSetKey: (key: string) => invoke<Accounts>("itch_set_key", { key }),
  signOut: (store: Store) => invoke<Accounts>("sign_out", { store }),
  startLibrarySync: () => invoke<void>("start_library_sync"),
  getLibrary: (store: Store | null = null) => invoke<LibraryItem[]>("get_library", { store }),
  openAccountPage: (store: Store) => invoke<void>("open_account_page", { store }),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => invoke<Settings>("update_settings", { patch }),
  /** Desktop only: a folder picker; null when cancelled. */
  pickLibraryDir: () => invoke<Settings | null>("pick_library_dir"),
  getStoreFiles: (store: Store, productId: string) => invoke<FileOption[]>("get_store_files", { store, productId }),
  /** Desktop only: the preview server never downloads. */
  enqueueDownload: (store: Store, productId: string, optionId: string) =>
    invoke<Download>("enqueue_download", { store, productId, optionId }),
  /** Desktop only: queues a magnet link or a `.torrent` address for this game. */
  enqueueTorrent: (appid: number, title: string, source: string) => invoke<Download>("enqueue_torrent", { appid, title, source }),
  getDownloads: () => invoke<DownloadList>("get_downloads"),
  pauseDownload: (id: number) => invoke<void>("pause_download", { id }),
  resumeDownload: (id: number) => invoke<void>("resume_download", { id }),
  /** Cancels a download and deletes its files. */
  removeDownload: (id: number) => invoke<void>("remove_download", { id }),
  clearFinishedDownloads: () => invoke<void>("clear_finished_downloads"),
  openDownloadFolder: (id: number) => invoke<void>("open_download_folder", { id }),
  /** Lets someone else's installer (an itch.io upload) run. */
  approveInstall: (id: number) => invoke<void>("approve_install", { id }),
  retryInstall: (id: number) => invoke<void>("retry_install", { id }),
  getInstalls: () => invoke<Installed[]>("get_installs"),
  launchGame: (store: Store, productId: string) => invoke<void>("launch_game", { store, productId }),
  uninstallGame: (store: Store, productId: string) => invoke<void>("uninstall_game", { store, productId }),
  openInstallFolder: (store: Store, productId: string) => invoke<void>("open_install_folder", { store, productId }),
  setLaunchTarget: (store: Store, productId: string, exe: string) => invoke<Installed>("set_launch_target", { store, productId, exe }),
  /** Desktop only: a file picker; null when cancelled. */
  pickLaunchTarget: (store: Store, productId: string) => invoke<Installed | null>("pick_launch_target", { store, productId }),
  listSites: () => invoke<SiteInfo[]>("list_sites"),
  listLinks: (appid: number) => invoke<GameLink[]>("list_links", { appid }),
  findLinks: (appid: number) => invoke<FoundLink[]>("find_links", { appid }),
  saveLink: (input: LinkInput) => invoke<GameLink>("save_link", { input }),
  deleteLink: (id: number) => invoke<boolean>("delete_link", { id }),
  checkLink: (id: number) => invoke<LinkCheck>("check_link", { id }),
  openLink: (id: number) => invoke<void>("open_link", { id }),
  /** Desktop only: the in-app browser; anything downloaded there is queued for this game. */
  openBrowser: (appid: number, title: string, url: string) => invoke<void>("open_browser", { appid, title, url }),
  openInSteam: (appid: number, target: "web" | "client" | "install") => invoke<void>("open_in_steam", { appid, target }),
  /** A web search in the default browser (gameplay videos, hardware comparisons). */
  openSearch: (site: SearchSite, query: string) => invoke<void>("open_search", { site, query }),
};

export const EVENT_PROGRESS = "sync:progress";
export const EVENT_FINISHED = "sync:finished";

export function onSyncProgress(cb: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>(EVENT_PROGRESS, (e) => cb(e.payload));
}

export function onSyncFinished(cb: (f: SyncFinished) => void): Promise<UnlistenFn> {
  return listen<SyncFinished>(EVENT_FINISHED, (e) => cb(e.payload));
}

export const EVENT_DOWNLOAD_PROGRESS = "download:progress";
export const EVENT_DOWNLOAD_STATE = "download:state";

export function onDownloadProgress(cb: (p: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>(EVENT_DOWNLOAD_PROGRESS, (e) => cb(e.payload));
}

export function onDownloadState(cb: (d: Download | DownloadRemoved) => void): Promise<UnlistenFn> {
  return listen<Download | DownloadRemoved>(EVENT_DOWNLOAD_STATE, (e) => cb(e.payload));
}

export const EVENT_UPDATE_PROGRESS = "update:progress";

export function onUpdateProgress(cb: (p: UpdateProgress) => void): Promise<UnlistenFn> {
  return listen<UpdateProgress>(EVENT_UPDATE_PROGRESS, (e) => cb(e.payload));
}

export const EVENT_INSTALL_PROGRESS = "install:progress";
export const EVENT_INSTALL_CHANGED = "install:changed";

export function onInstallProgress(cb: (p: InstallProgress) => void): Promise<UnlistenFn> {
  return listen<InstallProgress>(EVENT_INSTALL_PROGRESS, (e) => cb(e.payload));
}

export function onInstallChanged(cb: (c: InstallChanged) => void): Promise<UnlistenFn> {
  return listen<InstallChanged>(EVENT_INSTALL_CHANGED, (e) => cb(e.payload));
}

/** Normalizes anything thrown by `invoke` into a `CmdError`. */
export function toCmdError(e: unknown): CmdError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    return e as CmdError;
  }
  return { kind: "other", message: e instanceof Error ? e.message : String(e) };
}
