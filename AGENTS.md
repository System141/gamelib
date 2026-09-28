# Repository Guidelines

## Project Overview

GameLib is a Tauri 2 desktop app (Windows/macOS/Linux) that mirrors the released Steam catalog (~130k games) into a local SQLite database and presents it in a React SPA. It also signs in to GOG and itch.io to read owned libraries, download/install/launch those games, and store user-added external links per game.

- UI language is Turkish-only (`<html lang="tr">`), region hard-wired to Turkey (USD prices, English descriptions). `README.md` is the user-facing doc and is written in Turkish.
- One Rust workspace with a React frontend; the browser preview is served by the CLI, not by a standalone web backend.
- Not implemented (roadmap — do not assume it exists): per-site link handlers beyond the generic one, link export/import, local Steam library scan, favorites, image cache.

## Architecture & Data Flow

Workspace members (root `Cargo.toml`, `default-members` deliberately excludes `src-tauri`):

| Crate | Path | Role |
|---|---|---|
| `gamelib-core` | `crates/gamelib-core` | All domain logic. Zero Tauri dependency; only coupling to shells is the `EventSink` trait (`src/app.rs`). |
| `gamelib-cli` | `crates/gamelib-cli` | Headless CLI; `serve` re-exposes the same command surface over loopback HTTP (`127.0.0.1:1430`) for the Vite browser preview. |
| `gamelib` (lib `gamelib_lib`) | `src-tauri` | Thin shell: 51 `#[tauri::command]` wrappers, `tauri-plugin-opener`/`dialog`, window + CSP config. |

Layering inside `gamelib-core/src/` (all modules declared in `lib.rs`):

1. Helpers: `text.rs`, `date.rs`, `search.rs`, `rating.rs`, `error.rs`, `http.rs`
2. API clients: `steam/` (`CatalogSource` trait + `SteamClient`), `stores/` (GOG catalog/account, itch.io, GamesDB, title matching, library)
3. Persistence: `db/` (rusqlite, hand-written SQL, no ORM) + `secrets.rs` (file, not DB)
4. Pipelines: `sync.rs`, `new_releases.rs`, `downloads/`, `install/`, `links/`
5. Wire models: `record.rs` (Steam item → `games` row), `model.rs` (all DTOs shared with the frontend)
6. `app.rs` — `pub struct App`; its public methods **are** the IPC command set, shared by both shells.

Core invariants:

