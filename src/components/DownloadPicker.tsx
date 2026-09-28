// Downloading a store product: the "İndir" button with its file picker, and a compact line
// showing a download's progress with its controls.

import clsx from "clsx";
import { Download as DownloadIcon, FolderOpen, LoaderCircle, Pause, Play, RotateCcw, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatBytes, formatDuration, formatPercent } from "../lib/format";
import { showToast } from "../lib/toast";
import type { CmdError, Download, DownloadProgress, FileOption, Platform, Store } from "../lib/types";
import { downloadOf, useDownloads, useEnqueueDownload, useSettings, useStoreFiles } from "../hooks/useData";
import { StoreMark } from "./badges";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";
import { SmallButton } from "./ui";

/** The latest download of a product, with live progress while it runs. */
export function useProductDownload(store: Store, productId: string) {
  const list = useDownloads();
  const download = downloadOf(list.data, store, productId);
  const live = download && list.data?.live?.id === download.id ? list.data.live : null;
  return { download, live };
}

/** Commands on a download; their results arrive as `download:state` events. */
export function useDownloadActions() {
  const run = (p: Promise<unknown>) => void p.catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
  return {
    pause: (d: Download) => run(api.pauseDownload(d.id)),
    resume: (d: Download) => run(api.resumeDownload(d.id)),
    remove: (d: Download) => run(api.removeDownload(d.id)),
    openFolder: (d: Download) => run(api.openDownloadFolder(d.id)),
  };
}

export function DownloadButton({ store, productId, title }: { store: Store; productId: string; title: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <SmallButton tone="primary" onClick={() => setOpen(true)} icon={<DownloadIcon size={13} />}>
        {tr.downloads.download}
      </SmallButton>
      {open && <DownloadPicker store={store} productId={productId} title={title} onClose={() => setOpen(false)} />}
    </>
  );
}

/** Lists what the store offers (the best variant for this computer preselected) and queues the
 *  chosen one. A modal of its own, so it also works on top of the game details. */
