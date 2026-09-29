// "Sahip olduklarım" (games owned on the signed-in stores) and "Kurulu" (installed games).

import clsx from "clsx";
import {
  CircleCheck,
  ExternalLink,
  FolderPlus,
  HardDrive,
  Info,
  Library,
  Link2,
  LoaderCircle,
  RefreshCw,
  Settings as SettingsIcon,
  Store as StoreIcon,
} from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { normalizeName } from "../lib/fold";
import { showToast } from "../lib/toast";
import type { AppStatus, FoundSource, Installed, LibraryItem, Store } from "../lib/types";
import { useAccounts, useAutoScan, useInstalls, useLibrary, useScanInstalled, useScanningInstalled } from "../hooks/useData";
import { usePersistentState } from "../hooks/useUtils";
import { FoundMark, FoundPill, StoreMark, StorePill } from "./badges";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";
import { DownloadButton, DownloadLine, useProductDownload } from "./DownloadPicker";
import { FoundMatchDialog } from "./FoundMatchDialog";
import { InstallMenu, PlayButton, useGameInstall } from "./InstallActions";
import { IconButton, SmallButton } from "./ui";

interface Props {
  /** "Kurulu": only installed games, owned or not. */
  installedOnly: boolean;
  search: string;
  status: AppStatus | undefined;
  onOpenGame: (appid: number) => void;
  onRefresh: () => void;
  onOpenSettings: () => void;
  onOpenLibrary: () => void;
}

/** A store for GameLib's and the stores' games, a source for games found on this computer. */
type Filter = Exclude<Store, "local"> | FoundSource | "all";

const INSTALLED_ORDER: Filter[] = ["all", "gog", "itch", "web", "steam", "epic", "folder"];

/** What a card is filtered by. */
function filterOf(item: LibraryItem, installed: Installed | undefined): Filter {
  return installed?.source ?? (item.store === "local" ? "folder" : item.store);
}