- **Synchronous/blocking by design.** No tokio, no `async fn`, `reqwest` `blocking` feature, `std::thread` workers. Tauri commands wrap DB/network work in `blocking()` (`tauri::async_runtime::spawn_blocking`, `src-tauri/src/commands.rs`); pure flag flips stay sync `fn`.
- **One DB connection per worker.** Threads `gamelib-job`, `gamelib-downloads`, `gamelib-installs` each open their own `Db`. One catalog job at a time (in-process `AtomicBool` + `<db>.job-lock` file lock, so desktop and CLI can't sync the same DB concurrently).
- **Never hold `App`'s `Mutex<Db>` across a network request or spawned process** (explicit comments in `app.rs`, `install/mod.rs`).
- **`EventSink` is the only outbound path to the UI:** Tauri `AppHandle::emit` in the desktop app, SSE frames on `GET /api/events` in `serve`.
- Cross-thread control is Mutex + `AtomicBool` + `Condvar` + SQLite rows; cancellation is a shared `AtomicBool` checked via `check_cancel`/`sleep_cancellable` (100 ms slices).

Data flow — catalog sync:

```
invoke start_sync → App::spawn_job (Busy if slot taken, acquires <db>.job-lock)
  → thread "gamelib-job" opens its own Db
  → SteamClient::page → http::get_json (retry/backoff/Pacer)
  → GameRecord::from_item per item (unparseable → skipped)
  → ONE tx per page: upsert_games + set_meta(SYNC_CURSOR) + commit
  → "sync:progress" per page; exactly one "sync:finished" at the end
```

Query path: `invoke query_games` → `spawn_blocking` → reader `Mutex<Db>` → `db::read::query_games` → `GamePage { total, items }`. Exception: `get_game_media` hits Steam live.

Store matching: `run_store_sync` pages the GOG catalog → `store_products`, title matching → `store_matches`, GamesDB cross-reference (`steam appid ↔ gog id`), then owned libraries.

Downloads/install: `enqueue_download` → queue row, CAS `transition(Queued → Downloading)` → `fetch::fetch` into `<library>/.gamelib/downloads/<id>/*.part` with `Range`/`If-Range` resume → MD5 → rename → install worker identifies the payload by **byte signature** (`install/inspect.rs`), then installer/archive/portable branch into `<library>/<title>`.

Frontend data flow: Rust emits Tauri event → `listen` wrapper in `src/lib/api.ts` → `src/hooks/useSyncEvents.ts` / `useDownloadEvents.ts` (each mounted once, in `App.tsx`) → React Query cache (`setQueryData` for hot fields, `invalidateQueries` for cold) → components re-render. No component calls `listen()` directly.

Event names (constants declared in the emitting core module, mirrored in `src/lib/api.ts`): `sync:progress`, `sync:finished`, `download:progress`, `download:state`, `install:progress`, `install:changed`.

## Key Directories

| Path | Contents |
|---|---|
| `crates/gamelib-core/src/` | Domain: `app.rs` (command surface), `sync.rs`, `new_releases.rs`, `steam/`, `stores/`, `db/`, `downloads/`, `install/`, `links/`, `model.rs`, `record.rs`, `secrets.rs`, `error.rs`, `http.rs` |
| `crates/gamelib-core/tests/` | Integration tests + `common/` fakes + `fixtures/*.json` |
| `crates/gamelib-cli/src/` | `main.rs` (hand-rolled arg parsing, 10 subcommands), `serve.rs` (loopback HTTP + SSE) |
| `src-tauri/src/` | `lib.rs` (plugins, state, `invoke_handler`), `commands.rs`, `login.rs` (incognito GOG login window), `error.rs` (`pub use gamelib_core::ErrorInfo as CmdError`) |
| `src/lib/` | Pure TS: `api.ts` (only Tauri boundary), `types.ts` (mirrors `model.rs`), `queryClient.ts`, `format.ts`, `fold.ts`, `grid.ts`, `toast.ts` |
| `src/hooks/` | `useData.ts` (all React Query hooks), `useFilters.ts` (view/filter state → `BaseQuery`), `useGameWindow.ts`, `useSyncEvents.ts`, `useDownloadEvents.ts` |
| `src/components/` | Shell (`App.tsx` is the only view switcher), grid, dialogs, views; shared atoms in `ui.tsx`/`badges.tsx`/`icons.tsx` |
| `src/i18n/tr.ts` | The single Turkish dictionary + label/error-text functions |
| `src/mocks/` | Dev-only in-browser backend (`install.ts`, `server.ts`, `backend.ts`, `fixture.json`) |
| `docs/screenshots/` | README images (`first-run.jpg` is currently unreferenced) |
| `.github/workflows/build.yml` | The only CI workflow |

## Development Commands

Prereqs: Node.js 22.12+, pnpm 10 (`packageManager: pnpm@10.33.0`), Rust ≥ 1.90 (edition 2024); Linux needs WebKitGTK ≥ 2.40 dev packages, Windows needs C++ Build Tools + WebView2.

```bash
pnpm install                 # add --frozen-lockfile in CI
pnpm tauri dev               # desktop app (runs pnpm dev first, vite on :1420)
pnpm dev                     # frontend only (browser preview; fixture or real data)
pnpm serve                   # gamelib-cli serve on 127.0.0.1:1430 -> real catalog in browser preview
pnpm build                   # tsc && vite build (this is also the TS typecheck gate)
pnpm tauri build             # installers; CI wraps this with --bundles/--target
pnpm test                    # vitest run
pnpm format / format:check   # prettier over src index.html vite.config.ts
pnpm fixture                 # regenerate src/mocks/fixture.json via the CLI

cargo test                   # core + cli only (no GTK/WebKit needed)
cargo test -p gamelib-core --test sync_fake downloads_everything_and_reports_progress
cargo test -p gamelib-core -- --ignored        # live Steam network test (not in CI)
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all              # CI checks with --check

cargo run --release -p gamelib-cli -- query --search "witcher" --limit 5
```

CLI subcommands: `serve`, `sync`, `new-releases`, `stores`, `stats`, `query`, `game`, `media`, `check-link`, `export-fixture`; default DB is `dirs::data_local_dir()/com.gamelib.desktop/gamelib.db` (`%LOCALAPPDATA%\com.gamelib.desktop\gamelib.db` on Windows).

CI (`.github/workflows/build.yml`): `check` (ubuntu) and `check-windows` run format/clippy/tests **only on pushes**; `bundle` jobs run on manual dispatch, `v*` tags, or a push whose last commit message contains `[installer]`. `cargo test` locally skips `src-tauri`, but CI's `cargo test --workspace --locked` includes it (hence the apt WebKit install).

## Code Conventions & Common Patterns

### Rust (core + shells)

- **Errors:** one enum, `gamelib_core::Error` (`error.rs`), propagated with `?`. Never add `anyhow` or a second error type. User-visible failures use `Error::Invalid(&'static str)` / `Error::Failed(&'static str, String)` with a **stable code** that the UI translates (e.g. `url_parse`, `itch_key`, `installer_failed`); never put prose in core or build codes dynamically. `ErrorInfo { kind, message }` is the IPC shape; `serve.rs` maps kinds to HTTP status.
- **Serde/IPC:** structs `#[serde(rename_all = "camelCase")]`, enums `snake_case`. `model.rs` and `src/lib/types.ts` are two halves of one contract (`model.rs` says "keep in sync"). Tauri command names are `snake_case`; argument keys are `camelCase` (`product_id` → `productId`).
- **Enums crossing the DB/string boundary** use their `as_str()`/`parse()` pair; parsers are total and silently fall back, so a wrong string never errors, it behaves wrong.
- **SQLite:** never `INSERT OR REPLACE` into `games` (it fires the delete trigger and desyncs the external-content FTS index) — use `ON CONFLICT(appid) DO UPDATE`. Booleans are INTEGER 0/1. JSON-in-TEXT columns must stay JSON arrays (`json_each` reads them). Migrations are append-only (`MIGRATION_N` + `PRAGMA user_version`); **editing an applied migration is invisible on existing DBs**. No foreign keys by design — deletes are flags (`games.delisted`, `store_products.in_catalog`), and user data (links, matches, installs) must survive catalog refreshes.
- **Positional row decoding is frozen:** `CARD_COLUMNS` + `CARD_WIDTH = 22` (`db/read.rs`) and the `COLUMNS`/`row` pairs in `db/{downloads,installs,links}.rs`. Append columns at the end and bump the width.
- **Duplicated constants that must move together:** `CONFIDENT = 0.85` in `stores/matching.rs`, `CONFIDENT_SQL` in `db/stores.rs`, and the inline rule in `CARD_COLUMNS`/`build_filter`. Match precedence Manual > GamesDB > Title is encoded in SQL — never invert it.
- **Queue state changes** go through the CAS helpers `queue::transition` / `transition_install` / `set_install_state` (`db/downloads.rs`); never `UPDATE downloads SET state = …` directly.
- **HTTP politeness:** all API GETs go through `http::get_text`/`get_json` (retry on transient/429/5xx, `Retry-After`, backoff, counters) and bulk loops use a `Pacer`. The download client must keep `no_gzip()` and its 10-redirect cap (byte offsets); the link-check client keeps `Policy::none()`.
- **Secrets never leave core:** tokens live only in `secrets.bin` via `SecretStore` (DPAPI on Windows, 0600 on unix). Never add token fields to `model.rs` DTOs, never log tokens or signed CDN URLs (`fetch::redact`). GOG refresh stays serialized behind the static `REFRESH` mutex.
- **Path hygiene:** every store/URL/archive name passes `names::safe_name`; archive entries also `archive::entry_path` (zip-slip/NUL/ADS guard); store-declared relative paths through `targets::join_relative`.
- Filenames in `downloads/`, `install/` must run through `safe_name`; installer switch strings (Inno `/VERYSILENT …`, NSIS `/S /D=…`, `msiexec /i`) are fixed ASCII — do not reword. Windows installers run via `ShellExecuteExW` after `windows::mark_downloaded` (Mark of the Web + AV scan); don't replace with plain `std::process`.
- Comments explain *why*, one line, lowercase; several record hard-won constraints. Preserve them.

### Frontend (React 19 + TS)

- **Layout by layer:** pure logic in `src/lib/`, hooks in `src/hooks/useX.ts`, components `PascalCase.tsx` with named exports (`interface Props` above), strings only in `src/i18n/tr.ts`.
- **Data access:** add queries/mutations to `src/hooks/useData.ts` (or `useGameWindow.ts` for the paged grid), never inline in a component. Query keys are lowercase noun arrays (`["status"]`, `["store-files", store, productId]`). Mutations must invalidate every affected key (`useInvalidateLinks` is the named-invalidator precedent). Use `setQueryData` only when a command returns the exact new object (downloads, settings, accounts, installs).
- **`staleTime: Infinity`** in `src/lib/queryClient.ts` means server-side changes are invisible until an explicit invalidate or `setQueryData`.
- **Adding a Tauri command touches three places:** a one-line typed wrapper in `src/lib/api.ts` (snake_case command name), types in `src/lib/types.ts`, and a `case` in `MockBackend.dispatch` (`src/mocks/backend.ts`). Never import `@tauri-apps/api/*` outside `lib/api.ts` (only exception: `isTauri` in `main.tsx`).
- **New events:** declare `EVENT_*` + `on*` in `api.ts`, consume them only in a hook mounted once in `App`, and register the name in `mocks/server.ts`'s `streamEvents`.
- **New views:** add to the `View` union + `VIEWS` in `hooks/useFilters.ts`, a nav entry in `components/Sidebar.tsx`, a branch in `App.tsx`'s `main`, and a heading case in `ViewHeader.tsx`. View-scoped query derivation belongs in `useFilters()`'s `useMemo`.
- **Strings & errors:** no Turkish prose in components; user-facing errors are always `errorText(toCmdError(e))` shown via `showToast`. Destructive actions confirm inline (`SmallButton tone="danger"`), never `window.confirm`.
- **Styling:** Tailwind v4 utilities + design tokens from `src/index.css` (`ink-*`, `accent`, `violet`, `review-*`, `deck-*`, `gog`, `itch`) and its custom `@utility` classes (`glass`, `shimmer`, `card-glow`, `scrollbar-none`). No new CSS files, no inline `style` except dynamic numbers, reuse `ui.tsx`/`badges.tsx` atoms first.
- **Accessibility is expected:** `title` + `aria-label` on icon-only controls, `aria-pressed`/`role="switch"`, `role="progressbar"` with aria values, native `<dialog>` + `showModal()`, toasts in an `aria-live` portal. Nested dialogs need `stopPropagation` on inner close handlers.
- **`lib/fold.ts::normalizeName` must stay identical to `gamelib_core::search::normalize`** (Turkish dotless-i/accents).

## Important Files

| File | Why it matters |
|---|---|
| `crates/gamelib-core/src/app.rs` | `App` = the single command surface + job slot + event constants |
| `crates/gamelib-core/src/model.rs` | Every IPC DTO; paired with `src/lib/types.ts` |
| `crates/gamelib-core/src/error.rs` | Error enum, stable codes, `ErrorInfo` |
| `crates/gamelib-core/src/db/schema.rs` | All DDL as frozen migrations; `SCHEMA_VERSION` |
| `crates/gamelib-core/src/db/read.rs` | Query/filter/order SQL, frozen `CARD_COLUMNS` |
| `crates/gamelib-core/src/stores/mod.rs` | Store-sync orchestrator + `StoreEndpoints` |
| `crates/gamelib-core/src/downloads/fetch.rs` | Resume/checksum streaming transfer |
| `crates/gamelib-core/src/install/inspect.rs` | Byte-signature file-type detection |
| `crates/gamelib-core/src/links/sites/mod.rs` | Documented extension point; `builtin()` is currently empty |
| `src-tauri/src/lib.rs` | Plugin registration, managed state, `invoke_handler` list |
| `src-tauri/tauri.conf.json` | devUrl `:1420`, `frontendDist ../dist`, CSP image allowlist, window |
| `src/lib/api.ts` | Only Tauri boundary: wrappers + event listeners |
| `src/hooks/useFilters.ts` | View union + filter state → `BaseQuery` |
| `src/i18n/tr.ts` | Single string dictionary and error-text mapping |
| `vite.config.ts` | Port 1420 (strict), `/api` proxy to `127.0.0.1:1430` with health bypass |
| `.github/workflows/build.yml` | The build/test/bundle pipeline |

## Runtime/Tooling Preferences

- **pnpm only** (no npm/yarn lockfiles); Node 22.12+; `pnpm-lock.yaml` is `lockfileVersion 9.0` with `autoInstallPeers` — CI enforces `--frozen-lockfile`.
- **Rust ≥ 1.90, edition 2024.** No `rust-toolchain.toml`, no `.cargo/config.toml`; CI pins `RUST_TOOLCHAIN=1.94.1`. Workspace deps are declared once in the root `Cargo.toml` and inherited via `.workspace = true`.
- **No ESLint / no TS lint step.** The TS gate is `tsc` inside `pnpm build` (strict, `noUnusedLocals`/`noUnusedParameters` → dead code fails the build). Prettier (`printWidth: 140`) covers only `src index.html vite.config.ts`.
- Tailwind CSS v4 via `@tailwindcss/vite`; no `tailwind.config.*` — all tokens live in `src/index.css` `@theme`.
- Vite dev server pins port 1420 and ignores `src-tauri/`, `crates/`, `target/`; env prefixes `VITE_`, `TAURI_ENV_`.
- Windows-first behaviors are real and load-bearing: DPAPI secrets, `ShellExecuteExW`/UAC, Mark-of-the-Web, registry read for GOG installs (`install/windows.rs` is entirely `#[cfg(windows)]`).
- Network-touching features need no API keys (keyless Steam store endpoints); corporate proxies work through `HTTPS_PROXY`.
- Archives: zip/7z/tar(.gz/.bz2/.xz) are extractable; RAR is deliberately not.

## Testing & QA

- **Rust:** `cargo test` at the root runs `gamelib-core` + `gamelib-cli` (hermetic, no GTK). Integration tests live in `crates/gamelib-core/tests/*.rs`; inline unit tests are `#[cfg(test)]` modules inside `src/` (25 files in core, 2 in cli).
- **Two fake harnesses, no HTTP mocking crates:** `tests/common/mod.rs` provides `FakeSource` (in-memory `CatalogSource` with per-request `hook`/`fail_at`/`cancel_at`) and `tests/common/http.rs` provides `TestServer` (real sockets on `127.0.0.1:0`, supports paced/dropped transfers). `tests/links.rs` rolls its own tiny server. DB tests use `Db::open_in_memory()`; temp dirs are per-test and removed on `Drop`.
- **Hermetic by default:** every test except `tests/live.rs` must pass offline — a network failure anywhere else is a real bug. `live.rs` is `#[ignore]`d and hits `api.steampowered.com` (`cargo test -p gamelib-core -- --ignored`); it is intentionally not in CI. Download-transfer tests are Windows-only except one `#[cfg(not(windows))]` case.
- **Frontend:** Vitest (`pnpm test` = `vitest run`) with no config file, default Node environment, globals off, DOM tests absent. The single suite `src/lib/lib.test.ts` covers `lib/format.ts`, `lib/fold.ts`, `lib/grid.ts` and `i18n/tr.ts` with hard-coded Turkish expectations. Put new logic that needs a test in `src/lib/`.
- **What CI gates:** `pnpm format:check`, `pnpm test`, `pnpm build` (typecheck), `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, then the platform test matrix.
- When changing behavior, exercise the real path (`cargo test -p gamelib-core --test <file>` or `pnpm test` + a `pnpm tauri dev`/`pnpm serve` smoke), not just a compile.
