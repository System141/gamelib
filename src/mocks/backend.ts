// In-browser stand-in for the Rust backend, used only by the dev preview (`pnpm dev` outside
// Tauri). It answers every command from a fixture of real games exported by
// `gamelib-cli export-fixture` and simulates catalog downloads with progress events.

import { fold, normalizeName } from "../lib/fold";
import { nowSeconds } from "../lib/format";
import type {
  Accounts,
  AppStatus,
  CmdError,
  GameCard,
  GameDetail,
  GameLink,
  GameMedia,
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
  WorkerKind,
} from "../lib/types";

export interface Fixture {
  generatedAt: number;
  games: GameDetail[];
  tags: TagInfo[];
  media: Record<string, GameMedia>;
  /** Store matches of fixture games, by appid (exported after a store sync). */
  storeMatches?: Record<string, StoreMatch[]>;
}

/** Matches count when confirmed, or automatic with this score. Mirrors matching::CONFIDENT. */
const CONFIDENT = 0.85;

type Emit = (event: string, payload: unknown) => void;

const SITES: SiteInfo[] = [{ id: "generic", name: "Other site", homepage: null, domains: [], color: "#8b93a7" }];
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
  private accounts: Accounts = { gog: null, itch: null };
  /** Owned products as `store:productId`. */
  private owned = new Set<string>();
  private itchLibrary: LibraryItem[] = [];
  private settings: Settings = { libraryDir: "C:\\Users\\oyuncu\\Games", keepInstallers: false, autoUpdate: true };

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

  async handle(cmd: string, args: Record<string, any>): Promise<unknown> {
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
      case "get_game_media":
        await sleep(350);
        return this.fixture.media[String(args.appid)] ?? { descriptionTr: null, screenshots: [] };
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
      case "list_sites":
        return SITES;
      case "list_links":
        return this.links.filter((l) => l.appid === args.appid);
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
      case "open_in_steam":
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

  /** Stores with a confident match for a game. */
  private storesOf(appid: number): Store[] {
    const stores = new Set((this.matches.get(appid) ?? []).filter(isConfident).map((m) => m.store));
    return (["gog", "itch"] as Store[]).filter((s) => stores.has(s));
  }

  private storeMatches(appid: number): StoreMatch[] {
    return (this.matches.get(appid) ?? [])
      .filter((m) => m.state !== "rejected")
      .map((m) => ({ ...m, confident: isConfident(m), owned: this.owned.has(`${m.store}:${m.productId}`) }))
      .sort((a, b) => a.store.localeCompare(b.store) || Number(b.confident) - Number(a.confident) || b.score - a.score);
  }

  private signIn(store: Store, username: string): Accounts {
    this.accounts = { ...this.accounts, [store]: { username } };
    void this.startWorker("library").catch(() => undefined);
    return this.accounts;
  }

  private signOut(store: Store): Accounts {
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

  private linkStoreProduct(store: Store, productId: string, appid: number): null {
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

  private setMatchState(store: Store, productId: string, appid: number, state: MatchState): null {
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
    if (!raw.includes("://")) raw = `https://${raw}`;
    let url: URL;
    try {
      url = new URL(raw);
    } catch {
      throw invalid("url_parse");
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") throw invalid("url_scheme");
    if (url.username || url.password) throw invalid("url_credentials");
    for (const key of [...url.searchParams.keys()]) if (/^utm_/i.test(key) || key === "fbclid") url.searchParams.delete(key);
    const text = (v: string | null | undefined, max: number, code: string) => {
      const t = v?.trim() || null;
      if (t && t.length > max) throw invalid(code);
      return t;
    };
    const now = nowSeconds();
    const existing = input.id ? this.links.find((l) => l.id === input.id) : undefined;
    const link: GameLink = {
      id: existing?.id ?? this.nextLinkId++,
      appid: input.appid,
      siteId: "generic",
      url: url.toString(),
      host: url.hostname,
      label: text(input.label, 120, "label_too_long"),
      kind: input.kind,
      platform: input.platform ?? null,
      version: text(input.version, 60, "version_too_long"),
      notes: text(input.notes, 1000, "notes_too_long"),
      insecure: url.protocol === "http:",
      lastCheck: existing && existing.url === url.toString() ? existing.lastCheck : null,
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