export function LibraryView({ installedOnly, search, status, onOpenGame, onRefresh, onOpenSettings, onOpenLibrary }: Props) {
  const library = useLibrary();
  const installs = useInstalls();
  const accounts = useAccounts();
  const scan = useScanInstalled();
  const scanning = useScanningInstalled();
  useAutoScan(installedOnly);
  const [chosen, setFilter] = usePersistentState<Filter>(installedOnly ? "gamelib.installedFilter" : "gamelib.libraryStore", "all");
  const term = normalizeName(search.trim());
  const all = useMemo(
    () =>
      installedOnly
        ? (installs.data ?? []).map((i) => ({ item: asItem(i, library.data), installed: i as Installed | undefined }))
        : (library.data ?? []).map((item) => ({ item, installed: undefined as Installed | undefined })),
    [installedOnly, installs.data, library.data],
  );
  const loaded = library.isSuccess && installs.isSuccess;
  // Installed games: only the stores and sources there are games from.
  const filters: Filter[] = installedOnly
    ? INSTALLED_ORDER.filter((f) => f === "all" || all.some((e) => filterOf(e.item, e.installed) === f))
    : ["all", "gog", "itch"];
  const filter = filters.includes(chosen) ? chosen : "all";
  const items = useMemo(
    () =>
      all.filter(
        (e) => (filter === "all" || filterOf(e.item, e.installed) === filter) && (!term || normalizeName(e.item.title).includes(term)),
      ),
    [all, filter, term],
  );
  const signedIn = !!(accounts.data?.gog || accounts.data?.itch);
  const busy = status?.worker === "library" || status?.worker === "stores";
  const rescan = () =>
    scan.mutate(undefined, {
      onSuccess: (r) => showToast({ tone: r.added > 0 ? "success" : "info", title: tr.found.toastScanned(r.found, r.added) }),
      onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
    });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-end justify-between gap-4 px-8 pt-6 pb-5">
        <div className="min-w-0">
          <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">
            {installedOnly ? tr.views.installed : tr.views.library}
          </h1>
          <p className="mt-1 h-5 text-sm text-ink-400 tabular-nums">
            {loaded ? (installedOnly ? tr.library.installedSubtitle(all.length) : tr.library.subtitle(all.length)) : ""}
          </p>
        </div>
        {(signedIn || installedOnly) && (
          <div className="flex flex-wrap items-center gap-3">
            {filters.length > 1 && (
              <div
                className="flex flex-wrap rounded-lg bg-white/4 p-1 ring-1 ring-white/6"
                role="group"
                aria-label={installedOnly ? tr.found.sourceFilter : tr.library.storeFilter}
              >
                {filters.map((f) => (
                  <button
                    key={f}
                    type="button"
                    onClick={() => setFilter(f)}
                    aria-pressed={filter === f}
                    className={clsx(
                      "inline-flex h-7 items-center gap-1.5 rounded-md px-3 text-[13px] font-medium transition",
                      filter === f ? "bg-ink-600 text-white shadow ring-1 ring-white/10" : "text-ink-300 hover:text-ink-50",
                    )}
                  >
                    {f === "all" ? null : isSource(f) ? <FoundMark source={f} size={15} /> : <StoreMark store={f} size={15} />}
                    {f === "all" ? tr.library.all : isSource(f) ? tr.found.sources[f] : tr.storeNames[f]}
                  </button>
                ))}
              </div>
            )}
            {installedOnly && (
              <button
                type="button"
                onClick={rescan}
                disabled={scanning}
                className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-3.5 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10 disabled:opacity-60"
              >
                {scanning ? <LoaderCircle size={15} className="animate-spin" /> : <RefreshCw size={15} />}
                {scanning ? tr.found.scanning : tr.found.rescan}
              </button>
            )}
            {!installedOnly && (
              <button
                type="button"
                onClick={onRefresh}
                disabled={status?.worker != null}
                className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-3.5 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10 disabled:opacity-60"
              >
                {busy ? <LoaderCircle size={15} className="animate-spin" /> : <RefreshCw size={15} />}
                {tr.library.refresh}
              </button>
            )}
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-10">
        {!loaded || !accounts.isSuccess ? null : installedOnly && all.length === 0 ? (
          <Empty
            icon={
              scanning ? <LoaderCircle size={26} className="animate-spin text-ink-400" /> : <HardDrive size={26} className="text-ink-400" />
            }
            title={scanning ? tr.found.scanning : tr.library.emptyInstalledTitle}
            text={tr.library.emptyInstalled}
            actions={[
              { label: tr.found.addFolder, onClick: onOpenSettings, icon: <FolderPlus size={15} /> },
              { label: tr.views.library, onClick: onOpenLibrary, icon: <Library size={15} /> },
            ]}
          />
        ) : !signedIn && !installedOnly ? (
          <Empty
            icon={<StoreIcon size={26} className="text-ink-400" />}
            title={tr.library.emptyNoAccountTitle}
            text={tr.library.emptyNoAccount}
            actions={[{ label: tr.library.connect, onClick: onOpenSettings, icon: <SettingsIcon size={15} /> }]}
          />
        ) : all.length === 0 ? (
          <Empty
            icon={<StoreIcon size={26} className="text-ink-400" />}
            title={tr.library.emptyTitle}
            text={tr.library.emptyText}
            actions={[{ label: tr.library.refresh, onClick: onRefresh, icon: <RefreshCw size={15} /> }]}
          />
        ) : items.length === 0 ? (
          <Empty icon={<Info size={26} className="text-ink-400" />} title={tr.empty.title} text={tr.library.noResults} />
        ) : (
          <ul
            className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-5"
            aria-label={installedOnly ? tr.views.installed : tr.views.library}
          >
            {items.map(({ item }) => (
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

/** An installed game as a library card, with the owned product's details when there is one. */
function asItem(i: Installed, owned: LibraryItem[] | undefined): LibraryItem {
  const item = owned?.find((o) => o.store === i.store && o.productId === i.productId);
  return (
    item ?? {
      store: i.store,
      productId: i.productId,
      title: i.title,
      url: null,
      cover: null,
      coverWide: null,
      win: false,
      mac: false,
      linux: false,
      appid: i.appid,
      steamHeader: i.steamHeader,
      steamCapsule: null,
    }
  );
}

function isSource(f: Filter): f is FoundSource {
  return f === "steam" || f === "epic" || f === "folder";
}

function LibraryCard({ item, onOpenGame }: { item: LibraryItem; onOpenGame: (appid: number) => void }) {
  const image = item.steamHeader ?? item.coverWide ?? item.cover;
  const { download, live, installing } = useProductDownload(item.store, item.productId);
  const installed = useGameInstall(item.store, item.productId);
  const [matching, setMatching] = useState(false);
  const source = installed?.store === "local" ? installed.source : null;
  // The download's line is for what is still going on; an installed game shows "Oyna".
  const showDownload = download && !(installed && download.state === "completed" && download.installState !== "installing");
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
            {source ? <FoundMark source={source} size={36} /> : <StoreMark store={item.store} size={36} />}
          </div>
        )}
        <span className="absolute top-2 left-2">
          {source ? (
            <FoundPill source={source} className="shadow-md shadow-black/50" />
          ) : (
            <StorePill store={item.store} className="shadow-md shadow-black/50" />
          )}
        </span>
        {installed && (
          <span className="absolute top-2 right-2 inline-flex items-center gap-1 rounded-full bg-ink-950/80 px-2 py-0.5 text-[10.5px] font-semibold text-success ring-1 ring-success/40 backdrop-blur">
            <CircleCheck size={11} />
            {tr.install.installed}
          </span>
        )}
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
          {source && installed ? (
            <span className="truncate" title={installed.dir ?? undefined}>
              {tr.found.via[source]}
              {installed.matchedBy === "title" && ` · ${tr.found.matchedBy.title}`}
            </span>
          ) : installed?.external ? (
            <span className="truncate">{tr.install.external}</span>
          ) : (
            item.appid == null && <span className="truncate">{tr.library.notOnSteam}</span>
          )}
        </div>
        {showDownload && (
          <div className="mt-2.5">
            <DownloadLine download={download} live={live} installing={installing} compact />
          </div>
        )}
        <div className="mt-auto flex items-center gap-1.5 pt-3">
          {installed ? (
            <PlayButton installed={installed} />
          ) : (
            !download && <DownloadButton store={item.store} productId={item.productId} title={item.title} />
          )}
          <span className="ml-auto flex items-center gap-1.5">
            {item.appid != null ? (
              <SmallButton onClick={() => onOpenGame(item.appid!)}>{tr.library.details}</SmallButton>
            ) : (
              installed &&
              source && (
                <SmallButton onClick={() => setMatching(true)} icon={<Link2 size={13} />} title={tr.found.matchLong}>
                  {tr.found.match}
                </SmallButton>
              )
            )}
            {/* Only GOG and itch.io have a store page to open. */}
            {item.url != null && item.store !== "web" && item.store !== "local" && (
              <IconButton label={tr.stores.openIn[item.store]} icon={<ExternalLink size={13} />} onClick={openStore} />
            )}
            {installed && <InstallMenu installed={installed} />}
          </span>
        </div>
      </div>
      {matching && installed && <FoundMatchDialog installed={installed} onClose={() => setMatching(false)} />}
    </div>
  );
}

function Empty({
  icon,
  title,
  text,
  actions = [],
}: {
  icon: ReactNode;
  title: string;
  text: string;
  actions?: { label: string; onClick: () => void; icon?: ReactNode }[];
}) {
  return (
    <div className="grid h-full min-h-[360px] place-items-center">
      <div className="animate-fade-in max-w-md text-center">
        <div className="mx-auto grid size-16 place-items-center rounded-2xl bg-white/4 ring-1 ring-white/8">{icon}</div>
        <h2 className="mt-5 font-display text-xl font-semibold text-ink-50">{title}</h2>
        <p className="mt-2 text-sm text-ink-400">{text}</p>
        {actions.length > 0 && (
          <div className="mt-5 flex flex-wrap justify-center gap-2">
            {actions.map((action) => (
              <button
                key={action.label}
                type="button"
                onClick={action.onClick}
                className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 hover:bg-white/10"
              >
                {action.icon}
                {action.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
