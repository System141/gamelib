// In-browser stand-in for the Rust backend, used only by the dev preview (`pnpm dev` outside
// Tauri). It answers every command from a fixture of real games exported by
// `gamelib-cli export-fixture` and simulates catalog downloads with progress events.

import { fold, normalizeName } from "../lib/fold";
import { nowSeconds } from "../lib/format";
import type {
  AccountStore,
  Accounts,
  AppStatus,
  CmdError,
  Download,
  DownloadList,
  DownloadProgress,
  FileOption,
  FoundLink,
  Installed,
  InstallProgress,
  GameCard,
  GameDetail,
  GameLink,
  GameMedia,
  GamePrices,
  GameRequirements,
  GameReviews,
  GamePage,
  GameQuery,
  LibraryItem,
  LinkCheck,
  LinkInput,
  MatchState,
  Settings,
  SiteInfo,
  Store,
  StoreMatch,
  StoreSearchHit,
  SyncFinished,
  SyncProgress,
  TagInfo,
  UpdateStatus,
  WorkerKind,
  RequirementLine,
} from "../lib/types";

export interface Fixture {
  generatedAt: number;
  games: GameDetail[];
  tags: TagInfo[];
  /** Older fixtures have no trailers or review summaries. */
  media: Record<string, Partial<GameMedia>>;
  /** Store matches of fixture games, by appid (exported after a store sync). */
  storeMatches?: Record<string, StoreMatch[]>;
}

/** Matches count when confirmed, or automatic with this score. Mirrors matching::CONFIDENT. */
const CONFIDENT = 0.85;

type Emit = (event: string, payload: unknown) => void;

const SITES: SiteInfo[] = [
  {
    id: "ankergames",
    name: "AnkerGames",
    homepage: "https://ankergames.to",
    domains: ["ankergames.to"],
    color: "#4f5b93",
    browserRequired: true,
  },
  {
    id: "fitgirl",
    name: "FitGirl Repacks",
    homepage: "https://fitgirl-repacks.site",
    domains: ["fitgirl-repacks.site"],
    color: "#e91e8c",
    browserRequired: false,
  },
  {
    id: "astralgames",
    name: "AstralGames",
    homepage: "https://astralgames.net",
    domains: ["astralgames.net"],
    color: "#8b5cf6",
    browserRequired: true,
  },
  {
    id: "gamebounty",
    name: "GameBounty",
    homepage: "https://gamebounty.world",
    domains: ["gamebounty.world"],
    color: "#f59e0b",
    browserRequired: false,
  },
  {
    id: "steamrip",
    name: "SteamRIP",
    homepage: "https://steamrip.com",
    domains: ["steamrip.com"],
    color: "#7c3aed",
    browserRequired: false,
  },
  {
    id: "gog-rev",
    name: "GoG Revived",
    homepage: "https://gog-rev.com",
    domains: ["gog-rev.com"],
    color: "#ef4444",
    browserRequired: true,
  },
  { id: "generic", name: "Other site", homepage: null, domains: [], color: "#8b93a7", browserRequired: false },
];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const invalid = (message: string): CmdError => ({ kind: "invalid", message });

