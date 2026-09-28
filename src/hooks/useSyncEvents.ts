// Listens to catalog job events from Rust, keeps the cache fresh and reports the outcome.

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onSyncFinished, onSyncProgress } from "../lib/api";
import { showToast } from "../lib/toast";
import type { AppStatus, SyncFinished } from "../lib/types";
import { errorText, tr } from "../i18n/tr";

/** How often visible pages are refreshed while a download adds games. */
const REFRESH_EVERY_MS = 4000;

export function useSyncEvents() {
  const qc = useQueryClient();

  useEffect(() => {
    let disposed = false;
    const unlisten: UnlistenFn[] = [];
    let lastRefresh = 0;

    const keep = (p: Promise<UnlistenFn>) =>
      p.then((fn) => (disposed ? fn() : unlisten.push(fn))).catch(() => undefined);

    keep(
      onSyncProgress((progress) => {
        qc.setQueryData<AppStatus>(["status"], (s) => (s ? { ...s, worker: progress.kind, progress } : s));
        const now = Date.now();
        if (now - lastRefresh > REFRESH_EVERY_MS && progress.fetched > 0) {
          lastRefresh = now;
          void qc.invalidateQueries({ queryKey: ["games"] });
          void qc.invalidateQueries({ queryKey: ["status"] });
          void qc.invalidateQueries({ queryKey: ["tags"] });
        }
      }),
    );

    keep(
      onSyncFinished((finished) => {
        qc.setQueryData<AppStatus>(["status"], (s) => (s ? { ...s, worker: null, progress: null } : s));
        for (const key of ["games", "status", "tags", "game"]) {
          void qc.invalidateQueries({ queryKey: [key] });
        }
        announce(finished);
      }),
    );

    return () => {
      disposed = true;
      unlisten.forEach((fn) => fn());
    };
  }, [qc]);
}

function announce(f: SyncFinished) {
  if (f.outcome === "cancelled") {
    showToast({ tone: "info", title: tr.sync.toastCancelled, description: f.kind === "full" ? tr.sync.toastCancelledDetail : undefined });
    return;
  }
  if (f.outcome === "failed") {
    showToast({ tone: "error", title: tr.sync.toastFailed, description: f.error ? errorText(f.error) : undefined }, 8000);
    return;
  }
  if (f.report) {
    showToast({
      tone: "success",
      title: tr.sync.toastFull(f.report.seen),
      description: f.report.inserted > 0 ? tr.sync.toastFullNew(f.report.inserted) : undefined,
    });
  }
  if (f.newReleases) {
    const r = f.newReleases;
    showToast({ tone: "success", title: tr.sync.toastNew(r.inserted), description: tr.sync.toastNewDetail(r.fetched) });
    if (r.partial) {
      showToast({ tone: "warning", title: tr.sync.toastPartial }, 9000);
    }
  }
}
