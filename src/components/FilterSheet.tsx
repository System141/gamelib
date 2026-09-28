import clsx from "clsx";
import { Check, Search, X } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { tr } from "../i18n/tr";
import { fold } from "../lib/fold";
import { formatNumber } from "../lib/format";
import type { DeckFilter, Platform, TagInfo } from "../lib/types";
import type { FiltersState } from "../hooks/useFilters";
import { AppleIcon, LinuxIcon, SteamIcon, WindowsIcon } from "./icons";

const COLLAPSED_TAGS = 36;

interface Props {
  open: boolean;
  onClose: () => void;
  f: FiltersState;
  tags: TagInfo[];
}

export function FilterSheet({ open, onClose, f, tags }: Props) {
  const [tagQuery, setTagQuery] = useState("");
  const [showAll, setShowAll] = useState(false);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    panel.current?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const visibleTags = useMemo(() => {
    const q = fold(tagQuery.trim());
    const selected = new Set(f.filters.tags);
    const matching = q ? tags.filter((t) => fold(t.name).includes(q)) : tags;
    // Selected tags stay on top; the rest keep the "most games first" order.
    const pinned = tags.filter((t) => selected.has(t.tagid));
    const rest = matching.filter((t) => !selected.has(t.tagid));
    const limited = q || showAll ? rest : rest.slice(0, Math.max(0, COLLAPSED_TAGS - pinned.length));
    return [...pinned, ...limited];
  }, [tags, tagQuery, showAll, f.filters.tags]);

  if (!open) return null;

  const platforms: { id: Platform; label: string; icon: ReactNode }[] = [
    { id: "win", label: tr.platforms.win, icon: <WindowsIcon size={14} /> },
    { id: "mac", label: tr.platforms.mac, icon: <AppleIcon size={14} /> },
    { id: "linux", label: tr.platforms.linux, icon: <LinuxIcon size={14} /> },
  ];
  const deckOptions: { value: DeckFilter | null; label: string }[] = [
    { value: null, label: tr.filters.deckAny },
    { value: "playable", label: tr.filters.deckPlayable },
    { value: "verified", label: tr.filters.deckVerified },
  ];
  const reviewOptions: { value: number | null; label: string }[] = [
    { value: null, label: tr.filters.reviewsAny },
    { value: 6, label: tr.filters.reviewsMostlyPositive },
    { value: 8, label: tr.filters.reviewsVeryPositive },
    { value: 9, label: tr.filters.reviewsOverwhelming },
  ];

  return (
    <div className="fixed inset-0 z-50">
      <div className="animate-fade-in absolute inset-0 bg-ink-950/60 backdrop-blur-sm" onClick={onClose} />
      <aside
        ref={panel}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={tr.filters.title}
        className="animate-slide-in absolute inset-y-0 right-0 flex w-[400px] max-w-[92vw] flex-col bg-ink-850 shadow-2xl shadow-black ring-1 ring-white/8 outline-none"
      >
        <div className="flex items-center justify-between border-b border-white/6 px-6 py-4">
          <h2 className="font-display text-xl font-semibold text-ink-50">{tr.filters.title}</h2>
          <button type="button" onClick={onClose} className="grid size-9 place-items-center rounded-lg text-ink-300 hover:bg-white/8 hover:text-white" aria-label={tr.filters.close}>
            <X size={18} />
          </button>
        </div>

        <div className="flex-1 space-y-7 overflow-y-auto px-6 py-5">
          <Section title={tr.filters.tags} count={f.filters.tags.length}>
            <label className="relative mb-3 flex items-center">
              <Search size={14} className="pointer-events-none absolute left-3 text-ink-400" />
              <input
                value={tagQuery}
                onChange={(e) => setTagQuery(e.target.value)}
                placeholder={tr.filters.tagSearch}
                className="h-9 w-full rounded-lg bg-ink-800 pr-3 pl-9 text-sm text-ink-50 ring-1 ring-white/8 outline-none placeholder:text-ink-400 focus:ring-accent/50"
              />
            </label>
            <div className="flex flex-wrap gap-1.5">
              {visibleTags.map((t) => {
                const on = f.filters.tags.includes(t.tagid);
                return (
                  <button
                    key={t.tagid}
                    type="button"
                    onClick={() => f.toggleTag(t.tagid)}
                    aria-pressed={on}
                    className={clsx(
                      "inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[13px] ring-1 transition",
                      on ? "bg-accent/15 text-accent-soft ring-accent/40" : "bg-white/4 text-ink-200 ring-white/6 hover:bg-white/8 hover:text-ink-50",
                    )}
                  >
                    {on && <Check size={13} />}
                    {t.name}
                    <span className={clsx("text-[11px] tabular-nums", on ? "text-accent/80" : "text-ink-500")}>{formatNumber(t.gameCount)}</span>
                  </button>
                );
              })}
              {visibleTags.length === 0 && <p className="text-sm text-ink-400">{tr.filters.noTags}</p>}
            </div>
            {!tagQuery && tags.length > COLLAPSED_TAGS && (
              <button type="button" onClick={() => setShowAll((s) => !s)} className="mt-3 text-sm font-medium text-accent hover:text-accent-soft">
                {showAll ? tr.filters.showFewerTags : tr.filters.showAllTags(tags.length)}
              </button>
            )}
          </Section>

          <Section title={tr.filters.platforms}>
            <div className="grid grid-cols-3 gap-2">
              {platforms.map((p) => {
                const on = f.filters.platforms.includes(p.id);
                return (
                  <button
                    key={p.id}
                    type="button"
                    aria-pressed={on}
                    onClick={() => f.togglePlatform(p.id)}
                    className={clsx(
                      "flex h-10 items-center justify-center gap-2 rounded-lg text-sm ring-1 transition",
                      on ? "bg-accent/15 text-accent-soft ring-accent/40" : "bg-white/4 text-ink-200 ring-white/6 hover:bg-white/8",
                    )}
                  >
                    {p.icon}
                    {p.label}
                  </button>
                );
              })}
            </div>
          </Section>

          <Section title={tr.filters.deck} icon={<SteamIcon size={14} className="text-ink-400" />}>
            <Segmented options={deckOptions} value={f.filters.deck} onChange={(deck) => f.patch({ deck })} />
          </Section>

          <Section title={tr.filters.reviews}>
            <Segmented options={reviewOptions} value={f.filters.minReviewScore} onChange={(minReviewScore) => f.patch({ minReviewScore })} small />
          </Section>

          <Section title={tr.filters.other}>
            <div className="space-y-1">
              <Toggle label={tr.filters.freeOnly} checked={f.filters.freeOnly} onChange={(freeOnly) => f.patch({ freeOnly })} />
              {f.view !== "links" && <Toggle label={tr.filters.hasLinks} checked={f.filters.hasLinks} onChange={(hasLinks) => f.patch({ hasLinks })} />}
              <Toggle label={tr.filters.showAdult} hint={tr.filters.showAdultHint} checked={f.showAdult} onChange={f.setShowAdult} />
            </div>
          </Section>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-white/6 px-6 py-4">
          <button type="button" onClick={f.clear} disabled={f.activeCount === 0} className="text-sm font-medium text-ink-300 hover:text-white disabled:opacity-40">
            {tr.filters.clearAll}
          </button>
          <button type="button" onClick={onClose} className="h-10 rounded-lg bg-gradient-to-r from-accent-strong to-violet-strong px-5 text-sm font-semibold text-white shadow-lg shadow-accent/15 hover:brightness-110">
            {tr.filters.apply}
          </button>
        </div>
      </aside>
    </div>
  );
}