export class MockBackend {
  private all: GameDetail[];
  private present = new Set<number>();
  private held: number[] = [];
  private links: GameLink[] = [];
  private nextLinkId = 1;
  private lastSyncAt: number | null;
  private lastNewReleasesAt: number | null = null;
  private worker: WorkerKind | null = null;
  private progress: SyncProgress | null = null;
  private cancelled = false;
  /** Every store match the fixture knows; `matches` holds the ones "found" so far. */
  private allMatches = new Map<number, StoreMatch[]>();
  private matches = new Map<number, StoreMatch[]>();
  private lastStoreSyncAt: number | null = null;
  private accounts: Accounts = { gog: null, itch: null, itad: null };
  /** Owned products as `store:productId`. */
  private owned = new Set<string>();
  private itchLibrary: LibraryItem[] = [];
  private settings: Settings = { libraryDir: "C:\\Users\\oyuncu\\Games", keepInstallers: false, autoUpdate: true };
  private downloads: Download[] = [];
  private live: DownloadProgress | null = null;
  private installs: Installed[] = [];
  private libraryPending = false;
  /** `?mock=update`: a newer version is out. */
  private fakeUpdate = new URLSearchParams(window.location.search).get("mock") === "update";
  private updateCheckedAt: number | null = null;
  private installing: InstallProgress | null = null;
  private nextDownloadId = 1;
  /** The simulated transfer's timer, while one runs. */
  private transfer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private fixture: Fixture,
    private emit: Emit,
    empty: boolean,
  ) {
    // Shift dates so "new" games in an older fixture still look new today.
    const shift = nowSeconds() - fixture.generatedAt;
    const move = (t: number | null) => (t == null ? t : t + shift);
    this.all = fixture.games.map((g) => ({
      ...g,
      releaseDate: move(g.releaseDate),
      firstSeenAt: g.firstSeenAt + shift,
      syncedAt: g.syncedAt + shift,
    }));
    // The three newest games only appear after "Yeni çıkanları getir".
    this.held = [...this.all]
      .sort((a, b) => (b.releaseDate ?? 0) - (a.releaseDate ?? 0))
      .slice(0, 3)
      .map((g) => g.appid);
    if (!empty) {
      for (const g of this.all) if (!this.held.includes(g.appid)) this.present.add(g.appid);
    }
    this.lastSyncAt = empty ? null : nowSeconds() - 2 * 86_400;

    for (const [appid, list] of Object.entries(fixture.storeMatches ?? synthesizeMatches(this.all))) {
      this.allMatches.set(Number(appid), list);
    }
    if (!empty) {
      this.matches = new Map([...this.allMatches].map(([k, v]) => [k, v.map((m) => ({ ...m }))]));
      this.lastStoreSyncAt = nowSeconds() - 86_400;
    }
  }

  /** Answers a command with a copy of the result, as IPC (which serializes) would: the UI
   *  must never hold the mock's own objects, which change later. */
  async handle(cmd: string, args: Record<string, any>): Promise<unknown> {
    const result = await this.dispatch(cmd, args);
    return result === undefined ? result : structuredClone(result);
  }

  private async dispatch(cmd: string, args: Record<string, any>): Promise<unknown> {
    switch (cmd) {
      case "get_status":
        return this.status();
      case "start_sync":
        return this.startWorker("full");
      case "fetch_new_releases":
        return this.startWorker("new_releases");
      case "cancel_sync":
        this.cancelled = true;
        return null;
      case "query_games":
        await sleep(60);
        return this.query(args.params as GameQuery);
      case "get_game":
        return this.present.has(args.appid) ? this.find(args.appid) : null;
      case "list_tags":
        return this.tags();
      case "get_game_media": {
        await sleep(350);
        const media = this.fixture.media[String(args.appid)];
        return { descriptionTr: null, screenshots: [], trailers: [], reviews: null, ...media } satisfies GameMedia;
      }
      case "get_game_reviews":
        await sleep(450);
        return fakeReviews(args.appid);
      case "get_game_requirements":
        await sleep(400);
        return fakeRequirements();
      case "start_store_sync":
        return this.startWorker("stores");
      case "get_store_matches":
        return this.storeMatches(args.appid);
      case "refresh_store_matches":
        await sleep(700);
        return this.storeMatches(args.appid);
      case "set_match_state":
        return this.setMatchState(args.store, args.productId, args.appid, args.state);
      case "open_store_page":
      case "open_account_page":
      case "open_gog_login_page":
        console.info(`[mock] ${cmd}`, args);
        return null;
      case "search_store":
        await sleep(600);
        return this.searchStore(args.appid);
      case "link_store_product":
        return this.linkStoreProduct(args.store, args.productId, args.appid);
      case "get_accounts":
        return this.accounts;
      case "gog_login_url":
        return "https://auth.gog.com/auth?client_id=46899977096215655";
      case "gog_login":
        await sleep(1500);
        return this.signIn("gog", "oyuncu");
      case "gog_login_with_code":
        if (!String(args.redirect).includes("code=")) throw invalid("gog_code");
        return this.signIn("gog", "oyuncu");
      case "itch_set_key":
        await sleep(500);
        if (String(args.key).trim().length < 8) throw invalid("itch_key");
        return this.signIn("itch", "Oyuncu");
      case "sign_out":
        return this.signOut(args.store);
      case "itad_set_key":
        await sleep(500);
        if (String(args.key).trim().length < 8) throw invalid("itad_key");
        this.accounts = { ...this.accounts, itad: { savedAt: nowSeconds() } };
        return this.accounts;
      case "itad_remove_key":
        this.accounts = { ...this.accounts, itad: null };
        return this.accounts;
      case "get_game_prices":
        await sleep(500);
        return this.accounts.itad ? fakePrices(args.appid) : null;
      case "start_library_sync":
        return this.startWorker("library");
      case "get_library":
        return this.library();
      case "get_settings":
        return this.settings;
      case "update_settings":
        if (args.patch.libraryDir != null && !/^([a-z]:\\|\/)/i.test(args.patch.libraryDir)) throw invalid("library_dir");
        this.settings = { ...this.settings, ...args.patch };
        return this.settings;
      case "pick_library_dir":
        await sleep(300);
        this.settings = { ...this.settings, libraryDir: "D:\\Oyunlar" };
        return this.settings;
      case "get_store_files":
        await sleep(500);
        return this.storeFiles(args.store, args.productId);
      case "enqueue_download":
        return this.enqueue(args.store, args.productId, args.optionId);
      case "enqueue_torrent":
        return this.enqueueTorrent(args.appid, args.title, args.source);
      case "get_update_status":
        return this.updateStatus();
      case "check_update":
        await sleep(500);
        this.updateCheckedAt = Math.floor(Date.now() / 1000);
        return this.updateStatus();
      case "install_update":
        return this.simulateUpdate();
      case "open_release_page":
        console.info(`[mock] ${cmd}`, args);
        return null;
      case "get_downloads":
        return { items: this.downloads, live: this.live, installing: this.installing } satisfies DownloadList;
      case "approve_install":
        return this.moveInstall(args.id, ["confirm"], "approved");
      case "retry_install":
        return this.moveInstall(args.id, [null, "failed"], "waiting");
      case "get_installs":
        return this.installs;
      case "launch_game": {
        const game = this.installs.find((i) => i.store === args.store && i.productId === args.productId);
        if (!game) throw { kind: "not_found", message: "not found" } satisfies CmdError;
        if (!game.exe) throw invalid("launch_target");
        console.info("[mock] launch", game.exe);
        return null;
      }
      case "uninstall_game":
        await sleep(900);
        return this.uninstall(args.store, args.productId);
      case "open_install_folder":
        console.info(`[mock] ${cmd}`, args);
        return null;
      case "set_launch_target":
        return this.setTarget(args.store, args.productId, args.exe);
      case "pick_launch_target": {
        const game = this.installs.find((i) => i.store === args.store && i.productId === args.productId);
        return game ? this.setTarget(args.store, args.productId, `${game.dir ?? "C:\\Program Files\\Game"}\\Game.exe`) : null;
      }
      case "pause_download":
        return this.pauseDownload(args.id);
      case "resume_download":
        return this.resumeDownload(args.id);
      case "remove_download":
        return this.removeDownload(args.id);
      case "clear_finished_downloads":
        for (const d of this.downloads.filter((x) => x.state === "completed")) this.emit("download:state", { id: d.id, removed: true });
        this.downloads = this.downloads.filter((d) => d.state !== "completed");
        return null;
      case "open_download_folder":
        console.info(`[mock] ${cmd}`, args);
        return null;
      case "list_sites":
        return SITES;
      case "list_links":
        return this.links.filter((l) => l.appid === args.appid);
      case "find_links":
        await sleep(900);
        return this.findLinks(args.appid as number);
      case "save_link":
        await sleep(150);
        return this.saveLink(args.input as LinkInput);
      case "delete_link": {
        const before = this.links.length;
        this.links = this.links.filter((l) => l.id !== args.id);
        return this.links.length < before;
      }
      case "check_link":
        await sleep(900);
        return this.checkLink(args.id as number);
      case "open_link":
      case "open_price_link":
      case "open_in_steam":
      case "open_browser":
      case "open_search":
        console.info(`[mock] ${cmd}`, args);
        return null;
      default:
        throw { kind: "other", message: `mock: unknown command ${cmd}` } satisfies CmdError;
    }
  }

  private find(appid: number): GameDetail {
    const g = this.all.find((x) => x.appid === appid)!;
    return { ...g, linkCount: this.links.filter((l) => l.appid === appid).length, stores: this.storesOf(appid) };
  }

  /** Canned site-search results, so the browser preview shows the feature without a network. */
  private findLinks(appid: number): FoundLink[] {
    const game = this.all.find((x) => x.appid === appid);
    if (!game) return [];
    const slug = game.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "");
    return [
      {
        siteId: "fitgirl",
        url: `magnet:?xt=urn:btih:${"7f9f2ea2a2bd89c65d14ed816987938fd5d48b07"}&dn=${encodeURIComponent(game.name)}`,
        label: `${game.name} [FitGirl Repack]`,
        kind: "download",
        version: "1.0",
        size: "31.7 GB",
        notes: null,
        score: 1,
        needsBrowser: false,
        direct: true,
      },
      {
        siteId: "ankergames",
        url: `https://ankergames.to/game/${slug}`,
        label: game.name,
        kind: "page",
        version: "1.0",
        size: "26.6 GB",
        notes: "İndirme bağlantısı için sayfada doğrulama adımı var.",
        score: 1,
        needsBrowser: true,
        direct: false,
      },
      {
        siteId: "astralgames",
        url: `https://astralgames.net/game/${slug}`,
        label: game.name,
        kind: "page",
        version: null,
        size: null,
        notes: null,
        score: 1,
        needsBrowser: true,
        direct: false,
      },
      {
        siteId: "gamebounty",
        url: `https://gamebounty.world/${slug}-free-pc-download`,
        label: game.name,
        kind: "page",
        version: null,
        size: null,
        notes: null,
        score: 1,
        needsBrowser: true,
        direct: false,
      },
      {
        siteId: "steamrip",
        url: `https://steamrip.com/${slug}-free-download/`,
        label: game.name,
        kind: "page",
        version: null,
        size: null,
        notes: null,
        score: 1,
        needsBrowser: true,
        direct: false,
      },
      {
        siteId: "gog-rev",
        url: `https://gog-rev.com/games/${slug.replace(/-/g, "_")}`,
        label: game.name,
        kind: "page",
        version: null,
        size: null,
        notes: null,
        score: 0.75,
        needsBrowser: true,
        direct: false,
      },
    ];
  }

  /** Stores with a confident match for a game. */
  private storesOf(appid: number): AccountStore[] {
    const stores = new Set((this.matches.get(appid) ?? []).filter(isConfident).map((m) => m.store));
    return (["gog", "itch"] as AccountStore[]).filter((s) => stores.has(s));
  }

  private storeMatches(appid: number): StoreMatch[] {
    return (this.matches.get(appid) ?? [])
      .filter((m) => m.state !== "rejected")
      .map((m) => ({ ...m, confident: isConfident(m), owned: this.owned.has(`${m.store}:${m.productId}`) }))
      .sort((a, b) => a.store.localeCompare(b.store) || Number(b.confident) - Number(a.confident) || b.score - a.score);
  }

  private signIn(store: AccountStore, username: string): Accounts {
    this.accounts = { ...this.accounts, [store]: { username } };
    // Like the app: when another job runs, the library is read once it ends.
    void this.startWorker("library").catch(() => (this.libraryPending = true));
    return this.accounts;
  }

  private signOut(store: AccountStore): Accounts {
    this.accounts = { ...this.accounts, [store]: null };
    for (const key of [...this.owned]) if (key.startsWith(`${store}:`)) this.owned.delete(key);
    if (store === "itch") this.itchLibrary = [];
    return this.accounts;
  }

  /** Signed-in accounts "own" a few of the games that have store matches. */
  private readLibrary(): number {
    if (this.accounts.gog) {
      const gog = this.visible()
        .sort((a, b) => b.reviewCount - a.reviewCount)
        .flatMap((g) => (this.matches.get(g.appid) ?? []).filter((m) => m.store === "gog" && isConfident(m)))
        .slice(0, 9);
      for (const m of gog) this.owned.add(`gog:${m.productId}`);
    }
    if (this.accounts.itch) {
      const [a, b] = this.visible().filter((g) => g.isFree || g.reviewCount < 50_000);
      this.itchLibrary = [
        ...[a, b].filter(Boolean).map((g) => ({
          store: "itch" as const,
          productId: String(9_000_000 + g!.appid),
          title: g!.name,
          url: "https://itch.io",
          cover: null,
          coverWide: g!.header,
          win: true,
          mac: false,
          linux: false,
          appid: g!.appid,
          steamHeader: g!.header,
          steamCapsule: g!.capsule,
        })),
        {
          store: "itch",
          productId: "9999001",
          title: "Obscure Jam Game",
          url: "https://itch.io",
          cover: null,
          coverWide: null,
          win: true,
          mac: true,
          linux: true,
          appid: null,
          steamHeader: null,
          steamCapsule: null,
        },
      ];
      for (const i of this.itchLibrary) this.owned.add(`itch:${i.productId}`);
    }
    return this.library().length;
  }

  private library(): LibraryItem[] {
    const gog: LibraryItem[] = [];
    for (const [appid, list] of this.matches) {
      const g = this.all.find((x) => x.appid === appid);
      for (const m of list) {
        if (m.store !== "gog" || !this.owned.has(`gog:${m.productId}`) || !g) continue;
        gog.push({
          store: "gog",
          productId: m.productId,
          title: m.title,
          url: m.url,
          cover: m.cover,
          coverWide: m.coverWide,
          win: m.win,
          mac: m.mac,
          linux: m.linux,
          appid,
          steamHeader: g.header,
          steamCapsule: g.capsule,
        });
      }
    }
    return [...gog, ...this.itchLibrary].sort((a, b) => a.title.localeCompare(b.title, "tr"));
  }

  // --- downloads -------------------------------------------------------------------------------

  /** A store's variants for a product, like the real APIs list them. */
  private storeFiles(store: AccountStore, productId: string): FileOption[] {
    if (!this.accounts[store]) throw invalid(store === "gog" ? "gog_signed_out" : "itch_signed_out");
    const title = this.productTitle(store, productId);
    const slug =
      fold(title)
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_|_$/g, "") || "game";
    const option = (id: string, label: string, platform: FileOption["platform"], size: number, extra: Partial<FileOption> = {}) =>
      ({
        id,
        label,
        platform,
        language: null,
        version: null,
        size,
        files: 1,
        demo: false,
        recommended: false,
        ...extra,
      }) satisfies FileOption;
    if (store === "gog") {
      const gb = 1024 ** 3;
      return [
        option("installer_windows_tr", "Windows · Türkçe · 1.6.2", "win", 23.4 * gb, {
          language: "tr",
          version: "1.6.2",
          files: 6,
          recommended: true,
        }),
        option("installer_windows_en", "Windows · English · 1.6.2", "win", 23.1 * gb, { language: "en", version: "1.6.2", files: 6 }),
        option("installer_mac_en", "macOS · English · 1.6.2", "mac", 24.8 * gb, { language: "en", version: "1.6.2" }),
      ];
    }
    const mb = 1024 ** 2;
    return [
      option("1", `${slug}-windows.zip`, "win", 812 * mb, { recommended: true }),
      option("2", `${slug}-linux.tar.gz`, "linux", 798 * mb),
      option("3", `${slug}-demo-setup.exe`, "win", 210 * mb, { demo: true }),
    ];
  }

  private productTitle(store: AccountStore, productId: string): string {
    const owned = this.library().find((i) => i.store === store && i.productId === productId);
    if (owned) return owned.title;
    for (const list of this.matches.values()) {
      const m = list.find((x) => x.store === store && x.productId === productId);
      if (m) return m.title;
    }
    throw { kind: "not_found", message: "not found" } satisfies CmdError;
  }

  private enqueue(store: AccountStore, productId: string, optionId: string): Download {
    const existing = this.downloads.find(
      (d) => d.store === store && d.productId === productId && d.optionId === optionId && d.state !== "completed",
    );
    if (existing) {
      this.resumeDownload(existing.id);
      return existing;
    }
    const option = this.storeFiles(store, productId).find((o) => o.id === optionId);
    if (!option) throw invalid("no_files");
    const appid = this.library().find((i) => i.store === store && i.productId === productId)?.appid ?? null;
    const id = this.nextDownloadId++;
    const download: Download = {
      id,
      store,
      sourceKind: "http",
      productId,
      appid,
      title: this.productTitle(store, productId),
      optionId,
      optionLabel: option.label,
      platform: option.platform,
      state: "queued",
      totalBytes: option.size,
      doneBytes: 0,
      dir: `${this.settings.libraryDir}\\.gamelib\\downloads\\${id}`,
      files: option.files,
      error: null,
      createdAt: nowSeconds(),
      finishedAt: null,
      installState: null,
      installKind: null,
      installError: null,
    };
    this.downloads = [download, ...this.downloads];
    this.emit("download:state", download);
    this.pump();
    return download;
  }

  /** A torrent's size is only known from its metadata; the preview assumes a plausible one. */
  private enqueueTorrent(appid: number, title: string, source: string): Download {
    const magnet = source.trim().toLowerCase().startsWith("magnet:");
    let optionId: string;
    if (magnet) {
      const hash = /[?&]xt=urn:btih:([0-9a-f]{40})(?:&|$)/i.exec(source)?.[1];
      if (!hash) throw invalid("torrent_parse");
      optionId = hash.toLowerCase();
    } else {
      let url: URL;
      try {
        url = new URL(source.trim());
      } catch {
        throw invalid(source.trim() ? "url_parse" : "url_empty");
      }
      if (url.protocol !== "http:" && url.protocol !== "https:") throw invalid("url_scheme");
      optionId = url.toString();
    }
    const existing = this.downloads.find(
      (d) => d.sourceKind === "torrent" && d.productId === String(appid) && d.optionId === optionId && d.state !== "completed",
    );
    if (existing) {
      this.resumeDownload(existing.id);
      return existing;
    }
    const id = this.nextDownloadId++;
    const download: Download = {
      id,
      store: "web",
      sourceKind: "torrent",
      productId: String(appid),
      appid,
      title,
      optionId,
      optionLabel: title,
      platform: null,
      state: "queued",
      totalBytes: 12 * 1024 ** 3,
      doneBytes: 0,
      dir: `${this.settings.libraryDir}\\.gamelib\\downloads\\${id}`,
      files: 1,
      error: null,
      createdAt: nowSeconds(),
      finishedAt: null,
      installState: null,
      installKind: null,
      installError: null,
    };
    this.downloads = [download, ...this.downloads];
    this.emit("download:state", download);
    this.pump();
    return download;
  }

  private setDownload(id: number, patch: Partial<Download>): Download | undefined {
    const d = this.downloads.find((x) => x.id === id);
    if (!d) return undefined;
    Object.assign(d, patch);
    this.emit("download:state", { ...d });
    return d;
  }

  private pauseDownload(id: number): null {
    const d = this.downloads.find((x) => x.id === id);
    if (!d) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (d.state === "downloading") this.stopTransfer();
    if (d.state === "downloading" || d.state === "queued") this.setDownload(id, { state: "paused" });
    this.pump();
    return null;
  }

  private resumeDownload(id: number): null {
    const d = this.downloads.find((x) => x.id === id);
    if (!d) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (d.state === "paused" || d.state === "failed") this.setDownload(id, { state: "queued", error: null });
    this.pump();
    return null;
  }

  private removeDownload(id: number): null {
    const d = this.downloads.find((x) => x.id === id);
    if (!d) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (d.state === "downloading") this.stopTransfer();
    this.downloads = this.downloads.filter((x) => x.id !== id);
    this.emit("download:state", { id, removed: true });
    this.pump();
    return null;
  }

  private stopTransfer() {
    if (this.transfer) clearInterval(this.transfer);
    this.transfer = null;
    this.live = null;
  }

  /** Starts the oldest queued download when nothing runs. Each takes about 12 seconds. */
  private pump() {
    if (this.transfer || this.downloads.some((d) => d.state === "downloading")) return;
    const next = [...this.downloads].reverse().find((d) => d.state === "queued");
    if (!next) return;
    this.setDownload(next.id, { state: "downloading" });
    const step = Math.max(1, Math.round(next.totalBytes / 48));
    let verifying = 0;
    this.transfer = setInterval(() => {
      const d = this.downloads.find((x) => x.id === next.id);
      if (!d || d.state !== "downloading") return this.stopTransfer();
      if (d.doneBytes >= d.totalBytes) {
        verifying += 1;
        this.live = { id: d.id, doneBytes: d.doneBytes, totalBytes: d.totalBytes, speed: 0, eta: 0, stage: "verifying" };
        this.emit("download:progress", this.live);
        if (verifying < 4) return;
        this.stopTransfer();
        // A torrent's files stay in their folder: nothing is installed.
        const torrent = d.sourceKind === "torrent";
        this.setDownload(d.id, {
          state: "completed",
          finishedAt: nowSeconds(),
          installState: torrent ? "manual" : "waiting",
          installKind: torrent ? "torrent" : null,
        });
        this.pump();
        if (!torrent) void this.install(d.id);
        return;
      }
      d.doneBytes = Math.min(d.totalBytes, d.doneBytes + Math.round(step * (0.8 + Math.random() * 0.4)));
      const speed = step * 4;
      this.live = {
        id: d.id,
        doneBytes: d.doneBytes,
        totalBytes: d.totalBytes,
        speed,
        eta: Math.ceil((d.totalBytes - d.doneBytes) / speed),
        stage: "downloading",
      };
      this.emit("download:progress", this.live);
    }, 250);
  }

  private updateStatus(): UpdateStatus {
    const found = this.fakeUpdate && this.updateCheckedAt != null;
    return { configured: true, currentVersion: "0.1.0", checkedAt: this.updateCheckedAt, update: found ? { version: "0.2.0" } : null };
  }

  /** Downloads a fake update with progress; "restarting" just reloads the preview. */
  private async simulateUpdate(): Promise<null> {
    if (!this.fakeUpdate || this.updateCheckedAt == null) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (this.installing) throw { kind: "invalid", message: "install_running" } satisfies CmdError;
    const total = 9_400_000;
    for (let i = 1; i <= 10; i += 1) {
      await sleep(250);
      this.emit("update:progress", { downloaded: (total * i) / 10, total });
    }
    await sleep(800);
    window.location.reload();
    return null;
  }

  // --- installs --------------------------------------------------------------------------------

  private moveInstall(id: number, from: (Download["installState"] | null)[], to: Download["installState"]): null {
    const d = this.downloads.find((x) => x.id === id);
    if (!d) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (d.state === "completed" && from.includes(d.installState)) {
      this.setDownload(id, { installState: to, installError: null });
      void this.install(id);
    }
    return null;
  }

  /** GOG installers run (no progress), zips unpack, someone else's installer asks first. */
  private async install(id: number) {
    const d = this.downloads.find((x) => x.id === id);
    if (!d || (d.installState !== "waiting" && d.installState !== "approved")) return;
    const setup = d.optionLabel?.endsWith("setup.exe") ?? false;
    if (setup && d.installState !== "approved") {
      this.setDownload(id, { installState: "confirm", installKind: "nsis" });
      return;
    }
    const kind = d.store === "gog" ? "inno_setup" : setup ? "nsis" : "zip";
    this.setDownload(id, { installState: "installing", installKind: kind });
    const report = (stage: InstallProgress["stage"], done: number, total: number) => {
      this.installing = { downloadId: id, stage, done, total };
      this.emit("install:progress", this.installing);
    };
    report("checking", 0, 0);
    await sleep(500);
    if (kind === "zip") {
      for (let i = 1; i <= 10; i += 1) {
        report("unpacking", (d.totalBytes * i) / 10, d.totalBytes);
        await sleep(200);
      }
    } else {
      report("installing", 0, 0);
      await sleep(2500);
    }
    this.installing = null;
    if (!this.downloads.some((x) => x.id === id)) return;
    const dir = `${this.settings.libraryDir}\\${d.title.replace(/[<>:"/\\|?*]/g, "_")}`;
    const exe = `${dir}\\${d.store === "gog" ? "bin\\game.exe" : "Game.exe"}`;
    const game: Installed = {
      store: d.store,
      productId: d.productId,
      appid: d.appid,
      title: d.title,
      dir,
      exe,
      args: "",
      workdir: null,
      method: d.store === "gog" ? "gog" : setup ? "installer" : "archive",
      candidates: [exe, `${dir}\\Launcher.exe`],
      optionLabel: d.optionLabel,
      installedAt: nowSeconds(),
      external: false,
      steamHeader: d.appid != null ? (this.all.find((g) => g.appid === d.appid)?.header ?? null) : null,
    };
    this.installs = [...this.installs.filter((i) => !(i.store === d.store && i.productId === d.productId)), game];
    this.emit("install:changed", { store: d.store, productId: d.productId });
    this.setDownload(id, { installState: "installed" });
  }

  private uninstall(store: Store, productId: string): null {
    const before = this.installs.length;
    this.installs = this.installs.filter((i) => !(i.store === store && i.productId === productId));
    if (this.installs.length === before) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    for (const d of this.downloads) {
      if (d.store === store && d.productId === productId && d.installState === "installed") d.installState = null;
    }
    this.emit("install:changed", { store, productId });
    return null;
  }

  private setTarget(store: Store, productId: string, exe: string): Installed {
    const game = this.installs.find((i) => i.store === store && i.productId === productId);
    if (!game) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    if (!/^([a-z]:\\|\/)/i.test(exe)) throw invalid("launch_target");
    Object.assign(game, { exe, args: "", workdir: null });
    this.emit("install:changed", { store, productId });
    return { ...game };
  }

  private searchStore(appid: number): StoreSearchHit[] {
    if (!this.accounts.itch) throw invalid("itch_signed_out");
    const g = this.find(appid);
    const hit = (productId: string, title: string, score: number): StoreSearchHit => ({
      store: "itch",
      productId,
      title,
      url: "https://itch.io",
      coverWide: g.header,
      developer: g.developers[0] ?? null,
      price: g.isFree ? null : "$9.99",
      isFree: g.isFree,
      win: true,
      mac: false,
      linux: false,
      score,
    });
    return [hit(String(8_000_000 + appid), g.name, 1), hit(String(8_500_000 + appid), `${g.name} Demake`, 0)];
  }

  private linkStoreProduct(store: AccountStore, productId: string, appid: number): null {
    const g = this.find(appid);
    const list = this.matches.get(appid) ?? [];
    list.push({
      store,
      productId,
      title: g.name,
      url: "https://itch.io",
      cover: null,
      coverWide: g.header,
      price: null,
      isFree: g.isFree,
      owned: false,
      win: true,
      mac: false,
      linux: false,
      method: "manual",
      score: 1,
      state: "confirmed",
      confident: true,
    });
    this.matches.set(appid, list);
    return null;
  }

  private setMatchState(store: AccountStore, productId: string, appid: number, state: MatchState): null {
    const match = (this.matches.get(appid) ?? []).find((m) => m.store === store && m.productId === productId);
    if (!match) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    match.state = state;
    return null;
  }

  private visible(): GameDetail[] {
    return this.all.filter((g) => this.present.has(g.appid)).map((g) => this.find(g.appid));
  }

  private status(): AppStatus {
    const visible = this.visible();
    return {
      gameCount: visible.length,
      tagCount: this.tags().length,
      linkedGameCount: new Set(this.links.map((l) => l.appid)).size,
      lastSyncAt: this.lastSyncAt,
      lastNewReleasesAt: this.lastNewReleasesAt,
      resumable: false,
      storeCounts: {
        gog: visible.filter((g) => g.stores.includes("gog")).length,
        itch: visible.filter((g) => g.stores.includes("itch")).length,
        owned: this.owned.size,
        gogProducts: this.lastStoreSyncAt == null ? 0 : 8204,
        lastStoreSyncAt: this.lastStoreSyncAt,
      },
      worker: this.worker,
      progress: this.progress,
      dbPath: "(tarayıcı önizlemesi)",
    };
  }

  private tags(): TagInfo[] {
    const counts = new Map<number, number>();
    for (const g of this.visible()) for (const t of g.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
    return this.fixture.tags
      .filter((t) => counts.has(t.tagid))
      .map((t) => ({ ...t, gameCount: counts.get(t.tagid)! }))
      .sort((a, b) => b.gameCount - a.gameCount || a.name.localeCompare(b.name, "tr"));
  }

  private query(q: GameQuery): GamePage {
    const now = nowSeconds();
    const term = q.search ? normalizeName(q.search) : "";
    const words = term.split(" ").filter(Boolean);
    let rows = this.visible().filter((g) => {
      if (g.adult && !q.showAdult) return false;
      if (words.length) {
        const tokens = normalizeName(g.name).split(" ");
        const byName = words.every((w) => tokens.some((t) => t.startsWith(w)));
        if (!byName && String(g.appid) !== term) return false;
      }
      if (!q.tags.every((t) => g.tags.includes(t))) return false;
      if (!q.platforms.every((p) => g[p])) return false;
      if (q.deck === "playable" && g.deck < 2) return false;
      if (q.deck === "verified" && g.deck !== 3) return false;
      if (q.freeOnly && !g.isFree) return false;
      if (q.minReviewScore && g.reviewScore < q.minReviewScore) return false;
      if (q.releasedWithinDays && (g.releaseDate ?? 0) < now - q.releasedWithinDays * 86_400) return false;
      if (q.hasLinks && g.linkCount === 0) return false;
      if (q.stores.length && !q.stores.some((s) => g.stores.includes(s))) return false;
      if (q.sort === "oldest" && g.releaseDate == null) return false;
      return true;
    });
    const bucket = (n: number) => (n >= 1e6 ? 12 : n >= 1e5 ? 10 : n >= 1e4 ? 8 : n >= 1e3 ? 6 : n >= 100 ? 4 : n >= 10 ? 2 : 0);
    const relevance = (g: GameDetail) => {
      const name = normalizeName(g.name);
      return (name === term ? 0 : name.startsWith(term) ? 2 : 4) - bucket(g.reviewCount);
    };
    const by: Record<GameQuery["sort"], (a: GameDetail, b: GameDetail) => number> = {
      relevance: (a, b) => relevance(a) - relevance(b) || b.reviewCount - a.reviewCount,
      popular: (a, b) => b.reviewCount - a.reviewCount,
      rating: (a, b) => rating(b) - rating(a),
      newest: (a, b) => (b.releaseDate ?? 0) - (a.releaseDate ?? 0),
      oldest: (a, b) => (a.releaseDate ?? 0) - (b.releaseDate ?? 0),
      name: (a, b) => normalizeName(a.name).localeCompare(normalizeName(b.name)),
    };
    const sort = q.sort === "relevance" && !term ? "popular" : q.sort;
    rows = rows.sort((a, b) => by[sort](a, b) || a.appid - b.appid);
    const items: GameCard[] = rows.slice(q.offset, q.offset + (q.limit || 60));
    return { total: rows.length, items };
  }

  private async startWorker(kind: WorkerKind): Promise<null> {
    if (this.worker) throw { kind: "busy", message: "busy" } satisfies CmdError;
    this.worker = kind;
    this.cancelled = false;
    const run = {
      full: this.simulateFull,
      stores: this.simulateStores,
      library: this.simulateLibrary,
      new_releases: this.simulateNewReleases,
    };
    void run[kind].call(this);
    return null;
  }

  private report(progress: SyncProgress) {
    this.progress = progress;
    this.emit("sync:progress", progress);
  }

  private finish(finished: SyncFinished) {
    this.worker = null;
    this.progress = null;
    this.emit("sync:finished", finished);
    if (this.libraryPending) {
      this.libraryPending = false;
      void this.startWorker("library").catch(() => undefined);
    }
  }

  private async simulateFull() {
    const startedAt = nowSeconds();
    const total = 130_615;
    const pages = 134;
    const base = { kind: "full" as const, startedAt, resumed: false };
    this.report({ ...base, phase: "starting", fetched: 0, total: 0, page: 0, pages: 0 });
    await sleep(400);
    this.report({ ...base, phase: "tags", fetched: 0, total: 0, page: 0, pages: 0 });
    await sleep(400);
    this.report({ ...base, phase: "featured", fetched: 0, total, page: 0, pages });
    const byPopularity = [...this.all].sort((a, b) => b.reviewCount - a.reviewCount).filter((g) => !this.held.includes(g.appid));
    byPopularity.slice(0, 60).forEach((g) => this.present.add(g.appid));
    const steps = 24;
    for (let i = 1; i <= steps; i += 1) {
      await sleep(260);
      if (this.cancelled) {
        this.finish({ kind: "full", outcome: "cancelled", report: null, newReleases: null, stores: null, library: null, error: null });
        return;
      }
      byPopularity.slice(0, Math.round((byPopularity.length * i) / steps)).forEach((g) => this.present.add(g.appid));
      this.report({
        ...base,
        phase: "catalog",
        fetched: Math.round((total * i) / steps),
        total,
        page: Math.round((pages * i) / steps),
        pages,
      });
    }
    this.report({ ...base, phase: "finalizing", fetched: total, total, page: pages, pages });
    await sleep(500);
    this.lastSyncAt = nowSeconds();
    const seen = this.present.size;
    this.finish({
      kind: "full",
      outcome: "completed",
      report: {
        total,
        seen,
        inserted: seen,
        delisted: 0,
        skipped: 0,
        requests: pages + 1,
        retries: 0,
        durationMs: 7000,
        pruneSkipped: false,
        resumed: false,
        warnings: [],
      },
      newReleases: null,
      stores: null,
      library: null,
      error: null,
    });
  }

  private async simulateNewReleases() {
    const startedAt = nowSeconds();
    const base = { kind: "new_releases" as const, phase: "new_releases" as const, total: 0, startedAt, resumed: false };
    this.report({ ...base, fetched: 0, page: 0, pages: 1 });
    await sleep(1200);
    const added = this.held.filter((id) => !this.present.has(id));
    const empty = this.present.size === 0;
    const recent = this.all.filter((g) => (g.releaseDate ?? 0) >= nowSeconds() - 30 * 86_400);
    for (const g of empty ? recent : []) this.present.add(g.appid);
    for (const id of this.held) this.present.add(id);
    this.report({ ...base, fetched: 1000, page: 1, pages: 1 });
    await sleep(300);
    this.lastNewReleasesAt = nowSeconds();
    const inserted = empty ? recent.length + added.length : added.length;
    this.finish({
      kind: "new_releases",
      outcome: "completed",
      report: null,
      newReleases: {
        fetched: 1000,
        inserted,
        updated: 1000 - inserted,
        pages: 1,
        partial: false,
        since: nowSeconds() - 3 * 86_400,
        watermark: nowSeconds(),
        requests: 1,
        retries: 0,
        durationMs: 1500,
      },
      stores: null,
      library: null,
      error: null,
    });
  }

  private async simulateLibrary() {
    const startedAt = nowSeconds();
    const base = { kind: "library" as const, startedAt, resumed: false, page: 0, pages: 0 };
    this.report({ ...base, phase: "library", fetched: 0, total: 0 });
    await sleep(900);
    this.readLibrary();
    this.report({ ...base, phase: "matching", fetched: 12, total: 12 });
    await sleep(300);
    this.finish({
      kind: "library",
      outcome: "completed",
      report: null,
      newReleases: null,
      stores: null,
      library: {
        gogOwned: this.accounts.gog ? [...this.owned].filter((k) => k.startsWith("gog:")).length : null,
        itchOwned: this.accounts.itch ? this.itchLibrary.length : null,
        matched: this.library().filter((i) => i.appid != null).length,
        gogSignedOut: false,
        warnings: [],
      },
      error: null,
    });
  }

  private async simulateStores() {
    const startedAt = nowSeconds();
    const base = { kind: "stores" as const, startedAt, resumed: false, page: 0, pages: 0 };
    const catalog = 8204;
    for (let i = 1; i <= 8; i += 1) {
      await sleep(220);
      if (this.cancelled) {
        this.finish({ kind: "stores", outcome: "cancelled", report: null, newReleases: null, stores: null, library: null, error: null });
        return;
      }
      this.report({ ...base, phase: "gog_catalog", fetched: Math.round((catalog * i) / 8), total: catalog });
    }
    this.report({ ...base, phase: "matching", fetched: 6458, total: 6458 });
    await sleep(400);
    for (const [appid, list] of this.allMatches) {
      if (!this.matches.has(appid))
        this.matches.set(
          appid,
          list.map((m) => ({ ...m })),
        );
    }
    for (let i = 1; i <= 5; i += 1) {
      await sleep(250);
      this.report({ ...base, phase: "gog_ids", fetched: i * 20, total: 100 });
    }
    this.lastStoreSyncAt = nowSeconds();
    const matchedGames = [...this.matches.keys()].filter((id) => this.storesOf(id).includes("gog")).length;
    this.finish({
      kind: "stores",
      outcome: "completed",
      report: null,
      newReleases: null,
      stores: {
        catalog,
        inserted: 0,
        matchedGames,
        checked: 100,
        remaining: 0,
        requests: 183,
        retries: 0,
        durationMs: 3600,
        warnings: [],
        library: null,
      },
      library: null,
      error: null,
    });
  }

  private saveLink(input: LinkInput): GameLink {
    let raw = input.url.trim();
    if (!raw) throw invalid("url_empty");
    if (raw.length > 2048) throw invalid("url_too_long");
    // A magnet link has no host and no site handler: it is stored as it was given.
    const magnet = raw.toLowerCase().startsWith("magnet:");
    if (magnet && !/[?&]xt=urn:btih:[0-9a-f]{40}(?:&|$)/i.test(raw)) throw invalid("torrent_parse");
    if (!magnet && !raw.includes("://")) raw = `https://${raw}`;
    let url: URL | null = null;
    if (!magnet) {
      try {
        url = new URL(raw);
      } catch {
        throw invalid("url_parse");
      }
      if (url.protocol !== "http:" && url.protocol !== "https:") throw invalid("url_scheme");
      if (url.username || url.password) throw invalid("url_credentials");
      for (const key of [...url.searchParams.keys()]) if (/^utm_/i.test(key) || key === "fbclid") url.searchParams.delete(key);
    }
    const stored = url?.toString() ?? raw;
    const text = (v: string | null | undefined, max: number, code: string) => {
      const t = v?.trim() || null;
      if (t && t.length > max) throw invalid(code);
      return t;
    };
    const now = nowSeconds();
    const existing = input.id ? this.links.find((l) => l.id === input.id) : undefined;
    // Mirrors the core's `SiteRegistry::detect`: the first site owning the host, else generic.
    const host = magnet ? "" : (url?.hostname ?? "").toLowerCase();
    const site = host ? SITES.find((s) => s.domains.some((d) => host === d || host.endsWith(`.${d}`))) : undefined;
    const link: GameLink = {
      id: existing?.id ?? this.nextLinkId++,
      appid: input.appid,
      siteId: site?.id ?? "generic",
      url: stored,
      host: magnet ? "magnet" : (url?.hostname ?? ""),
      label: text(input.label, 120, "label_too_long"),
      kind: input.kind,
      platform: input.platform ?? null,
      version: text(input.version, 60, "version_too_long"),
      notes: text(input.notes, 1000, "notes_too_long"),
      insecure: !magnet && url?.protocol === "http:",
      lastCheck: existing && existing.url === stored ? existing.lastCheck : null,
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    };
    this.links = existing ? this.links.map((l) => (l.id === link.id ? link : l)) : [...this.links, link];
    return link;
  }

  private checkLink(id: number): LinkCheck {
    const link = this.links.find((l) => l.id === id);
    if (!link) throw { kind: "not_found", message: "not found" } satisfies CmdError;
    const url = new URL(link.url);
    const now = nowSeconds();
    const last = url.pathname.split("/").filter(Boolean).pop() ?? "";
    const isFile = /\.[a-z0-9]{2,5}$/i.test(last);
    const broken = fold(url.hostname).includes("broken");
    const finalHost = broken ? url.hostname : `cdn.${url.hostname.replace(/^www\./, "")}`;
    const check: LinkCheck = {
      status: broken ? "broken" : "ok",
      httpStatus: broken ? 404 : 200,
      finalUrl: `${url.protocol}//${finalHost}${url.pathname}`,
      finalHost,
      hops: broken
        ? [{ url: link.url, status: 404 }]
        : [
            { url: link.url, status: 302 },
            { url: `https://${url.hostname}/r${url.pathname}`, status: 302 },
            { url: `https://${finalHost}${url.pathname}`, status: 200 },
          ],
      fileName: isFile && !broken ? decodeURIComponent(last) : null,
      sizeBytes: isFile && !broken ? 1_288_490_189 : null,
      contentType: isFile ? "application/zip" : "text/html",
      isFile: isFile && !broken,
      checkedAt: now,
      message: null,
    };
    link.lastCheck = {
      status: check.status,
      httpStatus: check.httpStatus,
      resolvedUrl: check.finalUrl,
      finalHost: check.finalHost,
      redirects: Math.max(0, check.hops.length - 1),
      hops: check.hops,
      fileName: check.fileName,
      sizeBytes: check.sizeBytes,
      contentType: check.contentType,
      isFile: check.isFile,
      checkedAt: now,
    };
    return check;
  }
}

