import clsx from "clsx";
import { LoaderCircle, Sparkles, Store as StoreIcon, X } from "lucide-react";
import { deckLabel, tr } from "../i18n/tr";
import type { AppStatus } from "../lib/types";
import type { FiltersState, NewDays } from "../hooks/useFilters";
import { StoreMark } from "./badges";

interface Props {
  f: FiltersState;
  total: number | undefined;
  status: AppStatus | undefined;
  tagName: (tagid: number) => string | undefined;
  onNewReleases: () => void;
  onStoreSync: () => void;
}

const PERIODS: NewDays[] = [7, 30, 90];

export function ViewHeader({ f, total, status, tagName, onNewReleases, onStoreSync }: Props) {
  const filtered = f.searching || f.activeCount > 0;
  const count = total == null ? "" : filtered ? tr.count.results(total) : tr.count.games(total);
  const busy = status?.worker === "new_releases";

  return (
    <div className="shrink-0 px-8 pt-6 pb-4">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="min-w-0">
          {f.view === "all" && (
            <>
              <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">{tr.views.all}</h1>
              <p className="mt-1 h-5 text-sm text-ink-400 tabular-nums">{count}</p>
            </>
          )}
          {f.view === "new" && (
            <>
              <h1 className="flex items-center gap-2 font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">
                <Sparkles size={22} className="text-violet" />
                {tr.newView.title}
              </h1>
              <p className="mt-1 h-5 text-sm text-ink-400 tabular-nums">
                {total == null ? "" : filtered ? tr.count.results(total) : tr.newView.subtitle(total, f.newDays)}
                <span className="text-ink-600"> · </span>
                {tr.sync.newReleasesHint(status?.lastNewReleasesAt ?? null)}
              </p>
            </>
          )}
          {f.view === "links" && (
            <>
              <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">{tr.linksView.title}</h1>
              <p className="mt-1 h-5 text-sm text-ink-400">
                {tr.linksView.subtitle}
                {total != null && total > 0 && <span className="tabular-nums"> · {tr.count.games(total)}</span>}
              </p>
            </>
          )}
          {(f.view === "gog" || f.view === "itch") && (
            <>
              <h1 className="flex items-center gap-2.5 font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">
                <StoreMark store={f.view} size={26} />
                {tr.views[f.view]}
              </h1>
              <p className="mt-1 h-5 text-sm text-ink-400 tabular-nums">
                {total == null
                  ? ""
                  : filtered
                    ? tr.count.results(total)
                    : f.view === "gog"
                      ? tr.stores.gogView.subtitle(total)
                      : tr.stores.itchView.subtitle(total)}
                {f.view === "gog" && status?.storeCounts.lastStoreSyncAt != null && (
                  <>
                    <span className="text-ink-600"> · </span>
                    {tr.stores.gogView.lastSync(status.storeCounts.lastStoreSyncAt)}
                  </>
                )}
              </p>
            </>
          )}
        </div>

        {f.view === "gog" && (status?.storeCounts.gogProducts ?? 0) > 0 && (
          <button
            type="button"
            onClick={onStoreSync}
            disabled={status?.worker != null}
            className="inline-flex h-9 items-center gap-2 rounded-lg bg-gog/12 px-3.5 text-sm font-semibold text-gog ring-1 ring-gog/35 transition hover:bg-gog/20 disabled:opacity-60"
          >
            {status?.worker === "stores" ? <LoaderCircle size={15} className="animate-spin" /> : <StoreIcon size={15} />}
            {tr.sync.storesSync}
          </button>
        )}

        {f.view === "new" && (
          <div className="flex items-center gap-3">
            <div className="flex rounded-lg bg-white/4 p-1 ring-1 ring-white/6">
              {PERIODS.map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => f.setNewDays(d)}
                  aria-pressed={f.newDays === d}
                  className={clsx(
                    "h-7 rounded-md px-3 text-[13px] font-medium transition",
                    f.newDays === d ? "bg-ink-600 text-white shadow ring-1 ring-white/10" : "text-ink-300 hover:text-ink-50",
                  )}
                >
                  {tr.newView.period(d)}
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={onNewReleases}
              disabled={status?.worker != null}
              className="inline-flex h-9 items-center gap-2 rounded-lg bg-violet/15 px-3.5 text-sm font-semibold text-violet ring-1 ring-violet/35 transition hover:bg-violet/25 disabled:opacity-60"
            >
              {busy ? <LoaderCircle size={15} className="animate-spin" /> : <Sparkles size={15} />}
              {tr.sync.newReleases}
            </button>
          </div>
        )}
      </div>

      <ActiveFilters f={f} tagName={tagName} />
    </div>
  );
}

function ActiveFilters({ f, tagName }: { f: FiltersState; tagName: (tagid: number) => string | undefined }) {
  const chips: { key: string; label: string; remove: () => void }[] = [];
  for (const t of f.filters.tags) {
    chips.push({ key: `t${t}`, label: tagName(t) ?? `#${t}`, remove: () => f.toggleTag(t) });
  }
  for (const p of f.filters.platforms) {
    chips.push({ key: `p${p}`, label: tr.platforms[p], remove: () => f.togglePlatform(p) });
  }
  if (f.filters.deck) {
    chips.push({
      key: "deck",
      label: `Steam Deck: ${deckLabel(f.filters.deck === "verified" ? 3 : 2)}${f.filters.deck === "playable" ? "+" : ""}`,
      remove: () => f.patch({ deck: null }),
    });
  }
  if (f.filters.minReviewScore) {
    const label =
      f.filters.minReviewScore >= 9
        ? tr.filters.reviewsOverwhelming
        : f.filters.minReviewScore >= 8
          ? tr.filters.reviewsVeryPositive
          : tr.filters.reviewsMostlyPositive;
    chips.push({ key: "rev", label, remove: () => f.patch({ minReviewScore: null }) });
  }
  if (f.filters.freeOnly) chips.push({ key: "free", label: tr.card.free, remove: () => f.patch({ freeOnly: false }) });
  if (f.filters.hasLinks && f.view !== "links")
    chips.push({ key: "links", label: tr.views.links, remove: () => f.patch({ hasLinks: false }) });
  if (f.showAdult) chips.push({ key: "adult", label: "+18", remove: () => f.setShowAdult(false) });

  if (chips.length === 0) return null;
  return (
    <div className="mt-4 flex flex-wrap items-center gap-2">
      {chips.map((c) => (
        <span
          key={c.key}
          className="animate-fade-in inline-flex h-7 items-center gap-1 rounded-full bg-accent/10 pr-1 pl-3 text-[13px] text-accent-soft ring-1 ring-accent/25"
        >
          {c.label}
          <button
            type="button"
            onClick={c.remove}
            className="grid size-5 place-items-center rounded-full hover:bg-accent/20"
            aria-label={tr.filters.removeFilter(c.label)}
          >
            <X size={12} />
          </button>
        </span>
      ))}
      {chips.length > 1 && (
        <button type="button" onClick={f.clear} className="ml-1 text-[13px] font-medium text-ink-400 hover:text-white">
          {tr.filters.clearAll}
        </button>
      )}
    </div>
  );
}
