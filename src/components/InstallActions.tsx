// Installed games: "Oyna", the game's menu (its folder, what to start, uninstalling) and the
// state of a finished download's install.

import clsx from "clsx";
import {
  Check,
  EyeOff,
  FileSearch,
  FolderOpen,
  Link2,
  LoaderCircle,
  MoreHorizontal,
  Play,
  RotateCcw,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { type CSSProperties, type ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatPercent } from "../lib/format";
import { showToast } from "../lib/toast";
import type { Download, Installed, InstallProgress, Store } from "../lib/types";
import { installOf, useInstalls } from "../hooks/useData";
import { useModalHost } from "./Feedback";
import { FoundMatchDialog } from "./FoundMatchDialog";
import { IconButton, SmallButton } from "./ui";

/** The installed copy of a store product, if any. */
export function useGameInstall(store: Store, productId: string): Installed | undefined {
  return installOf(useInstalls().data, store, productId);
}

const fail = (e: unknown) => showToast({ tone: "error", title: errorText(toCmdError(e)) });

export function useInstallActions() {
  const qc = useQueryClient();
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["installs"] });
    void qc.invalidateQueries({ queryKey: ["downloads"] });
  };
  return {
    play: (i: Installed) =>
      api
        .launchGame(i.store, i.productId)
        .then(() => showToast({ tone: "info", title: tr.install.toastStarted(i.title) }, 2500))
        .catch(fail),
    openFolder: (i: Installed) => void api.openInstallFolder(i.store, i.productId).catch(fail),
    /** A candidate, or the file picker when `exe` is not given. */
    chooseTarget: (i: Installed, exe?: string) =>
      void (exe ? api.setLaunchTarget(i.store, i.productId, exe) : api.pickLaunchTarget(i.store, i.productId))
        .then((updated) => {
          if (!updated) return;
          qc.setQueryData<Installed[]>(["installs"], (list) =>
            list?.map((x) => (x.store === updated.store && x.productId === updated.productId ? updated : x)),
          );
          showToast({ tone: "success", title: tr.install.toastTarget });
        })
        .catch(fail),
    /** GameLib's own installs are removed; a game Steam installed opens Steam's dialog. */
    uninstall: (i: Installed) =>
      api
        .uninstallGame(i.store, i.productId)
        .then(() => {
          refresh();
          showToast(
            i.store === "local"
              ? { tone: "info", title: tr.found.toastSteamUninstall(i.title) }
              : { tone: "success", title: tr.install.toastUninstalled(i.title) },
          );
        })
        .catch(fail),
    /** Takes a found game off the list (its files stay); the toast can undo it. */
    hide: (i: Installed) =>
      api
        .setFoundHidden(i.productId, true)
        .then(() => {
          refresh();
          void qc.invalidateQueries({ queryKey: ["hidden-found"] });
          showToast({
            tone: "info",
            title: tr.found.toastHidden(i.title),
            action: {
              label: tr.found.unhide,
              onClick: () =>
                void api
                  .setFoundHidden(i.productId, false)
                  .then(() => {
                    refresh();
                    void qc.invalidateQueries({ queryKey: ["hidden-found"] });
                  })
                  .catch(fail),
            },
          });
        })
        .catch(fail),
    approve: (d: Download) => void api.approveInstall(d.id).catch(fail),
    retry: (d: Download) => void api.retryInstall(d.id).catch(fail),
  };
}

/** "Oyna", or choosing what to start when that is not known yet. */
export function PlayButton({ installed }: { installed: Installed }) {
  const act = useInstallActions();
  if (!installed.exe && !installed.launchUrl) {
    return (
      <SmallButton tone="primary" onClick={() => act.chooseTarget(installed)} icon={<FileSearch size={13} />}>
        {tr.install.chooseTarget}
      </SmallButton>
    );
  }
  return (
    <SmallButton tone="primary" onClick={() => void act.play(installed)} icon={<Play size={13} />}>
      {tr.install.play}
    </SmallButton>
  );
}

/** How tall the menu can get; it opens the other way when there is less room. */
const MENU_ROOM = 320;

/** The game's menu: its folder, what to start, uninstalling (after a confirmation). Drawn above
 *  everything (in the open dialog, if any), so cards and scroll areas never cut it off. */
