// App updates: an automatic check soon after start and every few hours (release builds, when
// enabled in the settings), a manual check, and installing with progress.

import { useEffect, useSyncExternalStore } from "react";
import { type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, onUpdateProgress, toCmdError } from "../lib/api";
import { showToast } from "../lib/toast";
import type { UpdateProgress, UpdateStatus } from "../lib/types";
import { errorText } from "../i18n/tr";

const FIRST_CHECK_MS = 10_000;
const CHECK_EVERY_MS = 6 * 3_600_000;

/** Automatic checks run in release builds; `?mock=update` shows them in the browser preview. */
const AUTO_CHECKS = import.meta.env.PROD || new URLSearchParams(window.location.search).get("mock") === "update";

/** The version and what the last check found (checks update it). */
export function useUpdateStatus() {
  return useQuery({ queryKey: ["update"], queryFn: api.getUpdateStatus, staleTime: Infinity });
}

async function check(qc: QueryClient): Promise<UpdateStatus> {
  const status = await api.checkUpdate();
  await qc.cancelQueries({ queryKey: ["update"] });
  qc.setQueryData(["update"], status);
  return status;
}

export function useCheckUpdate() {
  const qc = useQueryClient();
  return () => check(qc);
}

/** Checks now and then while `enabled` (the "auto update" setting). Failures stay quiet. */
export function useAutoUpdateCheck(enabled: boolean) {
  const qc = useQueryClient();
  useEffect(() => {
    if (!enabled || !AUTO_CHECKS) return;
    const run = () => void check(qc).catch(() => undefined);
    const first = window.setTimeout(run, FIRST_CHECK_MS);
    const every = window.setInterval(run, CHECK_EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, [enabled, qc]);
}

/** One install at a time, shared by the banner and the settings. */
interface InstallState {
  installing: boolean;
  progress: UpdateProgress | null;
}

let installState: InstallState = { installing: false, progress: null };
const listeners = new Set<() => void>();

function setInstallState(next: InstallState) {
  installState = next;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** Installs the update found by the last check; the app restarts when it is done. */
function installUpdate() {
  if (installState.installing) return;
  setInstallState({ installing: true, progress: null });
  let unlisten: (() => void) | null = null;
  void onUpdateProgress((progress) => setInstallState({ installing: true, progress })).then((fn) => (unlisten = fn));
  api.installUpdate().catch((e) => {
    unlisten?.();
    setInstallState({ installing: false, progress: null });
    showToast({ tone: "error", title: errorText(toCmdError(e)) });
  });
}

export function useInstallUpdate() {
  const state = useSyncExternalStore(subscribe, () => installState);
  return { install: installUpdate, installing: state.installing, progress: state.progress };
}
