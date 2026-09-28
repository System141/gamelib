// Dev-only: lets the UI run in a plain browser (`pnpm dev`) without the Tauri backend.
// Loaded through a dynamic import guarded by `import.meta.env.DEV`, so it never ships.

import { mockIPC } from "@tauri-apps/api/mocks";
import { MockBackend, type Fixture } from "./backend";

type Internals = { runCallback: (id: number, data: unknown) => void };

export async function installMocks(params: URLSearchParams): Promise<void> {
  const fixture = (await import("./fixture.json")).default as unknown as Fixture;

  // Minimal event bus matching the `plugin:event|*` commands used by @tauri-apps/api/event.
  const listeners = new Map<string, Set<number>>();
  const emit = (event: string, payload: unknown) => {
    const internals = (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
    for (const id of listeners.get(event) ?? []) internals.runCallback(id, { event, id, payload });
  };

  const backend = new MockBackend(fixture, emit, params.get("mock") === "empty");

  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, any>;
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
        return backend.handle(cmd, a);
    }
  });
}
