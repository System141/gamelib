// Sync banner, toasts, empty state and the error boundary.

import clsx from "clsx";
import { CircleCheck, CircleX, Info, LoaderCircle, SearchX, TriangleAlert, X } from "lucide-react";
import { Component, type ErrorInfo, type ReactNode, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { tr } from "../i18n/tr";
import { nowSeconds } from "../lib/format";
import { dismissToast, type ToastTone, useToasts } from "../lib/toast";
import type { AppStatus } from "../lib/types";
import type { View } from "../hooks/useFilters";
import { StoreMark } from "./badges";

const STALE_AFTER_SECONDS = 7 * 86_400;

/** Thin bar under the top bar: download progress, an interrupted or never-run full download, or a stale catalog. */
export function SyncBanner({
  status,
  onResume,
  onRefresh,
}: {
  status: AppStatus | undefined;
  onResume: () => void;
  onRefresh: () => void;
}) {
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!status) return null;

  if (status.worker === "full" || status.worker === "stores" || status.worker === "library") {
    const p = status.progress;
    const pct = p && p.total > 0 ? Math.min(100, (p.fetched / p.total) * 100) : null;
    const counted = status.worker === "full" ? tr.firstRun.progress : tr.sync.storesProgress;
    return (
      <div className="relative shrink-0 border-b border-white/6 bg-ink-850/80">
        <div className="flex h-9 items-center gap-3 px-8 text-[13px] text-ink-300">
          <LoaderCircle size={14} className={clsx("animate-spin", status.worker === "full" ? "text-accent" : "text-gog")} />
          <span className="text-ink-100">{tr.sync.phases[p?.phase ?? "starting"]}</span>
          {p && p.total > 0 && <span className="tabular-nums">{counted(p.fetched, p.total)}</span>}
        </div>
        <div className="absolute inset-x-0 bottom-0 h-0.5 bg-ink-700">
          {pct != null && (
            <div className="h-full bg-gradient-to-r from-accent to-violet transition-[width] duration-500" style={{ width: `${pct}%` }} />
          )}
        </div>
      </div>
    );
  }
  if (status.worker) return null;

  let kind: string | null = null;
  let text = "";
  let action = "";
  let onAction = onRefresh;
  if (status.resumable) {
    kind = "resume";
    text = tr.sync.resumable;
    action = tr.sync.resume;
    onAction = onResume;
  } else if (!status.lastSyncAt && status.gameCount > 0) {
    // Only new releases were fetched so far.
    kind = "partial";
    text = tr.sync.notFull(status.gameCount);
    action = tr.sync.downloadAll;
  } else if (status.lastSyncAt && nowSeconds() - status.lastSyncAt > STALE_AFTER_SECONDS) {
    kind = "stale";
    text = tr.sync.stale(status.lastSyncAt);
    action = tr.sync.refresh;
  }
  if (!kind || dismissed === kind) return null;

  return (
    <div className="flex h-10 shrink-0 items-center gap-3 border-b border-warning/15 bg-warning/6 px-8 text-[13px] text-warning">
      <TriangleAlert size={14} />
      <span className="text-ink-100">{text}</span>
      <button type="button" onClick={onAction} className="font-semibold text-warning underline-offset-2 hover:underline">
        {action}
      </button>
      <button
        type="button"
        onClick={() => setDismissed(kind)}
        className="ml-auto grid size-6 place-items-center rounded-md text-ink-400 hover:bg-white/8 hover:text-white"
        aria-label={tr.filters.close}
      >
        <X size={13} />
      </button>
    </div>
  );
}

const TOAST_ICON: Record<ToastTone, ReactNode> = {
  success: <CircleCheck size={18} className="text-success" />,
  info: <Info size={18} className="text-accent" />,
  warning: <TriangleAlert size={18} className="text-warning" />,
  error: <CircleX size={18} className="text-danger" />,
};

/** The open modal dialog, if any: everything outside it is inert and drawn below it, so toasts
 *  shown while it is open have to live inside it. */