function rating(g: GameCard): number {
  if (g.reviewCount === 0) return 0;
  const avg = g.reviewPct / 100;
  return avg - (avg - 0.5) * 2 ** -Math.log10(g.reviewCount + 1);
}

function isConfident(m: StoreMatch): boolean {
  return m.state === "confirmed" || (m.state === "auto" && m.score >= CONFIDENT);
}

/** Plausible GOG matches for fixtures exported before store matching existed. */
function synthesizeMatches(games: GameDetail[]): Record<string, StoreMatch[]> {
  const out: Record<string, StoreMatch[]> = {};
  const popular = [...games].sort((a, b) => b.reviewCount - a.reviewCount);
  popular.forEach((g, i) => {
    if (i % 3 !== 0) return;
    out[String(g.appid)] = [
      {
        store: "gog",
        productId: String(1_100_000_000 + g.appid),
        title: g.name,
        url: `https://www.gog.com/en/game/${normalizeName(g.name).replace(/ /g, "_")}`,
        cover: null,
        coverWide: g.header,
        price: g.isFree ? null : (g.price ?? "$19.99"),
        isFree: g.isFree,
        owned: false,
        win: g.win,
        mac: g.mac,
        linux: g.linux,
        method: i % 2 === 0 ? "title" : "gamesdb",
        score: i % 9 === 0 ? 0.7 : 1,
        state: "auto",
        confident: i % 9 !== 0,
      },
    ];
  });
  return out;
}