function Section({ title, count, icon, children }: { title: string; count?: number; icon?: ReactNode; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-3 flex items-center gap-2 text-xs font-semibold tracking-wider text-ink-400 uppercase">
        {icon}
        {title}
        {count ? <span className="rounded-full bg-accent/20 px-1.5 text-[11px] text-accent-soft normal-case">{count}</span> : null}
      </h3>
      {children}
    </section>
  );
}

function Segmented<T>({ options, value, onChange, small = false }: { options: { value: T; label: string }[]; value: T; onChange: (v: T) => void; small?: boolean }) {
  return (
    <div className="grid rounded-lg bg-ink-800 p-1 ring-1 ring-white/6" style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}>
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          onClick={() => onChange(o.value)}
          aria-pressed={o.value === value}
          className={clsx(
            "rounded-md px-1 font-medium transition",
            small ? "py-1.5 text-[11.5px] leading-tight" : "py-2 text-[13px]",
            o.value === value ? "bg-ink-600 text-white shadow ring-1 ring-white/10" : "text-ink-300 hover:text-ink-50",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Toggle({ label, hint, checked, onChange }: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4 rounded-lg px-1 py-2 hover:bg-white/3">
      <span>
        <span className="block text-sm text-ink-100">{label}</span>
        {hint && <span className="mt-0.5 block text-xs text-ink-400">{hint}</span>}
      </span>
      <span className="relative mt-0.5 shrink-0">
        <input type="checkbox" className="peer sr-only" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <span className="block h-5 w-9 rounded-full bg-ink-600 transition peer-checked:bg-accent peer-focus-visible:ring-2 peer-focus-visible:ring-accent/60" />
        <span className="absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition peer-checked:translate-x-4" />
      </span>
    </label>
  );
}
