// "Mağazalar": the game's products on other stores (GOG, itch.io), found by matching.

import clsx from "clsx";
import { Check, ExternalLink, LoaderCircle, Search, ShieldCheck, Store as StoreIcon, X } from "lucide-react";
import { useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { showToast } from "../lib/toast";
import type { MatchState, StoreMatch, StoreSearchHit } from "../lib/types";
import { useAccounts, useLinkStoreProduct, useSetMatchState, useStatus, useStoreLookup, useStoreMatches } from "../hooks/useData";
import { StoreMark } from "./badges";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";
import { DownloadButton, DownloadLine, useProductDownload } from "./DownloadPicker";
import { InstallMenu, PlayButton, useGameInstall } from "./InstallActions";
import { SmallButton } from "./ui";

interface Props {
  appid: number;
  onStoreSync: () => void;
}

export function StoresSection({ appid, onStoreSync }: Props) {
  const matches = useStoreMatches(appid);
  const lookup = useStoreLookup(appid);
  const status = useStatus();
  const list = matches.data ?? [];
  const confident = list.filter((m) => m.confident);
  const suggestions = list.filter((m) => !m.confident);
  const synced = (status.data?.storeCounts.gogProducts ?? 0) > 0;
  const checking = lookup.isFetching && confident.length === 0;
  const accounts = useAccounts();
  const canSearchItch = !!accounts.data?.itch && !confident.some((m) => m.store === "itch");

  return (
    <section>
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 font-display text-lg font-semibold text-ink-50">
          <StoreIcon size={18} className="text-gog" />
          {tr.stores.title}
          {confident.length > 0 && (
            <span className="rounded-full bg-white/8 px-2 text-xs font-medium text-ink-300">{confident.length}</span>
          )}
        </h3>
        {lookup.isFetching && (
          <span className="inline-flex items-center gap-1.5 text-xs text-ink-400">
            <LoaderCircle size={13} className="animate-spin" />
            {tr.stores.checking}
          </span>
        )}
      </div>
      <p className="mb-4 text-[13px] leading-relaxed text-ink-400">{tr.stores.hint}</p>

      <div className="space-y-2">
        {confident.map((m) => (
          <MatchRow key={`${m.store}:${m.productId}`} match={m} appid={appid} />
        ))}
        {matches.isSuccess && confident.length === 0 && !checking && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-dashed border-white/10 px-4 py-4 text-sm text-ink-400">
            <span>{synced ? tr.stores.none : tr.stores.notSynced}</span>
            {!synced && (
              <SmallButton tone="primary" onClick={onStoreSync} icon={<StoreIcon size={13} />}>
                {tr.stores.syncCta}
              </SmallButton>
            )}
          </div>
        )}
        {checking && <div className="shimmer h-[76px] rounded-xl ring-1 ring-white/6" />}
      </div>

      {suggestions.length > 0 && <Suggestions matches={suggestions} appid={appid} />}
      {canSearchItch && <ItchSearch appid={appid} />}
    </section>
  );
}

/** itch.io has no Steam id cross-reference: search it and let the user pick. */
function ItchSearch({ appid }: { appid: number }) {
  const [hits, setHits] = useState<StoreSearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);
  const link = useLinkStoreProduct();

  const search = () => {
    setSearching(true);
    api
      .searchStore("itch", appid)
      .then(setHits)
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }))
      .finally(() => setSearching(false));
  };
  const pick = (hit: StoreSearchHit) =>
    link.mutate(
      { store: hit.store, productId: hit.productId, appid },
      {
        onSuccess: () => {
          setHits(null);
          showToast({ tone: "success", title: tr.stores.toastConfirmed });
        },
        onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
      },
    );

  if (hits == null) {
    return (
      <div className="mt-3">
        <SmallButton
          onClick={search}
          disabled={searching}
          icon={searching ? <LoaderCircle size={13} className="animate-spin" /> : <Search size={13} />}
        >
          {searching ? tr.stores.searching : tr.stores.searchItch}
        </SmallButton>
      </div>
    );
  }
  return (
    <div className="mt-3 rounded-xl bg-white/2 px-4 py-3 ring-1 ring-white/6">
      <p className="mb-2 text-xs text-ink-400">{hits.length === 0 ? tr.stores.noHits : tr.stores.itchHint}</p>
      <div className="space-y-1.5">
        {hits.slice(0, 6).map((h) => (
          <div key={h.productId} className="flex items-center gap-2.5">
            <StoreMark store="itch" size={18} />
            <span className="min-w-0 flex-1 truncate text-sm text-ink-100" title={h.title}>
              {h.title}
              {h.developer && <span className="ml-2 text-xs text-ink-400">{h.developer}</span>}
              {h.price && <span className="ml-2 text-xs text-ink-500">{h.price}</span>}
            </span>
            <SmallButton tone={h.score >= 0.85 ? "primary" : "default"} onClick={() => pick(h)} icon={<Check size={13} />}>
              {tr.stores.thisOne}
            </SmallButton>
          </div>
        ))}
      </div>
    </div>
  );
}

function useVerdict(appid: number) {
  const set = useSetMatchState();
  return (match: StoreMatch, state: MatchState) =>
    set.mutate(
      { match, appid, state },
      {
        onSuccess: () => {
          if (state === "auto") return;
          showToast({
            tone: "success",
            title: state === "rejected" ? tr.stores.toastRejected : tr.stores.toastConfirmed,
            action:
              state === "rejected" ? { label: tr.stores.undo, onClick: () => set.mutate({ match, appid, state: match.state }) } : undefined,
          });
        },
        onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
      },
    );
}

