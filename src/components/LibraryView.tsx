// "Sahip olduklarım": games the user owns on the signed-in stores (GOG, itch.io).

import clsx from "clsx";
import { ExternalLink, Info, LoaderCircle, RefreshCw, Settings as SettingsIcon, Store as StoreIcon } from "lucide-react";
import { type ReactNode, useMemo } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { normalizeName } from "../lib/fold";
import { showToast } from "../lib/toast";
import type { AppStatus, LibraryItem, Store } from "../lib/types";
import { useAccounts, useLibrary } from "../hooks/useData";
import { usePersistentState } from "../hooks/useUtils";
import { StoreMark, StorePill } from "./badges";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";
import { SmallButton } from "./ui";

interface Props {
  search: string;
  status: AppStatus | undefined;
  onOpenGame: (appid: number) => void;
  onRefresh: () => void;
  onOpenSettings: () => void;
}

type StoreFilter = Store | "all";

export function LibraryView({ search, status, onOpenGame, onRefresh, onOpenSettings }: Props) {
  const library = useLibrary();
  const accounts = useAccounts();
  const [store, setStore] = usePersistentState<StoreFilter>("gamelib.libraryStore", "all");
  const term = normalizeName(search.trim());
  const all = library.data ?? [];
  const items = useMemo(
    () => all.filter((i) => (store === "all" || i.store === store) && (!term || normalizeName(i.title).includes(term))),
    [all, store, term],
  );
  const signedIn = !!(accounts.data?.gog || accounts.data?.itch);
  const busy = status?.worker === "library" || status?.worker === "stores";
  const stores: StoreFilter[] = ["all", "gog", "itch"];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-end justify-between gap-4 px-8 pt-6 pb-5">
        <div className="min-w-0">
          <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">{tr.views.library}</h1>
          <p className="mt-1 h-5 text-sm text-ink-400 tabular-nums">{library.isSuccess ? tr.library.subtitle(all.length) : ""}</p>
        </div>
        {signedIn && (
          <div className="flex items-center gap-3">
            <div className="flex rounded-lg bg-white/4 p-1 ring-1 ring-white/6" role="group" aria-label={tr.library.storeFilter}>
              {stores.map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => setStore(s)}
                  aria-pressed={store === s}
                  className={clsx(
                    "inline-flex h-7 items-center gap-1.5 rounded-md px-3 text-[13px] font-medium transition",
                    store === s ? "bg-ink-600 text-white shadow ring-1 ring-white/10" : "text-ink-300 hover:text-ink-50",
                  )}
                >
                  {s !== "all" && <StoreMark store={s} size={15} />}
                  {s === "all" ? tr.library.all : tr.storeNames[s]}
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={onRefresh}
              disabled={status?.worker != null}
              className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-3.5 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10 disabled:opacity-60"
            >
              {busy ? <LoaderCircle size={15} className="animate-spin" /> : <RefreshCw size={15} />}
              {tr.library.refresh}
            </button>
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-10">
        {!library.isSuccess || !accounts.isSuccess ? null : !signedIn ? (
          <Empty
            icon={<StoreIcon size={26} className="text-ink-400" />}
            title={tr.library.emptyNoAccountTitle}
            text={tr.library.emptyNoAccount}
            action={{ label: tr.library.connect, onClick: onOpenSettings, icon: <SettingsIcon size={15} /> }}
          />
        ) : all.length === 0 ? (
          <Empty
            icon={<StoreIcon size={26} className="text-ink-400" />}
            title={tr.library.emptyTitle}
            text={tr.library.emptyText}
            action={{ label: tr.library.refresh, onClick: onRefresh, icon: <RefreshCw size={15} /> }}
          />
        ) : items.length === 0 ? (
          <Empty icon={<Info size={26} className="text-ink-400" />} title={tr.empty.title} text={tr.library.noResults} />
        ) : (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-5" aria-label={tr.views.library}>
            {items.map((item) => (
              <li key={`${item.store}:${item.productId}`}>
                <LibraryCard item={item} onOpenGame={onOpenGame} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function LibraryCard({ item, onOpenGame }: { item: LibraryItem; onOpenGame: (appid: number) => void }) {
  const image = item.steamHeader ?? item.coverWide ?? item.cover;
  const openStore = () =>
    api.openStorePage(item.store, item.productId).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <div className="group flex h-full flex-col overflow-hidden rounded-xl bg-ink-800/80 shadow-lg shadow-black/30 ring-1 ring-white/6 transition hover:ring-white/15">
      <button
        type="button"
        onClick={() => (item.appid != null ? onOpenGame(item.appid) : openStore())}
        className="relative aspect-[460/215] w-full overflow-hidden bg-ink-700"
        aria-label={item.title}
      >
        {image ? (
          <img
            src={image}
            alt=""
            loading="lazy"
            decoding="async"
            className="size-full object-cover transition duration-500 group-hover:scale-[1.03]"
          />
        ) : (
          <div className="grid size-full place-items-center">
            <StoreMark store={item.store} size={36} />
          </div>
        )}
        <span className="absolute top-2 left-2">
          <StorePill store={item.store} className="shadow-md shadow-black/50" />
        </span>
      </button>
      <div className="flex flex-1 flex-col p-3">
        <div className="truncate text-[14px] font-semibold text-ink-50" title={item.title}>
          {item.title}
        </div>
        <div className="mt-1 flex items-center gap-2 text-xs text-ink-400">
          <span className="inline-flex items-center gap-1">
            {item.win && <WindowsIcon size={11} />}
            {item.mac && <AppleIcon size={11} />}
            {item.linux && <LinuxIcon size={11} />}
          </span>
          {item.appid == null && <span className="truncate">{tr.library.notOnSteam}</span>}
        </div>
        <div className="mt-3 flex flex-wrap justify-end gap-1.5">
          {item.appid != null && <SmallButton onClick={() => onOpenGame(item.appid!)}>{tr.library.details}</SmallButton>}
          <SmallButton tone="primary" onClick={openStore} icon={<ExternalLink size={13} />}>
            {tr.stores.openIn[item.store]}
          </SmallButton>
        </div>
      </div>
    </div>
  );
}

function Empty({
  icon,
  title,
  text,
  action,
}: {
  icon: ReactNode;
  title: string;
  text: string;
  action?: { label: string; onClick: () => void; icon?: ReactNode };
}) {
  return (
    <div className="grid h-full min-h-[360px] place-items-center">
      <div className="animate-fade-in max-w-md text-center">
        <div className="mx-auto grid size-16 place-items-center rounded-2xl bg-white/4 ring-1 ring-white/8">{icon}</div>
        <h2 className="mt-5 font-display text-xl font-semibold text-ink-50">{title}</h2>
        <p className="mt-2 text-sm text-ink-400">{text}</p>
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="mt-5 inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 hover:bg-white/10"
          >
            {action.icon}
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}