export function useModalHost(): HTMLElement {
  // The last open dialog in document order is the innermost (a picker inside the game details).
  const find = () => [...document.querySelectorAll<HTMLElement>("dialog[open]")].pop() ?? document.body;
  const [host, setHost] = useState<HTMLElement>(find);
  useEffect(() => {
    const observer = new MutationObserver(() => setHost(find()));
    observer.observe(document.body, { subtree: true, attributes: true, attributeFilter: ["open"] });
    return () => observer.disconnect();
  }, []);
  return host;
}

export function Toasts() {
  const toasts = useToasts();
  const host = useModalHost();
  return createPortal(
    <div className="pointer-events-none fixed right-6 bottom-6 z-[60] flex w-96 max-w-[90vw] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => (
        <div
          key={t.id}
          className="animate-rise pointer-events-auto flex items-start gap-3 rounded-xl bg-ink-750/95 p-4 shadow-2xl shadow-black/60 ring-1 ring-white/10 backdrop-blur"
        >
          <span className="mt-0.5">{TOAST_ICON[t.tone]}</span>
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium text-ink-50">{t.title}</div>
            {t.description && <div className="mt-0.5 text-[13px] text-ink-300">{t.description}</div>}
            {t.action && (
              <button
                type="button"
                onClick={() => {
                  t.action!.onClick();
                  dismissToast(t.id);
                }}
                className="mt-2 text-[13px] font-semibold text-accent-soft underline-offset-2 hover:underline"
              >
                {t.action.label}
              </button>
            )}
          </div>
          <button
            type="button"
            onClick={() => dismissToast(t.id)}
            className="grid size-6 place-items-center rounded-md text-ink-400 hover:bg-white/8 hover:text-white"
            aria-label={tr.filters.close}
          >
            <X size={14} />
          </button>
        </div>
      ))}
    </div>,
    host,
  );
}

export function EmptyState({
  view,
  canClear,
  onClear,
  storesSynced,
  onStoreSync,
}: {
  view: View;
  canClear: boolean;
  onClear: () => void;
  storesSynced: boolean;
  onStoreSync: () => void;
}) {
  let title = tr.empty.title;
  let text = tr.empty.text;
  let icon: ReactNode = <SearchX size={26} className="text-ink-400" />;
  let action: { label: string; onClick: () => void } | null = canClear ? { label: tr.empty.clear, onClick: onClear } : null;
  if (!canClear) {
    if (view === "links") {
      title = tr.linksView.emptyTitle;
      text = tr.linksView.emptyText;
    } else if (view === "gog" && !storesSynced) {
      title = tr.stores.gogView.neverTitle;
      text = tr.stores.gogView.neverText;
      icon = <StoreMark store="gog" size={30} />;
      action = { label: tr.stores.syncCta, onClick: onStoreSync };
    } else if (view === "itch") {
      title = tr.stores.itchView.emptyTitle;
      text = tr.stores.itchView.emptyText;
      icon = <StoreMark store="itch" size={30} />;
    }
  }
  return (
    <div className="animate-fade-in max-w-md text-center">
      <div className="mx-auto grid size-16 place-items-center rounded-2xl bg-white/4 ring-1 ring-white/8">{icon}</div>
      <h2 className="mt-5 font-display text-xl font-semibold text-ink-50">{title}</h2>
      <p className="mt-2 text-sm text-ink-400">{text}</p>
      {action && (
        <button
          type="button"
          onClick={action.onClick}
          className={clsx("mt-5 h-9 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 hover:bg-white/10")}
        >
          {action.label}
        </button>
      )}
    </div>
  );
}

export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("UI error", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="app-backdrop grid h-full place-items-center p-8 text-center">
        <div>
          <h1 className="font-display text-2xl font-semibold text-ink-50">{tr.error.title}</h1>
          <p className="mt-2 font-mono text-sm text-ink-400">{this.state.error.message}</p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="mt-6 h-10 rounded-lg bg-accent px-5 text-sm font-semibold text-ink-950"
          >
            {tr.error.reload}
          </button>
        </div>
      </div>
    );
  }
}