/** Sample reviews for the preview: two Turkish, one English, and a latest-100 summary. */
function fakeReviews(appid: number): GameReviews {
  const now = nowSeconds();
  const positive = 55 + (appid % 45);
  return {
    top: [
      {
        id: `${appid}-1`,
        language: "turkish",
        positive: true,
        text: "Hikâyesi ve karakterleri çok iyi yazılmış. İlk saatler biraz yavaş ama sonrasında bırakamıyorsun.\n\n• Grafikler güzel\n• Türkçe altyazı var",
        helpful: 412,
        hoursAtReview: 38.5,
        hoursTotal: 120.2,
        created: now - 40 * 86_400,
        earlyAccess: false,
        receivedForFree: false,
      },
      {
        id: `${appid}-2`,
        language: "turkish",
        positive: false,
        text: "Son güncellemeden sonra performans düştü; orta seviye bir bilgisayarda sık sık takılıyor. Düzeltilene kadar indirimi beklemenizi öneririm.",
        helpful: 97,
        hoursAtReview: 6.1,
        hoursTotal: 6.4,
        created: now - 9 * 86_400,
        earlyAccess: false,
        receivedForFree: false,
      },
      {
        id: `${appid}-3`,
        language: "english",
        positive: true,
        text: "Great combat and exploration. Runs well on a GTX 1060 at medium settings.",
        helpful: 58,
        hoursAtReview: 12,
        hoursTotal: 30,
        created: now - 120 * 86_400,
        earlyAccess: true,
        receivedForFree: true,
      },
    ],
    recent: { count: 100, positive, from: now - 3 * 86_400, to: now - 600 },
  };
}