function MatchRow({ match, appid }: { match: StoreMatch; appid: number }) {
  const verdict = useVerdict(appid);
  const accounts = useAccounts();
  const { download, live, installing } = useProductDownload(match.store, match.productId);
  const installed = useGameInstall(match.store, match.productId);
  const canDownload = !installed && !!accounts.data?.[match.store] && (match.owned || (match.store === "itch" && match.isFree));
  const showDownload = download && !(installed && download.state === "completed" && download.installState !== "installing");
  const image = match.coverWide ?? match.cover;
  const open = () =>
    api.openStorePage(match.store, match.productId).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
  const byTitle = match.method === "title" && match.state === "auto";

  return (
    <div className="rounded-xl bg-ink-800/80 p-3 ring-1 ring-white/6 transition hover:ring-white/12">
      <div className="flex items-center gap-3.5">
        <div className="relative aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-ink-700 ring-1 ring-white/8">
          {image ? (
            <img src={image} alt="" loading="lazy" decoding="async" className="size-full object-cover" />
          ) : (
            <div className="grid size-full place-items-center">
              <StoreMark store={match.store} size={28} />
            </div>
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <StoreMark store={match.store} size={18} />
            <span className="truncate font-medium text-ink-50" title={match.title}>
              {match.title}
            </span>
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs">
            <span className={clsx("font-semibold", match.owned ? "text-success" : "text-ink-100")}>
              {match.owned ? tr.stores.owned : match.isFree ? tr.stores.free : (match.price ?? "")}
            </span>
            <Platforms match={match} />
            <span className="text-ink-500">·</span>
            <span className={clsx("inline-flex items-center gap-1", match.method === "title" ? "text-ink-400" : "text-ink-300")}>
              {match.method !== "title" && <ShieldCheck size={12} className="text-success" />}
              {match.state === "confirmed" ? tr.stores.confirmed : tr.stores.method[match.method]}
            </span>
          </div>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          <span className="flex gap-1.5">
            {installed && <PlayButton installed={installed} />}
            {canDownload && !download && <DownloadButton store={match.store} productId={match.productId} title={match.title} />}
            <SmallButton tone={canDownload || installed ? "default" : "primary"} onClick={open} icon={<ExternalLink size={13} />}>
              {tr.stores.openIn[match.store]}
            </SmallButton>
            {installed && <InstallMenu installed={installed} placement="down" />}
          </span>
          {byTitle ? (
            <span className="inline-flex items-center gap-1 text-[11.5px] text-ink-400">
              {tr.stores.askCorrect}
              <InlineChoice label={tr.stores.yes} onClick={() => verdict(match, "confirmed")} />
              <InlineChoice label={tr.stores.no} onClick={() => verdict(match, "rejected")} />
            </span>
          ) : (
            <button
              type="button"
              onClick={() => verdict(match, "rejected")}
              className="text-[11.5px] text-ink-500 underline-offset-2 hover:text-ink-200 hover:underline"
            >
              {tr.stores.wrong}
            </button>
          )}
        </div>
      </div>
      {showDownload && (
        <div className="mt-3 border-t border-white/6 pt-2.5">
          <DownloadLine download={download} live={live} installing={installing} />
        </div>
      )}
    </div>
  );
}

function Suggestions({ matches, appid }: { matches: StoreMatch[]; appid: number }) {
  const [open, setOpen] = useState(false);
  const verdict = useVerdict(appid);
  return (
    <div className="mt-3 rounded-xl bg-white/2 ring-1 ring-white/6">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex w-full items-center justify-between px-4 py-2.5 text-left text-[13px] font-medium text-ink-200 hover:text-white"
      >
        {tr.stores.suggestions(matches.length)}
        <span className={clsx("text-ink-500 transition", open && "rotate-180")}>▾</span>
      </button>
      {open && (
        <div className="space-y-1.5 border-t border-white/6 px-4 py-3">
          <p className="mb-2 text-xs text-ink-400">{tr.stores.suggestionsHint}</p>
          {matches.map((m) => (
            <div key={`${m.store}:${m.productId}`} className="flex items-center gap-2.5">
              <StoreMark store={m.store} size={18} />
              <span className="min-w-0 flex-1 truncate text-sm text-ink-100" title={m.title}>
                {m.title}
                {m.price && <span className="ml-2 text-xs text-ink-400">{m.price}</span>}
              </span>
              <SmallButton onClick={() => verdict(m, "rejected")} icon={<X size={13} />}>
                {tr.stores.notThis}
              </SmallButton>
              <SmallButton tone="primary" onClick={() => verdict(m, "confirmed")} icon={<Check size={13} />}>
                {tr.stores.thisOne}
              </SmallButton>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function InlineChoice({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded px-1 font-medium text-ink-200 ring-1 ring-white/10 hover:bg-white/8 hover:text-white"
    >
      {label}
    </button>
  );
}

function Platforms({ match }: { match: StoreMatch }) {
  if (!match.win && !match.mac && !match.linux) return null;
  return (
    <span className="inline-flex items-center gap-1 text-ink-400">
      {match.win && <WindowsIcon size={11} />}
      {match.mac && <AppleIcon size={11} />}
      {match.linux && <LinuxIcon size={11} />}
    </span>
  );
}
