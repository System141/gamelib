// Dev-only: lets the UI run in a plain browser (`pnpm dev`) without the Tauri window.
// Loaded through a dynamic import guarded by `import.meta.env.DEV`, so it never ships.
//
// With `gamelib-cli serve` running, commands go to the real catalog over HTTP (./server.ts).
// Otherwise they are answered from fixture data (./backend.ts). `?mock` forces the fixture and
// `?mock=empty` starts it with an empty catalog.

import { mockIPC } from "@tauri-apps/api/mocks";
import { tr } from "../i18n/tr";
import type { Fixture } from "./backend";
import { connectServer } from "./server";

export type Emit = (event: string, payload: unknown) => void;
type Handler = (cmd: string, args: Record<string, any>) => Promise<unknown>;
type Internals = { runCallback: (id: number, data: unknown) => void };

export async function installMocks(params: URLSearchParams): Promise<void> {
  const bus = createEventBus();
  const server = params.has("mock") ? null : await connectServer(bus.emit);

  let handler: Handler;
  if (server) {
    handler = server.handle;
    showBadge("server", tr.preview.server, tr.preview.serverHint(server.info.dbPath));
  } else {
    const [{ MockBackend }, fixture] = await Promise.all([import("./backend"), import("./fixture.json")]);
    const data = fixture.default as unknown as Fixture;
    const backend = new MockBackend(data, bus.emit, params.get("mock") === "empty");
    handler = (cmd, args) => backend.handle(cmd, args);
    showBadge("fixture", tr.preview.fixture, tr.preview.fixtureHint);
  }

  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, any>;
    return cmd.startsWith("plugin:event|") ? bus.handle(cmd, a) : handler(cmd, a);
  });
}

/** Minimal event bus matching the `plugin:event|*` commands used by @tauri-apps/api/event. */
function createEventBus() {
  const listeners = new Map<string, Set<number>>();
  const emit: Emit = (event, payload) => {
    const internals = (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
    // A copy for each listener, as Tauri's serialized events give.
    for (const id of listeners.get(event) ?? []) internals.runCallback(id, { event, id, payload: structuredClone(payload) });
  };
  const handle = (cmd: string, a: Record<string, any>): unknown => {
    switch (cmd) {
      case "plugin:event|listen": {
        const set = listeners.get(a.event) ?? new Set<number>();
        set.add(a.handler);
        listeners.set(a.event, set);
        return a.handler;
      }
      case "plugin:event|unlisten":
        listeners.get(a.event)?.delete(a.eventId);
        return null;
      case "plugin:event|emit":
        emit(a.event, a.payload);
        return null;
      default:
        return null;
    }
  };
  return { emit, handle };
}

/** Small corner note saying where the preview's data comes from, right of the sidebar so it
 *  never covers navigation. Plain DOM: it is not part of the app. */
function showBadge(source: "server" | "fixture", title: string, hint: string) {
  const badge = document.createElement("div");
  badge.dataset.previewBadge = source;
  badge.setAttribute("role", "status");
  badge.className =
    "animate-fade-in fixed bottom-4 left-20 z-[55] flex max-w-sm items-start gap-2.5 rounded-xl bg-ink-800/95 py-2.5 pr-2 pl-3 text-[12px] leading-snug shadow-xl shadow-black/50 ring-1 ring-white/10 backdrop-blur xl:left-64";

  const dot = document.createElement("span");
  dot.className = `mt-1 size-2 shrink-0 rounded-full ${source === "server" ? "bg-success" : "bg-warning"}`;

  const text = document.createElement("div");
  text.className = "min-w-0 flex-1";
  const strong = document.createElement("div");
  strong.className = "font-medium text-ink-100";
  strong.textContent = title;
  const small = document.createElement("div");
  small.className = "mt-0.5 break-words text-ink-400";
  small.textContent = hint;
  text.append(strong, small);

  const close = document.createElement("button");
  close.type = "button";
  close.className =
    "grid size-5 shrink-0 place-items-center rounded-md text-[15px] leading-none text-ink-400 hover:bg-white/8 hover:text-white";
  close.setAttribute("aria-label", tr.filters.close);
  close.textContent = "×";
  close.addEventListener("click", () => badge.remove());

  badge.append(dot, text, close);
  document.body.append(badge);
}
