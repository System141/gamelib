// "Bu hangi oyun?": ties a game found on this computer to a Steam game from the catalog (or to
// none), so its details, prices and reviews can be shown.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, LoaderCircle, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatYear } from "../lib/format";
import { showToast } from "../lib/toast";
import type { Installed } from "../lib/types";
import { useDebounced } from "../hooks/useUtils";
import { FoundMark } from "./badges";
import { SmallButton } from "./ui";

const RESULTS = 8;

export function FoundMatchDialog({ installed, onClose }: { installed: Installed; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const qc = useQueryClient();
  const [term, setTerm] = useState(installed.title);
  const [saving, setSaving] = useState<number | "none" | null>(null);
  const search = useDebounced(term.trim(), 250);
  const results = useQuery({
    queryKey: ["match-search", search],
    queryFn: () =>
      api.queryGames({
        search,
        tags: [],
        platforms: [],
        deck: null,
        freeOnly: false,
        minReviewScore: null,
        showAdult: true,
        releasedWithinDays: null,
        hasLinks: false,
        stores: [],
        owned: false,
        sort: "relevance",
        offset: 0,
        limit: RESULTS,
      }),
    enabled: search.length >= 2,
    staleTime: 60_000,
  });

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const close = () => ref.current?.close();
  const choose = (appid: number | null) => {
    setSaving(appid ?? "none");
    api
      .matchFound(installed.productId, appid)
      .then((updated) => {
        qc.setQueryData<Installed[]>(["installs"], (list) =>
          list?.map((i) => (i.store === updated.store && i.productId === updated.productId ? updated : i)),
        );
        void qc.invalidateQueries({ queryKey: ["installs"] });
        void qc.invalidateQueries({ queryKey: ["hidden-found"] });
        showToast({ tone: "success", title: appid == null ? tr.found.toastUnmatched : tr.found.toastMatched(updated.title) });
        close();
      })
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }))
      .finally(() => setSaving(null));
  };

  const items = results.data?.items ?? [];
  return (
    <dialog
      ref={ref}
      // Close and cancel events would otherwise reach a dialog around this one.
      onClose={(e) => {
        e.stopPropagation();
        onClose();
      }}
      onCancel={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        if (e.target === ref.current) close();
      }}
      className="m-auto w-[min(560px,94vw)] max-w-none rounded-2xl bg-ink-850 p-0 text-ink-100 shadow-2xl shadow-black ring-1 ring-white/10 open:animate-rise"
      aria-label={tr.found.matchTitle}
    >
      <div className="p-6">
        <div className="flex items-start gap-3">
          {installed.source && <FoundMark source={installed.source} size={26} className="mt-0.5" />}
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm text-ink-400" title={installed.dir ?? undefined}>
              {installed.dir ?? installed.title}
            </div>
            <h2 className="font-display text-lg font-semibold text-ink-50">{tr.found.matchTitle}</h2>
          </div>
          <button
            type="button"
            onClick={close}
            className="grid size-8 shrink-0 place-items-center rounded-lg text-ink-400 hover:bg-white/8 hover:text-white"
            aria-label={tr.detail.close}
          >
            <X size={17} />
          </button>
        </div>
        <p className="mt-2 text-[13px] leading-relaxed text-ink-400">{tr.found.matchHint}</p>

        <div className="relative mt-4">
          <Search size={15} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-500" />
          <input
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            placeholder={tr.found.searchPlaceholder}
            aria-label={tr.found.searchPlaceholder}
            spellCheck={false}
            autoFocus
            className="h-10 w-full rounded-lg bg-ink-900 pr-3 pl-9 text-sm text-ink-100 ring-1 ring-white/10 outline-none placeholder:text-ink-500 focus:ring-accent/50"
          />
        </div>

        <ul className="mt-3 max-h-[44vh] space-y-1.5 overflow-y-auto pr-1" aria-label={tr.found.matchTitle}>
          {results.isFetching && items.length === 0 ? (
            <li className="flex items-center gap-2 py-6 text-sm text-ink-400">
              <LoaderCircle size={15} className="animate-spin" />
              {tr.found.searching}
            </li>
          ) : search.length >= 2 && results.isSuccess && items.length === 0 ? (
            <li className="py-6 text-sm text-ink-400">{tr.found.noResults}</li>
          ) : (
            items.map((g) => {
              const current = g.appid === installed.appid;
              return (
                <li key={g.appid} className="flex items-center gap-3 rounded-xl bg-white/3 p-2 ring-1 ring-white/6">
                  <div className="aspect-[460/215] w-28 shrink-0 overflow-hidden rounded-md bg-ink-700">
                    {g.header && <img src={g.header} alt="" loading="lazy" className="size-full object-cover" />}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-ink-50" title={g.name}>
                      {g.name}
                    </div>
                    <div className="mt-0.5 text-xs text-ink-400 tabular-nums">
                      {formatYear(g.releaseDate)}
                      {current && <span className="ml-2 text-success">{tr.found.current}</span>}
                    </div>
                  </div>
                  <SmallButton
                    tone={current ? "default" : "primary"}
                    onClick={() => choose(g.appid)}
                    disabled={saving != null || current}
                    icon={saving === g.appid ? <LoaderCircle size={13} className="animate-spin" /> : <Check size={13} />}
                  >
                    {tr.found.thisOne}
                  </SmallButton>
                </li>
              );
            })
          )}
        </ul>

        <div className="mt-4 flex items-center justify-end border-t border-white/8 pt-4">
          <SmallButton
            onClick={() => choose(null)}
            disabled={saving != null || (installed.appid == null && installed.matchedBy === "manual")}
            icon={saving === "none" ? <LoaderCircle size={13} className="animate-spin" /> : <X size={13} />}
          >
            {tr.found.notOnSteam}
          </SmallButton>
        </div>
      </div>
    </dialog>
  );
}