/** Requirements of a modern game against a mid-range Windows 11 computer. */
function fakeRequirements(): GameRequirements {
  const GB = 1024 ** 3;
  const line = (kind: RequirementLine["kind"], text: string): RequirementLine => ({ kind, label: null, text });
  return {
    platform: "win",
    minimum: {
      lines: [
        line("os", "Windows 10 64-bit"),
        line("processor", "Intel Core i5-8400 / AMD Ryzen 5 2600"),
        line("memory", "12 GB RAM"),
        line("graphics", "NVIDIA GeForce GTX 1660 / AMD Radeon RX 5500 XT (6 GB VRAM)"),
        line("directx", "Version 12"),
        line("storage", "70 GB available space"),
        line("notes", "SSD required"),
      ],
      checks: [
        { kind: "memory", need: 12 * GB, have: 16 * GB, verdict: "ok" },
        { kind: "video_memory", need: 6 * GB, have: 12 * GB, verdict: "ok" },
        { kind: "storage", need: 70 * GB, have: 180 * GB, verdict: "ok" },
        { kind: "ssd", need: 1, have: 1, verdict: "ok" },
        { kind: "directx", need: 12, have: 12, verdict: "ok" },
        { kind: "windows", need: 10, have: 11, verdict: "ok" },
        { kind: "bits64", need: 1, have: 1, verdict: "ok" },
      ],
    },
    recommended: {
      lines: [
        line("os", "Windows 11 64-bit"),
        line("processor", "Intel Core i7-12700 / AMD Ryzen 7 5800X"),
        line("memory", "32 GB RAM"),
        line("graphics", "NVIDIA GeForce RTX 3070 / AMD Radeon RX 6800 (8 GB VRAM)"),
        line("directx", "Version 12"),
        line("storage", "70 GB available space"),
      ],
      checks: [
        { kind: "memory", need: 32 * GB, have: 16 * GB, verdict: "short" },
        { kind: "video_memory", need: 8 * GB, have: 12 * GB, verdict: "ok" },
        { kind: "storage", need: 70 * GB, have: 180 * GB, verdict: "ok" },
        { kind: "directx", need: 12, have: 12, verdict: "ok" },
        { kind: "windows", need: 11, have: 11, verdict: "ok" },
        { kind: "bits64", need: 1, have: 1, verdict: "ok" },
      ],
    },
    pc: {
      os: "Windows 11 24H2 (26100)",
      windows: 11,
      bits64: true,
      cpu: "AMD Ryzen 5 5600X 6-Core Processor",
      cores: 12,
      memory: 16 * GB,
      gpu: "NVIDIA GeForce RTX 3060",
      videoMemory: 12 * GB,
      directx: 12,
      diskFree: 180 * GB,
      diskSsd: true,
      diskPath: "C:\\Users\\oyuncu\\Games",
    },
  };
}

