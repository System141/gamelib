// "İndirmeler": the running download, the queue and finished downloads.

import clsx from "clsx";
import { Download as DownloadIcon, FolderOpen, Info, Library, Pause, Play, RotateCcw, Trash2, X } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { normalizeName } from "../lib/fold";
import { formatBytes, formatPercent, formatRelative } from "../lib/format";
import { showToast } from "../lib/toast";
import type { Download, DownloadProgress, LibraryItem } from "../lib/types";
import { useDownloads, useLibrary, useSettings } from "../hooks/useData";
import { StoreMark, StorePill } from "./badges";
import { IconButton, ProgressBar, percentOf, progressText, useDownloadActions } from "./DownloadPicker";
import { SmallButton } from "./ui";

interface Props {
  search: string;
  onOpenGame: (appid: number) => void;
  onOpenLibrary: () => void;
}

export function DownloadsView({ search, onOpenGame, onOpenLibrary }: Props) {
  const downloads = useDownloads();
  const library = useLibrary();
  const settings = useSettings();
  const term = normalizeName(search.trim());
  const all = downloads.data?.items ?? [];
  const live = downloads.data?.live ?? null;
  const items = useMemo(() => all.filter((d) => !term || normalizeName(d.title).includes(term)), [all, term]);
  const active = items.filter((d) => d.state === "downloading");
  const waiting = items.filter((d) => d.state === "queued" || d.state === "paused" || d.state === "failed");
  const finished = items.filter((d) => d.state === "completed");
  const art = useMemo(() => {
    const byKey = new Map<string, LibraryItem>();
    for (const i of library.data ?? []) byKey.set(`${i.store}:${i.productId}`, i);
    return (d: Download) => {
      const i = byKey.get(`${d.store}:${d.productId}`);
      return i ? (i.steamHeader ?? i.coverWide ?? i.cover) : null;
    };
  }, [library.data]);
  const running = all.filter((d) => d.state === "downloading").length;
  const done = all.filter((d) => d.state === "completed").length;

  const clear = () => api.clearFinishedDownloads().catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-end justify-between gap-4 px-8 pt-6 pb-5">
        <div className="min-w-0">
          <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">{tr.downloads.title}</h1>
          <p className="mt-1 h-5 truncate text-sm text-ink-400 tabular-nums">
            {downloads.isSuccess && all.length > 0 ? tr.downloads.subtitle(running, all.length - running - done, done) : ""}
            {settings.data && all.length > 0 && <span className="text-ink-500"> · {tr.downloads.location(settings.data.libraryDir)}</span>}
          </p>
        </div>
        {done > 0 && (
          <button
            type="button"
            onClick={clear}
            title={tr.downloads.clearFinishedHint}
            className="inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-3.5 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10"
          >
            <Trash2 size={15} />
            {tr.downloads.clearFinished}
          </button>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-10">
        {!downloads.isSuccess ? null : all.length === 0 ? (
          <Empty onOpenLibrary={onOpenLibrary} />
        ) : items.length === 0 ? (
          <div className="grid min-h-[300px] place-items-center text-sm text-ink-400">
            <span className="inline-flex items-center gap-2">
              <Info size={16} />
              {tr.downloads.noResults}
            </span>
          </div>
        ) : (
          <div className="space-y-8">
            {active.length > 0 && (
              <Section title={tr.downloads.active}>
                {active.map((d) => (
                  <ActiveCard key={d.id} download={d} live={live?.id === d.id ? live : null} image={art(d)} onOpenGame={onOpenGame} />
                ))}
              </Section>
            )}
            {waiting.length > 0 && (
              <Section title={tr.downloads.queue}>
                {waiting.map((d) => (
                  <Row key={d.id} download={d} image={art(d)} onOpenGame={onOpenGame} />
                ))}
              </Section>
            )}
            {finished.length > 0 && (
              <Section title={tr.downloads.finished}>
                {finished.map((d) => (
                  <Row key={d.id} download={d} image={art(d)} onOpenGame={onOpenGame} />
                ))}
              </Section>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h2 className="mb-3 text-[11px] font-semibold tracking-wider text-ink-500 uppercase">{title}</h2>
      <div className="space-y-2.5">{children}</div>
    </section>
  );
}

function Art({ download, image, className }: { download: Download; image: string | null; className: string }) {
  return (
    <div className={clsx("relative shrink-0 overflow-hidden rounded-lg bg-ink-700 ring-1 ring-white/8", className)}>
      {image ? (
        <img src={image} alt="" loading="lazy" decoding="async" className="size-full object-cover" />
      ) : (
        <div className="grid size-full place-items-center">
          <StoreMark store={download.store} size={28} />
        </div>
      )}
    </div>
  );
}

function Title({ download, onOpenGame }: { download: Download; onOpenGame: (appid: number) => void }) {
  const text = (
    <span className="truncate font-medium text-ink-50" title={download.title}>
      {download.title}
    </span>
  );
  return (
    <div className="flex min-w-0 items-center gap-2">
      {download.appid != null ? (
        <button type="button" onClick={() => onOpenGame(download.appid!)} className="min-w-0 truncate text-left hover:underline">
          {text}
        </button>
      ) : (
        text
      )}
      <StorePill store={download.store} className="shrink-0" />
    </div>
  );
}

function ActiveCard({
  download,
  live,
  image,
  onOpenGame,
}: {
  download: Download;
  live: DownloadProgress | null;
  image: string | null;
  onOpenGame: (appid: number) => void;
}) {
  const act = useDownloadActions();
  const [confirm, setConfirm] = useState(false);
  const pct = percentOf(download, live);
  const verifying = live?.stage === "verifying";
  return (
    <div className="rounded-2xl bg-ink-800/80 p-4 shadow-lg shadow-black/30 ring-1 ring-accent/20">
      <div className="flex items-center gap-4">
        <Art download={download} image={image} className="aspect-[460/215] w-44" />
        <div className="min-w-0 flex-1">
          <Title download={download} onOpenGame={onOpenGame} />
          <div className="mt-0.5 truncate text-xs text-ink-400">{download.optionLabel}</div>
          <div className="mt-3 flex items-baseline justify-between gap-3">
            <span className="text-sm font-medium text-accent-soft">
              {verifying ? tr.downloads.verifying : tr.downloads.states.downloading}
            </span>
            <span className="font-display text-lg font-semibold text-ink-50 tabular-nums">{formatPercent(pct)}</span>
          </div>
          <ProgressBar pct={pct} state="downloading" className="h-2" />
          <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
            <span className="text-[13px] text-ink-300 tabular-nums">{progressText(download, live)}</span>
            {confirm ? (
              <span className="flex items-center gap-1.5 text-[12.5px] text-ink-200">
                {tr.downloads.confirmCancel}
                <SmallButton tone="danger" onClick={() => (setConfirm(false), act.remove(download))}>
                  {tr.downloads.yesDelete}
                </SmallButton>
                <SmallButton onClick={() => setConfirm(false)}>{tr.downloads.no}</SmallButton>
              </span>
            ) : (
              <span className="flex gap-1.5">
                <SmallButton onClick={() => act.pause(download)} icon={<Pause size={13} />}>
                  {tr.downloads.pause}
                </SmallButton>
                <SmallButton onClick={() => setConfirm(true)} icon={<X size={13} />}>
                  {tr.downloads.cancelDownload}
                </SmallButton>
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Row({ download, image, onOpenGame }: { download: Download; image: string | null; onOpenGame: (appid: number) => void }) {
  const act = useDownloadActions();
  const [confirm, setConfirm] = useState(false);
  const pct = percentOf(download, null);
  const completed = download.state === "completed";
  const detail =
    download.state === "failed" && download.error
      ? errorText(download.error)
      : completed
        ? [formatBytes(download.totalBytes), download.finishedAt ? tr.downloads.finishedAt(formatRelative(download.finishedAt)) : null]
            .filter(Boolean)
            .join(" · ")
        : progressText(download, null);

  return (
    <div className="rounded-xl bg-ink-800/60 p-3 ring-1 ring-white/6">
      <div className="flex items-center gap-3.5">
        <Art download={download} image={image} className="aspect-[460/215] w-24" />
        <div className="min-w-0 flex-1">
          <Title download={download} onOpenGame={onOpenGame} />
          <div className="mt-1 flex min-w-0 items-center gap-2 text-xs">
            <span
              className={clsx(
                "shrink-0 font-medium",
                download.state === "failed" ? "text-danger" : completed ? "text-success" : "text-ink-200",
              )}
            >
              {tr.downloads.states[download.state]}
              {download.state === "paused" && ` · ${formatPercent(pct)}`}
            </span>
            <span className="text-ink-500">·</span>
            <span className="truncate text-ink-400 tabular-nums" title={detail}>
              {download.optionLabel ? `${download.optionLabel} · ${detail}` : detail}
            </span>
          </div>
          {download.state === "paused" && <ProgressBar pct={pct} state="paused" className="max-w-md" />}
        </div>
        {confirm ? (
          <span className="flex shrink-0 items-center gap-1.5 text-[12.5px] text-ink-200">
            {completed ? tr.downloads.confirmDelete : tr.downloads.confirmCancel}
            <SmallButton tone="danger" onClick={() => (setConfirm(false), act.remove(download))}>
              {tr.downloads.yesDelete}
            </SmallButton>
            <SmallButton onClick={() => setConfirm(false)}>{tr.downloads.no}</SmallButton>
          </span>
        ) : (
          <span className="flex shrink-0 items-center gap-1.5">
            {download.state === "paused" && (
              <SmallButton tone="primary" onClick={() => act.resume(download)} icon={<Play size={13} />}>
                {tr.downloads.resume}
              </SmallButton>
            )}
            {download.state === "failed" && (
              <SmallButton tone="primary" onClick={() => act.resume(download)} icon={<RotateCcw size={13} />}>
                {tr.downloads.retry}
              </SmallButton>
            )}
            {download.state === "queued" && (
              <SmallButton onClick={() => act.pause(download)} icon={<Pause size={13} />}>
                {tr.downloads.pause}
              </SmallButton>
            )}
            {completed && (
              <SmallButton tone="primary" onClick={() => act.openFolder(download)} icon={<FolderOpen size={13} />}>
                {tr.downloads.openFolder}
              </SmallButton>
            )}
            <IconButton
              label={completed ? tr.downloads.deleteFiles : tr.downloads.cancelDownload}
              icon={completed ? <Trash2 size={13} /> : <X size={13} />}
              onClick={() => setConfirm(true)}
            />
          </span>
        )}
      </div>
    </div>
  );
}

function Empty({ onOpenLibrary }: { onOpenLibrary: () => void }) {
  return (
    <div className="grid h-full min-h-[360px] place-items-center">
      <div className="animate-fade-in max-w-md text-center">
        <div className="mx-auto grid size-16 place-items-center rounded-2xl bg-white/4 ring-1 ring-white/8">
          <DownloadIcon size={26} className="text-ink-400" />
        </div>
        <h2 className="mt-5 font-display text-xl font-semibold text-ink-50">{tr.downloads.emptyTitle}</h2>
        <p className="mt-2 text-sm text-ink-400">{tr.downloads.emptyText}</p>
        <button
          type="button"
          onClick={onOpenLibrary}
          className="mt-5 inline-flex h-9 items-center gap-2 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 hover:bg-white/10"
        >
          <Library size={15} />
          {tr.downloads.goLibrary}
        </button>
      </div>
    </div>
  );
}
