// Listens to the download queue's events from Rust and keeps the download list current.

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, onDownloadProgress, onDownloadState, toCmdError } from "../lib/api";
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

    keep(
      onDownloadProgress((progress) => {
        if (!known()) return void qc.invalidateQueries({ queryKey: ["downloads"] });
        qc.setQueryData<DownloadList>(["downloads"], (list) =>
          list
            ? {
                live: progress,
                items: list.items.map((d) =>
                  d.id === progress.id
                    ? { ...d, doneBytes: progress.doneBytes, totalBytes: Math.max(d.totalBytes, progress.totalBytes) }
                    : d,
                ),
              }
            : list,
        );
      }),
    );

    keep(
      onDownloadState((payload) => {
        if (!known()) return void qc.invalidateQueries({ queryKey: ["downloads"] });
        if ("removed" in payload) {
          qc.setQueryData<DownloadList>(["downloads"], (list) =>
            list ? { items: list.items.filter((d) => d.id !== payload.id), live: list.live?.id === payload.id ? null : list.live } : list,
          );
          return;
        }
        const previous = qc.getQueryData<DownloadList>(["downloads"])?.items.find((d) => d.id === payload.id);
        qc.setQueryData<DownloadList>(["downloads"], (list) => {
          const next = upsertDownload(list, payload);
          return payload.state === "downloading" || next.live?.id !== payload.id ? next : { ...next, live: null };
        });
        if (previous?.state !== payload.state) announce(payload);
      }),
    );

    return () => {
      disposed = true;
      unlisten.forEach((fn) => fn());
    };
  }, [qc]);
}

function announce(d: Download) {
  if (d.state === "completed") {
    showToast({
      tone: "success",
      title: tr.downloads.toastDone(d.title),
      action: {
        label: tr.downloads.openFolder,
        onClick: () => void api.openDownloadFolder(d.id).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) })),
      },
    });
  } else if (d.state === "failed") {
    showToast({ tone: "error", title: tr.downloads.toastFailed(d.title), description: d.error ? errorText(d.error) : undefined }, 9000);
  }
}
