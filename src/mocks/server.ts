// Dev-only bridge to `gamelib-cli serve` (crates/gamelib-cli/src/serve.rs), reached through the
// `/api` proxy in vite.config.ts. Every `invoke` becomes `POST /api/<command>` with the same JSON
// arguments; catalog job events arrive as server-sent events from `/api/events`.

import { EVENT_FINISHED, EVENT_PROGRESS } from "../lib/api";
import type { CmdError } from "../lib/types";
import type { Emit } from "./install";

export interface ServerInfo {
  version: string;
  dbPath: string;
}

export interface ServerBridge {
  info: ServerInfo;
  handle: (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
}

/** Connects when the local server answers its health check; otherwise returns null. */
export async function connectServer(emit: Emit): Promise<ServerBridge | null> {
  let info: ServerInfo;
  try {
    const res = await fetch("/api/health", { cache: "no-store" });
    const body = res.ok ? await res.json() : null;
    if (!body?.ok) return null;
    info = body as ServerInfo;
  } catch {
    return null;
  }
  streamEvents(emit);
  return { info, handle: call };
}

async function call(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  let res: Response;
  try {
    res = await fetch(`/api/${cmd}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args),
    });
  } catch (e) {
    throw { kind: "other", message: `gamelib-cli serve unreachable: ${e}` } satisfies CmdError;
  }
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    throw body && typeof body === "object" && "kind" in body ? body : ({ kind: "other", message: `HTTP ${res.status}` } satisfies CmdError);
  }
  // The desktop app opens these itself; in the browser the page does.
  if (cmd === "open_link" || cmd === "open_in_steam") {
    openUrl((body as { url: string }).url);
    return null;
  }
  return body;
}

function openUrl(url: string) {
  if (/^https?:\/\//i.test(url)) {
    window.open(url, "_blank", "noopener,noreferrer");
  } else {
    // steam:// hands the page over to the Steam client; the browser stays on this page.
    window.location.assign(url);
  }
}

/** Relays job events. EventSource retries dropped connections itself but gives up on an error
 *  response (the dev proxy answers 404 while the server is stopped), so reconnect with backoff. */
function streamEvents(emit: Emit) {
  let delay = 1000;
  const connect = () => {
    const source = new EventSource("/api/events");
    for (const name of [EVENT_PROGRESS, EVENT_FINISHED]) {
      source.addEventListener(name, (e) => emit(name, JSON.parse((e as MessageEvent<string>).data)));
    }
    source.onopen = () => {
      delay = 1000;
    };
    source.onerror = () => {
      if (source.readyState === EventSource.CLOSED) {
        setTimeout(connect, delay);
        delay = Math.min(delay * 2, 30_000);
      }
    };
  };
  connect();
}
