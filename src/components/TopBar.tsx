import clsx from "clsx";
import { ArrowUpDown, Check, ChevronDown, Database, LoaderCircle, RefreshCw, Search, SlidersHorizontal, Sparkles, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { tr } from "../i18n/tr";
import { formatNumber } from "../lib/format";
import type { AppStatus, SortKey } from "../lib/types";
import type { FiltersState, View } from "../hooks/useFilters";
import { useDismiss } from "../hooks/useUtils";
import { Logo } from "./icons";

interface Props {
  f: FiltersState;
  status: AppStatus | undefined;
  /** First run: nothing to browse yet, so only the logo and the update button show. */
  catalogEmpty: boolean;
  onOpenFilters: () => void;
  onFullSync: () => void;
  onNewReleases: () => void;
  onCancelSync: () => void;
}

export function TopBar({ f, status, catalogEmpty, onOpenFilters, onFullSync, onNewReleases, onCancelSync }: Props) {
  return (
    <header className="glass relative z-30 flex h-16 shrink-0 items-center gap-4 border-b border-white/6 px-5">
      <div className="flex shrink-0 items-center gap-2.5 pr-1">
        <Logo size={30} />
        <span className="hidden font-display text-lg font-semibold tracking-tight text-ink-50 xl:inline">
          Game<span className="text-gradient">Lib</span>
        </span>
      </div>

      {catalogEmpty ? (
        <div className="flex-1" />
      ) : (
        <>
          <ViewTabs view={f.view} onChange={f.setView} linked={status?.linkedGameCount ?? 0} />
          <div className="flex min-w-[240px] flex-1 justify-center">
            <SearchBox value={f.search} onChange={f.setSearch} />
          </div>
        </>
      )}

      <div className="flex shrink-0 items-center gap-2">
        {!catalogEmpty && (
          <>
            <SortMenu value={f.sort} searching={f.searching} onChange={f.setSort} />
            <button
              type="button"
              onClick={onOpenFilters}
              title={tr.filters.button}
              className={clsx(
                "inline-flex h-9 items-center gap-2 rounded-lg px-3 text-sm font-medium ring-1 transition",
                f.activeCount > 0
                  ? "bg-accent/12 text-accent-soft ring-accent/35 hover:bg-accent/18"
                  : "bg-white/4 text-ink-200 ring-white/8 hover:bg-white/8 hover:text-ink-50",
              )}
            >
              <SlidersHorizontal size={15} />
              <span className="hidden xl:inline">{tr.filters.button}</span>
              {f.activeCount > 0 && (
                <span className="grid h-5 min-w-5 place-items-center rounded-full bg-accent px-1 text-[11px] font-bold text-ink-950">
                  {f.activeCount}
                </span>
              )}
            </button>
          </>
        )}
        <SyncButton status={status} onFullSync={onFullSync} onNewReleases={onNewReleases} onCancel={onCancelSync} />
      </div>
    </header>
  );
}

function ViewTabs({ view, onChange, linked }: { view: View; onChange: (v: View) => void; linked: number }) {
  const tabs: { id: View; label: string; badge?: number }[] = [
    { id: "all", label: tr.views.all },
    { id: "new", label: tr.views.new },
    { id: "links", label: tr.views.links, badge: linked || undefined },
  ];
  return (
    <nav className="flex shrink-0 items-center gap-1 rounded-xl bg-white/4 p-1 ring-1 ring-white/6" aria-label="Görünüm">
      {tabs.map((t) => (
        <button
          key={t.id}
          type="button"
          onClick={() => onChange(t.id)}
          aria-current={view === t.id ? "page" : undefined}
          className={clsx(
            "relative inline-flex h-8 items-center gap-1.5 rounded-lg px-3 text-[13px] font-medium whitespace-nowrap transition",
            view === t.id ? "bg-ink-700 text-white shadow-sm shadow-black/40 ring-1 ring-white/10" : "text-ink-300 hover:text-ink-50",
          )}
        >
          {t.id === "new" && <Sparkles size={13} className={view === t.id ? "text-violet" : undefined} />}
          {t.label}
          {t.badge != null && (
            <span className="rounded-full bg-accent/20 px-1.5 text-[11px] font-semibold text-accent-soft">{formatNumber(t.badge)}</span>
          )}
        </button>
      ))}
    </nav>
  );
}

function SearchBox({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing =
        e.target instanceof HTMLElement && (e.target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(e.target.tagName));
      if (((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") || (e.key === "/" && !typing)) {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <label className="group relative flex h-10 w-full max-w-xl items-center rounded-xl bg-ink-850/80 ring-1 ring-white/8 transition focus-within:bg-ink-800 focus-within:ring-accent/50 hover:ring-white/15">
      <Search size={16} className="pointer-events-none absolute left-3.5 text-ink-400 transition group-focus-within:text-accent" />
      <input
        ref={input}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.preventDefault();
            onChange("");
          }
        }}
        placeholder={tr.search.placeholder}
        spellCheck={false}
        className="h-full w-full rounded-xl bg-transparent pr-20 pl-10 text-sm text-ink-50 outline-none placeholder:text-ink-400"
        aria-label={tr.search.placeholder}
      />
      {value ? (
        <button
          type="button"
          onClick={() => onChange("")}
          className="absolute right-2 grid size-7 place-items-center rounded-lg text-ink-400 hover:bg-white/8 hover:text-ink-50"
          aria-label={tr.search.clear}
        >
          <X size={15} />
        </button>
      ) : (
        <kbd className="pointer-events-none absolute right-3 rounded-md border border-white/10 bg-white/5 px-1.5 py-0.5 font-sans text-[11px] text-ink-400">
          Ctrl K
        </kbd>
      )}
    </label>
  );
}

function SortMenu({ value, searching, onChange }: { value: SortKey; searching: boolean; onChange: (s: SortKey) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(ref, open, close);
  const options: SortKey[] = [...(searching ? (["relevance"] as SortKey[]) : []), "popular", "rating", "newest", "oldest", "name"];

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={`${tr.sort.label}: ${tr.sort[value]}`}
        className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/4 px-3 text-sm font-medium text-ink-200 ring-1 ring-white/8 transition hover:bg-white/8 hover:text-ink-50"
      >
        <ArrowUpDown size={15} className="text-ink-400" />
        <span className="hidden whitespace-nowrap xl:inline">{tr.sort[value]}</span>
        <ChevronDown size={14} className={clsx("text-ink-400 transition", open && "rotate-180")} />
      </button>
      {open && (
        <ul
          role="listbox"
          className="animate-rise absolute right-0 z-40 mt-2 w-52 overflow-hidden rounded-xl bg-ink-800 p-1 shadow-2xl shadow-black/60 ring-1 ring-white/10"
        >
          <li className="px-3 pt-2 pb-1 text-[11px] font-semibold tracking-wider text-ink-400 uppercase">{tr.sort.label}</li>
          {options.map((o) => (
            <li key={o}>
              <button
                type="button"
                role="option"
                aria-selected={o === value}
                onClick={() => {
                  onChange(o);
                  setOpen(false);
                }}
                className={clsx(
                  "flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition",
                  o === value ? "bg-accent/12 text-accent-soft" : "text-ink-100 hover:bg-white/6",
                )}
              >
                {tr.sort[o]}
                {o === value && <Check size={15} />}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function SyncButton({
  status,
  onFullSync,
  onNewReleases,
  onCancel,
}: {
  status: AppStatus | undefined;
  onFullSync: () => void;
  onNewReleases: () => void;
  onCancel: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(ref, open, close);
  const progress = status?.progress;

  if (status?.worker) {
    const pct = progress && progress.total > 0 ? Math.round((progress.fetched / progress.total) * 100) : null;
    return (
      <div className="relative inline-flex h-9 items-center gap-2 overflow-hidden rounded-lg bg-accent/10 pr-1 pl-3 text-sm text-accent-soft ring-1 ring-accent/30">
        {pct != null && (
          <span className="absolute inset-y-0 left-0 bg-accent/15 transition-[width] duration-500" style={{ width: `${pct}%` }} />
        )}
        <LoaderCircle size={15} className="relative animate-spin" />
        <span className="relative font-medium whitespace-nowrap tabular-nums">
          {status.worker === "new_releases" ? tr.views.new : pct != null ? `%${pct}` : tr.sync.phases[progress?.phase ?? "starting"]}
        </span>
        <button
          type="button"
          onClick={onCancel}
          className="relative grid size-7 place-items-center rounded-md text-accent-soft/80 hover:bg-white/10 hover:text-white"
          aria-label={tr.sync.cancel}
          title={tr.sync.cancel}
        >
          <X size={14} />
        </button>
      </div>
    );
  }

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        className="inline-flex h-9 items-center gap-2 rounded-lg bg-gradient-to-r from-accent-strong to-violet-strong px-3.5 text-sm font-semibold text-white shadow-lg shadow-accent/15 transition hover:brightness-110"
      >
        <RefreshCw size={15} />
        <span className="hidden xl:inline">{tr.sync.update}</span>
        <ChevronDown size={14} className={clsx("transition", open && "rotate-180")} />
      </button>
      {open && (
        <div
          role="menu"
          className="animate-rise absolute right-0 z-40 mt-2 w-80 rounded-xl bg-ink-800 p-1.5 shadow-2xl shadow-black/60 ring-1 ring-white/10"
        >
          <MenuItem
            icon={<Sparkles size={17} className="text-violet" />}
            title={tr.sync.newReleases}
            hint={tr.sync.newReleasesHint(status?.lastNewReleasesAt ?? null)}
            onClick={() => {
              setOpen(false);
              onNewReleases();
            }}
          />
          <MenuItem
            icon={<Database size={17} className="text-accent" />}
            title={tr.sync.fullSync}
            hint={tr.sync.fullSyncHint(status?.lastSyncAt ?? null)}
            onClick={() => {
              setOpen(false);
              onFullSync();
            }}
          />
        </div>
      )}
    </div>
  );
}

function MenuItem({ icon, title, hint, onClick }: { icon: ReactNode; title: string; hint: string; onClick: () => void }) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className="flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition hover:bg-white/6"
    >
      <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg bg-white/5 ring-1 ring-white/8">{icon}</span>
      <span className="min-w-0">
        <span className="block text-sm font-medium text-ink-50">{title}</span>
        <span className="block text-xs text-ink-400">{hint}</span>
      </span>
    </button>
  );
}