/** IsThereAnyDeal-like prices: three shops, lows, a subscription, a bundle and two years of Steam history. */
function fakePrices(appid: number): GamePrices {
  const now = nowSeconds();
  const day = 86_400;
  const usd = (amount: number) => ({ amount, currency: "USD" });
  const regular = 19.99 + (appid % 3) * 10;
  const at = (daysAgo: number) => now - daysAgo * day;
  return {
    found: true,
    url: "https://isthereanydeal.com/game/example/info/",
    deals: [
      {
        shop: "GOG",
        price: usd(+(regular * 0.3).toFixed(2)),
        regular: usd(regular),
        cut: 70,
        storeLow: usd(+(regular * 0.25).toFixed(2)),
        drm: ["DRM Free"],
        expiry: now + 5 * day,
        url: "https://itad.link/example/35/",
      },
      {
        shop: "Steam",
        price: usd(+(regular * 0.5).toFixed(2)),
        regular: usd(regular),
        cut: 50,
        storeLow: usd(+(regular * 0.25).toFixed(2)),
        drm: ["Steam"],
        expiry: now + 9 * day,
        url: "https://itad.link/example/61/",
      },
      {
        shop: "Humble Store",
        price: usd(regular),
        regular: usd(regular),
        cut: 0,
        storeLow: null,
        drm: ["Steam"],
        expiry: null,
        url: "https://itad.link/example/37/",
      },
    ],
    lowest: { shop: "Steam", price: usd(+(regular * 0.25).toFixed(2)), regular: usd(regular), cut: 75, at: at(300) },
    lowestYear: usd(+(regular * 0.25).toFixed(2)),
    lowestMonths: usd(+(regular * 0.3).toFixed(2)),
    subscriptions: appid % 2 === 0 ? [{ name: "PC Game Pass", leaving: now + 60 * day }] : [],
    bundles: [
      {
        title: "Macera Paketi",
        store: "Fanatical",
        price: usd(7.49),
        expiry: now + 12 * day,
        url: "https://isthereanydeal.com/bundles/1/",
      },
    ],
    history: [
      { at: at(700), price: regular, regular, cut: 0 },
      { at: at(560), price: +(regular * 0.5).toFixed(2), regular, cut: 50 },
      { at: at(546), price: regular, regular, cut: 0 },
      { at: at(420), price: +(regular * 0.4).toFixed(2), regular, cut: 60 },
      { at: at(406), price: regular, regular, cut: 0 },
      { at: at(300), price: +(regular * 0.25).toFixed(2), regular, cut: 75 },
      { at: at(286), price: regular, regular, cut: 0 },
      { at: at(150), price: +(regular * 0.4).toFixed(2), regular, cut: 60 },
      { at: at(136), price: regular, regular, cut: 0 },
      { at: at(4), price: +(regular * 0.5).toFixed(2), regular, cut: 50 },
    ],
  };
}
