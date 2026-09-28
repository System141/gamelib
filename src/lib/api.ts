import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppStatus,
  CmdError,
  GameDetail,
  GameLink,
  GameMedia,
  GamePage,
  GameQuery,
  LinkCheck,
  LinkInput,
  SiteInfo,
  SyncFinished,
  SyncProgress,
  TagInfo,
} from "./types";

/** Typed wrappers around the Rust commands in src-tauri/src/commands.rs. */
export const api = {
  getStatus: () => invoke<AppStatus>("get_status"),
  startSync: (fresh = false) => invoke<void>("start_sync", { fresh }),
  fetchNewReleases: (days: number | null = null) => invoke<void>("fetch_new_releases", { days }),
  cancelSync: () => invoke<void>("cancel_sync"),
  queryGames: (params: GameQuery) => invoke<GamePage>("query_games", { params }),
  getGame: (appid: number) => invoke<GameDetail | null>("get_game", { appid }),
  listTags: () => invoke<TagInfo[]>("list_tags"),
  getGameMedia: (appid: number) => invoke<GameMedia>("get_game_media", { appid }),
  listSites: () => invoke<SiteInfo[]>("list_sites"),
  listLinks: (appid: number) => invoke<GameLink[]>("list_links", { appid }),
  saveLink: (input: LinkInput) => invoke<GameLink>("save_link", { input }),
  deleteLink: (id: number) => invoke<boolean>("delete_link", { id }),
  checkLink: (id: number) => invoke<LinkCheck>("check_link", { id }),
  openLink: (id: number) => invoke<void>("open_link", { id }),
  openInSteam: (appid: number, target: "web" | "client") => invoke<void>("open_in_steam", { appid, target }),
};

export const EVENT_PROGRESS = "sync:progress";
export const EVENT_FINISHED = "sync:finished";

export function onSyncProgress(cb: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>(EVENT_PROGRESS, (e) => cb(e.payload));
}

export function onSyncFinished(cb: (f: SyncFinished) => void): Promise<UnlistenFn> {
  return listen<SyncFinished>(EVENT_FINISHED, (e) => cb(e.payload));
}

/** Normalizes anything thrown by `invoke` into a `CmdError`. */
export function toCmdError(e: unknown): CmdError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    return e as CmdError;
  }
  return { kind: "other", message: e instanceof Error ? e.message : String(e) };
}