function DownloadPicker({ store, productId, title, onClose }: { store: Store; productId: string; title: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const files = useStoreFiles(store, productId, true);
  const settings = useSettings();
  const enqueue = useEnqueueDownload();
  const [choice, setChoice] = useState<string | null>(null);
  const [error, setError] = useState<CmdError | null>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const options = files.data ?? [];
  const selected = options.find((o) => o.id === choice) ?? options.find((o) => o.recommended) ?? options[0];
  const here = options.find((o) => o.recommended)?.platform ?? null;
  const close = () => ref.current?.close();

  const start = () => {
    if (!selected) return;
    setError(null);
    enqueue.mutate(
      { store, productId, optionId: selected.id },
      {
        onSuccess: (d) => {
          showToast({ tone: "success", title: tr.downloads.toastQueued(d.title) });
          close();
        },
        onError: (e) => setError(toCmdError(e)),
      },
    );
  };

  return (
    <dialog
      ref={ref}
      // Close and cancel events would otherwise reach the game details dialog around this one.
      onClose={(e) => {
        e.stopPropagation();
        onClose();
      }}
      onCancel={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        if (e.target === ref.current) close();
      }}
      className="m-auto w-[min(580px,94vw)] max-w-none rounded-2xl bg-ink-850 p-0 text-ink-100 shadow-2xl shadow-black ring-1 ring-white/10 open:animate-rise"
      aria-label={tr.downloads.pickerTitle}
    >
      <div className="p-6">
        <div className="flex items-start gap-3">
          <StoreMark store={store} size={26} className="mt-0.5" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm text-ink-400" title={title}>
              {title}
            </div>
            <h2 className="font-display text-lg font-semibold text-ink-50">{tr.downloads.pickerTitle}</h2>
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

        <div className="mt-5 max-h-[46vh] space-y-2 overflow-y-auto pr-1">
          {files.isLoading ? (
            <div className="flex items-center gap-2 py-6 text-sm text-ink-400">
              <LoaderCircle size={15} className="animate-spin" />
              {tr.downloads.loadingFiles}
            </div>
          ) : files.isError ? (
            <p className="rounded-xl bg-danger/10 px-4 py-3 text-sm text-danger ring-1 ring-danger/25">
              {errorText(toCmdError(files.error))}
            </p>
          ) : options.length === 0 ? (
            <p className="py-6 text-sm text-ink-400">{tr.downloads.noFiles}</p>
          ) : (
            <div role="radiogroup" aria-label={tr.downloads.pickerTitle} className="space-y-2">
              {options.map((o) => (
                <OptionRow
                  key={o.id}
                  option={o}
                  checked={o.id === selected?.id}
                  foreign={o.platform != null && here != null && o.platform !== here}
                  onPick={() => setChoice(o.id)}
                />
              ))}
            </div>
          )}
        </div>

        {settings.data && <p className="mt-4 text-xs leading-relaxed text-ink-400">{tr.downloads.pickerHint(settings.data.libraryDir)}</p>}
        {error && (
          <p className="mt-3 rounded-lg bg-danger/10 px-3 py-2 text-[13px] text-danger ring-1 ring-danger/25">{errorText(error)}</p>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            className="h-9 rounded-lg px-4 text-sm font-medium text-ink-300 ring-1 ring-white/10 hover:bg-white/6 hover:text-white"
          >
            {tr.downloads.cancel}
          </button>
          <button
            type="button"
            onClick={start}
            disabled={!selected || enqueue.isPending}
            className="inline-flex h-9 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-semibold text-ink-950 transition hover:bg-accent-soft disabled:opacity-50"
          >
            {enqueue.isPending ? <LoaderCircle size={15} className="animate-spin" /> : <DownloadIcon size={15} />}
            {selected && selected.size > 0 ? tr.downloads.startSize(formatBytes(selected.size)) : tr.downloads.start}
          </button>
        </div>
      </div>
    </dialog>
  );
}

function OptionRow({ option, checked, foreign, onPick }: { option: FileOption; checked: boolean; foreign: boolean; onPick: () => void }) {
  const details = [
    option.size > 0 ? formatBytes(option.size) : tr.downloads.unknownSize,
    option.files > 1 ? tr.downloads.files(option.files) : null,
    foreign ? tr.downloads.otherPlatform : null,
  ].filter(Boolean);
  return (
    <label
      className={clsx(
        "flex cursor-pointer items-center gap-3 rounded-xl px-3.5 py-3 ring-1 transition",
        checked ? "bg-accent/10 ring-accent/45" : "bg-white/3 ring-white/8 hover:ring-white/16",
      )}
    >
      <input type="radio" name="download-option" className="sr-only" checked={checked} onChange={onPick} />
      <span
        className={clsx("grid size-4 shrink-0 place-items-center rounded-full ring-1", checked ? "bg-accent ring-accent" : "ring-white/25")}
      >
        {checked && <span className="size-1.5 rounded-full bg-white" />}
      </span>
      <PlatformGlyph platform={option.platform} />
      <span className="min-w-0 flex-1">
        <span className={clsx("block truncate text-sm font-medium", foreign ? "text-ink-300" : "text-ink-50")} title={option.label}>
          {option.label}
        </span>
        <span className="mt-0.5 block text-xs text-ink-400 tabular-nums">{details.join(" · ")}</span>
      </span>
      {option.demo && <Pill className="bg-white/8 text-ink-300 ring-white/10">{tr.downloads.demo}</Pill>}
      {option.recommended && <Pill className="bg-success/12 text-success ring-success/30">{tr.downloads.recommended}</Pill>}
    </label>
  );
}

function PlatformGlyph({ platform }: { platform: Platform | null }) {
  const Icon = platform === "win" ? WindowsIcon : platform === "mac" ? AppleIcon : platform === "linux" ? LinuxIcon : null;
  return <span className="grid size-5 shrink-0 place-items-center text-ink-300">{Icon ? <Icon size={14} /> : null}</span>;
}

function Pill({ children, className }: { children: ReactNode; className: string }) {
  return <span className={clsx("shrink-0 rounded-full px-2 py-0.5 text-[11px] font-semibold ring-1", className)}>{children}</span>;
}

/** Bytes done and total, speed and time left, as one line. */
export function progressText(d: Download, live: DownloadProgress | null): string {
  const done = live?.doneBytes ?? d.doneBytes;
  const total = Math.max(d.totalBytes, live?.totalBytes ?? 0);
  const parts = [total > 0 ? tr.downloads.progress(formatBytes(done), formatBytes(total)) : formatBytes(done)];
  if (d.state === "downloading" && live && live.stage === "downloading" && live.speed > 0) {
    parts.push(tr.downloads.speed(formatBytes(live.speed)));
    if (live.eta != null) parts.push(tr.downloads.eta(formatDuration(live.eta)));
  }
  return parts.join(" · ");
}

export function percentOf(d: Download, live: DownloadProgress | null): number {
  const done = live?.doneBytes ?? d.doneBytes;
  const total = Math.max(d.totalBytes, live?.totalBytes ?? 0);
  if (d.state === "completed") return 100;
  return total > 0 ? Math.min(100, (done / total) * 100) : 0;
}

/** A download's state, progress and controls in a small space (store sections, library cards). */
export function DownloadLine({ download, live }: { download: Download; live: DownloadProgress | null }) {
  const act = useDownloadActions();
  const [confirm, setConfirm] = useState(false);
  const pct = percentOf(download, live);
  const verifying = download.state === "downloading" && live?.stage === "verifying";
  const label = verifying ? tr.downloads.verifying : tr.downloads.states[download.state];
  const tone =
    download.state === "failed"
      ? "text-danger"
      : download.state === "completed"
        ? "text-success"
        : download.state === "downloading"
          ? "text-accent-soft"
          : "text-ink-300";

  if (confirm) {
    return (
      <div className="flex flex-wrap items-center justify-between gap-2 text-[12.5px]">
        <span className="text-ink-200">{tr.downloads.confirmCancel}</span>
        <span className="flex gap-1.5">
          <SmallButton tone="danger" onClick={() => (setConfirm(false), act.remove(download))}>
            {tr.downloads.yesDelete}
          </SmallButton>
          <SmallButton onClick={() => setConfirm(false)}>{tr.downloads.no}</SmallButton>
        </span>
      </div>
    );
  }

  return (
    <div className="min-w-0">
      <div className="flex items-center gap-2">
        <span className={clsx("inline-flex min-w-0 items-center gap-1.5 text-[12.5px] font-medium", tone)}>
          {download.state === "downloading" ? (
            <LoaderCircle size={13} className="shrink-0 animate-spin" />
          ) : (
            <DownloadIcon size={13} className="shrink-0" />
          )}
          <span className="truncate">{label}</span>
          {download.state !== "completed" && download.state !== "queued" && (
            <span className="text-ink-300 tabular-nums">{formatPercent(pct)}</span>
          )}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-1">
          {download.state === "downloading" && (
            <IconButton label={tr.downloads.pause} icon={<Pause size={13} />} onClick={() => act.pause(download)} />
          )}
          {download.state === "paused" && (
            <IconButton label={tr.downloads.resume} icon={<Play size={13} />} onClick={() => act.resume(download)} />
          )}
          {download.state === "failed" && (
            <IconButton label={tr.downloads.retry} icon={<RotateCcw size={13} />} onClick={() => act.resume(download)} />
          )}
          {download.state === "completed" ? (
            <IconButton label={tr.downloads.openFolder} icon={<FolderOpen size={13} />} onClick={() => act.openFolder(download)} />
          ) : (
            <IconButton label={tr.downloads.cancelDownload} icon={<X size={13} />} onClick={() => setConfirm(true)} />
          )}
        </span>
      </div>
      {download.state !== "completed" && (
        <>
          <ProgressBar pct={pct} state={download.state} />
          <div
            className="mt-1 truncate text-[11.5px] text-ink-400 tabular-nums"
            title={download.error ? errorText(download.error) : undefined}
          >
            {download.state === "failed" && download.error ? errorText(download.error) : progressText(download, live)}
          </div>
        </>
      )}
    </div>
  );
}

export function ProgressBar({ pct, state, className }: { pct: number; state: Download["state"]; className?: string }) {
  return (
    <div
      className={clsx("mt-1.5 h-1.5 overflow-hidden rounded-full bg-ink-700", className)}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
    >
      <div
        className={clsx(
          "h-full rounded-full transition-[width] duration-500",
          state === "failed" ? "bg-danger/70" : state === "downloading" ? "bg-gradient-to-r from-accent to-violet" : "bg-ink-400",
        )}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function IconButton({
  label,
  icon,
  onClick,
  tone = "default",
}: {
  label: string;
  icon: ReactNode;
  onClick: () => void;
  tone?: "default" | "danger";
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={clsx(
        "grid size-7 place-items-center rounded-md ring-1 transition",
        tone === "danger"
          ? "text-danger ring-danger/30 hover:bg-danger/15"
          : "bg-white/4 text-ink-200 ring-white/8 hover:bg-white/10 hover:text-white",
      )}
    >
      {icon}
    </button>
  );
}