export function InstallMenu({ installed, placement = "up" }: { installed: Installed; placement?: "up" | "down" }) {
  const act = useInstallActions();
  const host = useModalHost();
  const [position, setPosition] = useState<CSSProperties | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [matching, setMatching] = useState(false);
  const found = installed.store === "local";
  // Steam and Epic games start through their launcher; only folder games start a program.
  const ownProgram = !installed.launchUrl;
  const anchor = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const open = position != null;

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!anchor.current?.contains(target) && !menu.current?.contains(target)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Keep the game details open.
        e.stopPropagation();
        e.preventDefault();
        close();
      }
    };
    const onScroll = (e: Event) => {
      if (!menu.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  function show() {
    const r = anchor.current?.getBoundingClientRect();
    if (!r) return;
    const above = r.top;
    const below = window.innerHeight - r.bottom;
    const up = placement === "up" ? above >= MENU_ROOM || above > below : below < MENU_ROOM && above > below;
    const right = Math.max(8, window.innerWidth - r.right);
    setPosition(up ? { bottom: window.innerHeight - r.top + 6, right } : { top: r.bottom + 6, right });
  }

  function close() {
    setPosition(null);
    setConfirm(false);
  }

  const uninstall = () => {
    setBusy(true);
    void act.uninstall(installed).finally(() => {
      setBusy(false);
      close();
    });
  };

  return (
    <div ref={anchor} className="relative">
      <IconButton label={tr.install.more} icon={<MoreHorizontal size={14} />} onClick={() => (open ? close() : show())} />
      {position &&
        createPortal(
          <div
            ref={menu}
            role="menu"
            aria-label={tr.install.more}
            style={position}
            className="animate-fade-in fixed z-[70] max-h-[70vh] w-72 overflow-y-auto rounded-xl bg-ink-750 p-1.5 text-left shadow-2xl shadow-black/60 ring-1 ring-white/10"
          >
            {confirm ? (
              <div className="p-2 text-[12.5px]">
                <div className="font-medium text-ink-50">{tr.install.confirmUninstall}</div>
                <p className="mt-1 leading-relaxed text-ink-400">{tr.install.uninstallHint(installed.method)}</p>
                <div className="mt-3 flex justify-end gap-1.5">
                  <SmallButton onClick={() => setConfirm(false)} disabled={busy}>
                    {tr.install.cancel}
                  </SmallButton>
                  <SmallButton
                    tone="danger"
                    onClick={uninstall}
                    disabled={busy}
                    icon={busy ? <LoaderCircle size={13} className="animate-spin" /> : <Trash2 size={13} />}
                  >
                    {busy ? tr.install.uninstalling : tr.install.yesUninstall}
                  </SmallButton>
                </div>
              </div>
            ) : (
              <>
                {installed.dir && (
                  <MenuItem icon={<FolderOpen size={14} />} onClick={() => (close(), act.openFolder(installed))}>
                    {tr.install.openFolder}
                  </MenuItem>
                )}
                {found && installed.source !== "steam" && (
                  <MenuItem icon={<Link2 size={14} />} onClick={() => (close(), setMatching(true))}>
                    {installed.appid != null ? tr.found.changeMatch : tr.found.matchLong}
                  </MenuItem>
                )}
                {ownProgram && (
                  <>
                    {(installed.dir || found) && <div className="my-1 h-px bg-white/8" />}
                    <div className="px-2.5 pt-1.5 pb-1 text-[11px] font-semibold tracking-wider text-ink-500 uppercase">
                      {tr.install.target}
                    </div>
                    {installed.candidates.map((path) => (
                      <MenuItem
                        key={path}
                        icon={path === installed.exe ? <Check size={14} className="text-success" /> : <span className="size-3.5" />}
                        onClick={() => (close(), act.chooseTarget(installed, path))}
                        title={path}
                      >
                        <span className="truncate">{relative(path, installed.dir)}</span>
                      </MenuItem>
                    ))}
                    {installed.exe && !installed.candidates.includes(installed.exe) && (
                      <MenuItem icon={<Check size={14} className="text-success" />} onClick={close} title={installed.exe}>
                        <span className="truncate">{relative(installed.exe, installed.dir)}</span>
                      </MenuItem>
                    )}
                    <MenuItem icon={<FileSearch size={14} />} onClick={() => (close(), act.chooseTarget(installed))}>
                      {tr.install.otherFile}
                    </MenuItem>
                  </>
                )}
                <div className="my-1 h-px bg-white/8" />
                {!found ? (
                  <MenuItem icon={<Trash2 size={14} />} danger onClick={() => setConfirm(true)}>
                    {tr.install.uninstall}
                  </MenuItem>
                ) : (
                  <>
                    {installed.source === "steam" && installed.appid != null && (
                      <MenuItem icon={<Trash2 size={14} />} danger onClick={() => (close(), void act.uninstall(installed))}>
                        {tr.found.steamUninstall}
                      </MenuItem>
                    )}
                    <MenuItem icon={<EyeOff size={14} />} onClick={() => (close(), void act.hide(installed))} title={tr.found.hideHint}>
                      {tr.found.hide}
                    </MenuItem>
                    {installed.source === "epic" && (
                      <p className="px-2.5 pt-1 pb-1.5 text-[11.5px] leading-relaxed text-ink-500">{tr.found.epicHint}</p>
                    )}
                  </>
                )}
              </>
            )}
          </div>,
          host,
        )}
      {matching && <FoundMatchDialog installed={installed} onClose={() => setMatching(false)} />}
    </div>
  );
}

function MenuItem({
  children,
  icon,
  onClick,
  danger,
  title,
}: {
  children: ReactNode;
  icon: ReactNode;
  onClick: () => void;
  danger?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      title={title}
      className={clsx(
        "flex h-8 w-full min-w-0 items-center gap-2.5 rounded-lg px-2.5 text-[13px] transition",
        danger ? "text-danger hover:bg-danger/12" : "text-ink-200 hover:bg-white/6 hover:text-white",
      )}
    >
      <span className="grid size-4 shrink-0 place-items-center">{icon}</span>
      <span className="flex min-w-0 flex-1">{children}</span>
    </button>
  );
}

/** A path shown relative to the install folder. */
function relative(path: string, dir: string | null): string {
  if (dir && path.startsWith(dir)) return path.slice(dir.length).replace(/^[\\/]+/, "") || path;
  return path;
}

/** The install of a finished download: its progress, the question for someone else's installer,
 *  or what the user can do. Nothing once installed (the caller shows "Oyna"). */
export function InstallLine({
  download,
  progress,
  compact,
}: {
  download: Download;
  progress: InstallProgress | null;
  /** Short texts, for narrow cards. */
  compact?: boolean;
}) {
  const act = useInstallActions();
  const openFolder = (
    <IconButton
      label={tr.downloads.openFolder}
      icon={<FolderOpen size={13} />}
      onClick={() => void api.openDownloadFolder(download.id).catch(fail)}
    />
  );

  switch (download.installState) {
    case "installed":
      return null;
    case "installing": {
      const pct = progress && progress.total > 0 ? Math.min(100, (progress.done / progress.total) * 100) : null;
      return (
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[12.5px] font-medium text-accent-soft">
            <LoaderCircle size={13} className="shrink-0 animate-spin" />
            <span className="truncate">{progress ? tr.install.stages[progress.stage] : tr.install.installing}</span>
            {pct != null && <span className="text-ink-300 tabular-nums">{formatPercent(pct)}</span>}
          </div>
          <Bar pct={pct} />
        </div>
      );
    }
    case "waiting":
    case "approved":
      return <Status tone="text-ink-300" icon={<LoaderCircle size={13} className="shrink-0 animate-spin" />} label={tr.install.waiting} />;
    case "confirm":
      return (
        <div className="min-w-0">
          <Status
            tone="text-warning"
            icon={<ShieldAlert size={13} className="shrink-0" />}
            label={tr.install.confirm}
            actions={openFolder}
          />
          <p className="mt-1 text-[11.5px] leading-relaxed text-ink-400" title={compact ? tr.install.confirmText : undefined}>
            {compact ? tr.install.confirmShort : tr.install.confirmText}
          </p>
          <div className="mt-2">
            <SmallButton tone="primary" onClick={() => act.approve(download)} icon={<Play size={13} />}>
              {tr.install.approve}
            </SmallButton>
          </div>
        </div>
      );
    case "failed":
      return (
        <div className="min-w-0">
          <Status
            tone="text-danger"
            icon={<RotateCcw size={13} className="shrink-0" />}
            label={tr.install.failed}
            actions={
              <>
                <IconButton label={tr.install.retry} icon={<RotateCcw size={13} />} onClick={() => act.retry(download)} />
                {openFolder}
              </>
            }
          />
          {download.installError && (
            <div className="mt-1 truncate text-[11.5px] text-ink-400" title={errorText(download.installError)}>
              {errorText(download.installError)}
            </div>
          )}
        </div>
      );
    case "manual":
      return (
        <div className="min-w-0">
          <Status tone="text-ink-200" icon={<FolderOpen size={13} className="shrink-0" />} label={tr.install.manual} actions={openFolder} />
          <div className="mt-1 text-[11.5px] leading-relaxed text-ink-400">{tr.install.manualHint(download.installKind)}</div>
        </div>
      );
    default:
      // Downloaded before installs existed.
      return (
        <Status
          tone="text-success"
          icon={<Check size={13} className="shrink-0" />}
          label={tr.install.notInstalled}
          actions={
            <>
              <SmallButton tone="primary" onClick={() => act.retry(download)}>
                {tr.install.install}
              </SmallButton>
              {openFolder}
            </>
          }
        />
      );
  }
}

function Status({ tone, icon, label, actions }: { tone: string; icon: ReactNode; label: string; actions?: ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span className={clsx("inline-flex min-w-0 items-center gap-1.5 text-[12.5px] font-medium", tone)}>
        {icon}
        <span className="truncate">{label}</span>
      </span>
      {actions && <span className="ml-auto flex shrink-0 items-center gap-1">{actions}</span>}
    </div>
  );
}

/** A progress bar; without a percentage it runs back and forth (an installer gives none). */
function Bar({ pct }: { pct: number | null }) {
  return (
    <div className="relative mt-1.5 h-1.5 overflow-hidden rounded-full bg-ink-700">
      {pct == null ? (
        <div className="animate-progress absolute inset-y-0 w-1/3 rounded-full bg-gradient-to-r from-transparent via-accent to-transparent" />
      ) : (
        <div
          className="h-full rounded-full bg-gradient-to-r from-accent to-violet transition-[width] duration-500"
          style={{ width: `${pct}%` }}
        />
      )}
    </div>
  );
}
