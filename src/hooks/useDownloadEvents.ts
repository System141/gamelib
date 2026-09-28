// Listens to the download queue's and installer's events from Rust and keeps the download list
// and installed games current.

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, onDownloadProgress, onDownloadState, onInstallChanged, onInstallProgress, toCmdError } from "../lib/api";
import { showToast } from "../lib/toast";
import type { Download, DownloadList } from "../lib/types";
import { errorText, tr } from "../i18n/tr";
import { upsertDownload } from "./useData";

export function useDownloadEvents() {
  const qc = useQueryClient();

  useEffect(() => {
    let disposed = false;
    const unlisten: UnlistenFn[] = [];
    const keep = (p: Promise<UnlistenFn>) => p.then((fn) => (disposed ? fn() : unlisten.push(fn))).catch(() => undefined);
    const known = () => qc.getQueryData<DownloadList>(["downloads"]) != null;
    const update = (fn: (list: DownloadList) => DownloadList) =>
      qc.setQueryData<DownloadList>(["downloads"], (list) => (list ? fn(list) : list));

    keep(
      onDownloadProgress((progress) => {
        if (!known()) return void qc.invalidateQueries({ queryKey: ["downloads"] });
        update((list) => ({
          ...list,
          live: progress,
          items: list.items.map((d) =>
            d.id === progress.id ? { ...d, doneBytes: progress.doneBytes, totalBytes: Math.max(d.totalBytes, progress.totalBytes) } : d,
          ),
        }));
      }),
    );

    keep(
      onDownloadState((payload) => {
        if (!known()) return void qc.invalidateQueries({ queryKey: ["downloads"] });
        if ("removed" in payload) {
          update((list) => ({
            ...list,
            items: list.items.filter((d) => d.id !== payload.id),
            live: list.live?.id === payload.id ? null : list.live,
          }));
          return;
        }
        const previous = qc.getQueryData<DownloadList>(["downloads"])?.items.find((d) => d.id === payload.id);
        qc.setQueryData<DownloadList>(["downloads"], (list) => {
          const next = upsertDownload(list, payload);
          const live = payload.state === "downloading" || next.live?.id !== payload.id ? next.live : null;
          const installing = payload.installState === "installing" || next.installing?.downloadId !== payload.id ? next.installing : null;
          return { ...next, live, installing };
        });
        if (previous?.state !== payload.state) announceDownload(payload);
        if (previous?.installState !== payload.installState) announceInstall(payload);
      }),
    );

    keep(
      onInstallProgress((progress) => {
        if (!known()) return void qc.invalidateQueries({ queryKey: ["downloads"] });
        update((list) => ({ ...list, installing: progress }));
      }),
    );

    keep(
      onInstallChanged(() => {
        void qc.invalidateQueries({ queryKey: ["installs"] });
        void qc.invalidateQueries({ queryKey: ["downloads"] });
      }),
    );

    return () => {
      disposed = true;
      unlisten.forEach((fn) => fn());
    };
  }, [qc]);
}

const toastError = (e: unknown) => showToast({ tone: "error", title: errorText(toCmdError(e)) });

function announceDownload(d: Download) {
  // A finished download is announced by its install.
  if (d.state === "failed") {
    showToast({ tone: "error", title: tr.downloads.toastFailed(d.title), description: d.error ? errorText(d.error) : undefined }, 9000);
  }
}

function announceInstall(d: Download) {
  switch (d.installState) {
    case "installed":
      showToast({
        tone: "success",
        title: tr.install.toastInstalled(d.title),
        action: { label: tr.install.play, onClick: () => void api.launchGame(d.store, d.productId).catch(toastError) },
      });
      break;
    case "failed":
      showToast(
        { tone: "error", title: tr.install.toastFailed(d.title), description: d.installError ? errorText(d.installError) : undefined },
        9000,
      );
      break;
    case "confirm":
      showToast({ tone: "info", title: tr.install.toastConfirm(d.title), description: tr.install.toastConfirmDetail }, 9000);
      break;
    case "manual":
      showToast(
        {
          tone: "info",
          title: tr.install.toastManual(d.title),
          description: tr.install.manualHint(d.installKind),
          action: { label: tr.downloads.openFolder, onClick: () => void api.openDownloadFolder(d.id).catch(toastError) },
        },
        9000,
      );
      break;
  }
}
