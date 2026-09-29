// "Steam dışı bağlantılar": user-added links to other stores, official sites or downloads.

import clsx from "clsx";
import {
  CircleCheck,
  CircleX,
  Download,
  ExternalLink,
  Globe,
  Link2,
  LoaderCircle,
  Pencil,
  Plus,
  Radar,
  Search,
  ShieldAlert,
  Trash,
} from "lucide-react";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { fileKind, formatBytes, formatRelative, isRecent } from "../lib/format";
import { showToast } from "../lib/toast";
import type { FoundLink, GameLink, LinkInput, LinkKind, Platform, SiteInfo } from "../lib/types";
import { useCheckLink, useDeleteLink, useEnqueueTorrent, useFindLinks, useLinks, useSaveLink, useSites } from "../hooks/useData";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";
import { SmallButton } from "./ui";

/** A check older than this is refreshed automatically when the dialog opens. */
const STALE_DAYS = 7;

/** Torrent links are not pages: they are handed to the download queue instead of a browser. */
function isMagnet(url: string): boolean {
  return url.trim().toLowerCase().startsWith("magnet:");
}

export function LinksSection({ appid, gameTitle }: { appid: number; gameTitle: string }) {
  const links = useLinks(appid);
  const sites = useSites();
  const find = useFindLinks();
  const [editing, setEditing] = useState<GameLink | "new" | null>(null);
  const [found, setFound] = useState<FoundLink[] | null>(null);
  const siteById = useMemo(() => new Map((sites.data ?? []).map((s) => [s.id, s])), [sites.data]);
  const list = links.data ?? [];
  const { run, progress } = useBulkCheck();
  const autoRan = useRef(false);
  // A magnet link has no page to follow, so it is never checked.
  const checkable = list.filter((l) => !isMagnet(l.url));
  const stale = checkable.filter((l) => !l.lastCheck || !isRecent(l.lastCheck.checkedAt, STALE_DAYS));

  const search = () =>
    find.mutate(appid, {
      onSuccess: (results) => setFound(results),
      onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
    });

  // Re-check everything older than the window, once per dialog open; `autoRan` resets on unmount.
  useEffect(() => {
    if (autoRan.current || !links.isSuccess || stale.length === 0) return;
    autoRan.current = true;
    void run(stale);
  }, [links.isSuccess, stale.length]);

  return (
    <section>
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 font-display text-lg font-semibold text-ink-50">
          <Link2 size={18} className="text-accent" />
          {tr.links.title}
          {list.length > 0 && <span className="rounded-full bg-white/8 px-2 text-xs font-medium text-ink-300">{list.length}</span>}
        </h3>
        <div className="flex items-center gap-2">
          {editing == null && (
            <button
              type="button"
              onClick={() => setEditing("new")}
              className="inline-flex h-8 items-center gap-1.5 rounded-lg bg-accent/12 px-3 text-sm font-medium text-accent-soft ring-1 ring-accent/30 transition hover:bg-accent/20"
            >
              <Plus size={15} />
              {tr.links.add}
            </button>
          )}
          <SmallButton
            onClick={search}
            disabled={find.isPending}
            title={tr.links.findSources}
            ariaLabel={tr.links.findSources}
            icon={find.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Search size={13} />}
          >
            {find.isPending ? tr.links.searching : tr.links.findSources}
          </SmallButton>
          {checkable.length > 0 && (
            <SmallButton
              onClick={() => void run(checkable)}
              disabled={progress != null}
              icon={progress ? <LoaderCircle size={13} className="animate-spin" /> : <Radar size={13} />}
            >
              {progress ? tr.links.checkingAll(progress.done, progress.total) : tr.links.checkAll}
            </SmallButton>
          )}
        </div>
      </div>
      <p className="mb-4 text-[13px] leading-relaxed text-ink-400">{tr.links.hint}</p>

      {found != null && (
        <div className="mb-4">
          {found.length === 0 ? (
            <div className="rounded-xl border border-dashed border-white/10 px-4 py-6 text-center text-sm text-ink-400">
              {tr.links.noSources}
            </div>
          ) : (
            <div className="space-y-2">
              {found.map((f) => (
                <FoundLinkRow key={`${f.siteId}-${f.url}`} appid={appid} gameTitle={gameTitle} found={f} site={siteById.get(f.siteId)} />
              ))}
            </div>
          )}
        </div>
      )}

      {editing != null && (
        <LinkForm appid={appid} link={editing === "new" ? null : editing} siteById={siteById} onDone={() => setEditing(null)} />
      )}

      <div className="space-y-2">
        {list.map((link) =>
          editing !== "new" && editing?.id === link.id ? null : (
            <LinkRow key={link.id} link={link} site={siteById.get(link.siteId)} gameTitle={gameTitle} onEdit={() => setEditing(link)} />
          ),
        )}
        {links.isSuccess && list.length === 0 && editing == null && (
          <div className="rounded-xl border border-dashed border-white/10 px-4 py-6 text-center text-sm text-ink-400">{tr.links.empty}</div>
        )}
      </div>
    </section>
  );
}

function siteName(site: SiteInfo | undefined, id: string) {
  return tr.links.sites[id] ?? site?.name ?? id;
}

function PlatformGlyph({ platform }: { platform: Platform | null }) {
  if (platform === "win") return <WindowsIcon size={13} />;
  if (platform === "mac") return <AppleIcon size={13} />;
  if (platform === "linux") return <LinuxIcon size={13} />;
  return null;
}

function LinkRow({ link, site, gameTitle, onEdit }: { link: GameLink; site: SiteInfo | undefined; gameTitle: string; onEdit: () => void }) {
  const check = useCheckLink();
  const remove = useDeleteLink();
  const queue = useEnqueueTorrent();
  const [confirming, setConfirming] = useState(false);
  const color = site?.color ?? "#8b93a7";
  const last = link.lastCheck;
  const ok = last?.status === "ok";
  const stale = last != null && !isRecent(last.checkedAt, STALE_DAYS);
  const magnet = isMagnet(link.url);
  // A browser-required site only hands the download out after a click-through, so its pages
  // open in the in-app browser, which queues whatever is downloaded there for this game.
  const browserRequired = site?.browserRequired === true;

  const open = () =>
    (browserRequired ? api.openBrowser(link.appid, gameTitle, link.url) : api.openLink(link.id)).catch((e) =>
      showToast({ tone: "error", title: errorText(toCmdError(e)) }),
    );

  // A magnet link has no page to check or open: it goes into the download queue.
  const enqueue = () =>
    queue.mutate(
      { appid: link.appid, title: gameTitle, source: link.url },
      {
        onSuccess: () => showToast({ tone: "success", title: tr.downloads.toastTorrentQueued(gameTitle) }),
        onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
      },
    );

  return (
    <div className="group rounded-xl bg-ink-800/80 p-3.5 ring-1 ring-white/6 transition hover:ring-white/12">
      <div className="flex items-start gap-3">
        <span
          className="grid size-10 shrink-0 place-items-center rounded-lg text-sm font-bold text-ink-950"
          style={{ background: `linear-gradient(135deg, ${color}, color-mix(in oklab, ${color} 55%, #0b0f16))` }}
          title={siteName(site, link.siteId)}
        >
          {link.host
            .replace(/^www\./, "")
            .charAt(0)
            .toUpperCase() || <Globe size={16} />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="truncate font-medium text-ink-50">{link.label ?? link.host}</span>
            <span className="rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] font-medium text-ink-300">{tr.links.kinds[link.kind]}</span>
            {magnet && (
              <span className="inline-flex items-center gap-1 rounded-md bg-violet/15 px-1.5 py-0.5 text-[11px] font-medium text-violet">
                <Download size={12} />
                {tr.links.magnet}
              </span>
            )}
            {link.platform && (
              <span className="inline-flex items-center gap-1 rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300">
                <PlatformGlyph platform={link.platform} />
                {tr.platforms[link.platform]}
              </span>
            )}
            {link.version && (
              <span className="rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300">v{link.version.replace(/^v/i, "")}</span>
            )}
            {link.insecure && (
              <span className="inline-flex items-center gap-1 rounded-md bg-warning/12 px-1.5 py-0.5 text-[11px] font-medium text-warning">
                <ShieldAlert size={12} />
                {tr.links.insecure}
              </span>
            )}
          </div>
          <div className="mt-0.5 truncate text-xs text-ink-400" title={link.url}>
            {siteName(site, link.siteId)} · {link.url}
          </div>
          {link.notes && <p className="mt-1.5 text-[13px] text-ink-300">{link.notes}</p>}
          <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
            {last ? (
              <>
                <span className={clsx("inline-flex items-center gap-1 font-medium", ok ? "text-success" : "text-danger")}>
                  {ok ? <CircleCheck size={13} /> : <CircleX size={13} />}
                  {tr.checkStatus[last.status]}
                  {last.httpStatus ? ` (${last.httpStatus})` : ""}
                </span>
                {stale && (
                  <span className="rounded-md bg-warning/12 px-1.5 py-0.5 text-[11px] font-medium text-warning">{tr.links.stale}</span>
                )}
                {ok && (
                  <span className="text-ink-300">
                    {last.isFile
                      ? [tr.links.file, fileKind(last.contentType, last.fileName), last.sizeBytes ? formatBytes(last.sizeBytes) : null]
                          .filter(Boolean)
                          .join(" · ")
                      : tr.links.webPage}
                  </span>
                )}
                <span className="text-ink-500">·</span>
                <span className="text-ink-400">
                  {tr.links.redirects(last.redirects)}
                  {last.finalHost && last.finalHost !== link.host ? ` → ${last.finalHost}` : ""}
                </span>
                <span className="text-ink-500">·</span>
                <span className="text-ink-500">{tr.links.checkedAgo(formatRelative(last.checkedAt))}</span>
              </>
            ) : magnet ? null : (
              <span className="text-ink-500">{tr.links.notChecked}</span>
            )}
          </div>
          {last && last.hops.length > 1 && (
            <div className="mt-1 flex flex-wrap items-center gap-1 text-[11.5px] text-ink-400">
              <span className="text-ink-500">{tr.links.chain}:</span>
              {last.hops.map((hop, i) => (
                <span key={`${hop.url}-${i}`} className="inline-flex items-center gap-1">
                  {i > 0 && <span className="text-ink-600">→</span>}
                  <span title={hop.url} className={clsx("rounded bg-white/6 px-1 py-0.5", i === last.hops.length - 1 && "text-ink-200")}>
                    {hostOf(hop.url)?.host ?? hop.url} ({hop.status})
                  </span>
                </span>
              ))}
            </div>
          )}
          {last?.fileName && ok && <div className="mt-1 truncate font-mono text-[11.5px] text-ink-400">{last.fileName}</div>}
        </div>
      </div>

      <div className="mt-3 flex flex-wrap items-center justify-end gap-1.5">
        {confirming ? (
          <>
            <span className="mr-1 text-xs text-ink-300">{tr.links.confirmDelete}</span>
            <SmallButton onClick={() => setConfirming(false)}>{tr.links.confirmNo}</SmallButton>
            <SmallButton
              tone="danger"
              onClick={() =>
                remove.mutate(link, {
                  onSuccess: () => showToast({ tone: "success", title: tr.links.toastDeleted }),
                  onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
                })
              }
            >
              {tr.links.confirmYes}
            </SmallButton>
          </>
        ) : (
          <>
            <SmallButton onClick={() => setConfirming(true)} icon={<Trash size={13} />}>
              {tr.links.delete}
            </SmallButton>
            <SmallButton onClick={onEdit} icon={<Pencil size={13} />}>
              {tr.links.edit}
            </SmallButton>
            {/* A magnet link is never checked: its address is the download itself. */}
            {!magnet && (
              <SmallButton
                onClick={() => check.mutate(link, { onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }) })}
                disabled={check.isPending}
                icon={check.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Radar size={13} />}
              >
                {check.isPending ? tr.links.checking : tr.links.check}
              </SmallButton>
            )}
            {magnet ? (
              <SmallButton
                tone="primary"
                onClick={enqueue}
                disabled={queue.isPending}
                icon={queue.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Download size={13} />}
              >
                {tr.links.addToQueue}
              </SmallButton>
            ) : browserRequired ? (
              <SmallButton tone="primary" onClick={open} icon={<Globe size={13} />}>
                {tr.links.openInBrowser}
              </SmallButton>
            ) : (
              <SmallButton tone="primary" onClick={open} icon={<ExternalLink size={13} />}>
                {tr.links.open}
              </SmallButton>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** A link a site search turned up, offered for the user to save or use. */
function FoundLinkRow({
  appid,
  gameTitle,
  found,
  site,
}: {
  appid: number;
  gameTitle: string;
  found: FoundLink;
  site: SiteInfo | undefined;
}) {
  const save = useSaveLink();
  const queue = useEnqueueTorrent();
  const color = site?.color ?? "#8b93a7";
  const input: LinkInput = { appid, url: found.url, label: found.label, kind: found.kind, version: found.version, notes: found.notes };
  const magnet = isMagnet(found.url);

  const add = () =>
    save.mutate(input, {
      onSuccess: () => showToast({ tone: "success", title: tr.links.toastSaved }),
      onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
    });

  // A found link is not saved yet, and only saved links can be opened by id, so opening saves it.
  const open = () =>
    save.mutate(input, {
      onSuccess: (link) => {
        api.openLink(link.id).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
      },
      onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
    });

  // Nothing to open: the magnet link goes into the download queue as it is.
  const enqueue = () =>
    queue.mutate(
      { appid, title: gameTitle, source: found.url },
      {
        onSuccess: () => showToast({ tone: "success", title: tr.downloads.toastTorrentQueued(gameTitle) }),
        onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
      },
    );

  // A page that needs a click-through opens in the in-app browser, where the download is
  // captured for the game the window is showing.
  const openInBrowser = () =>
    api.openBrowser(appid, gameTitle, found.url).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <div className="rounded-xl bg-ink-800/80 p-3.5 ring-1 ring-white/6">
      <div className="flex items-start gap-3">
        <span
          className="grid size-10 shrink-0 place-items-center rounded-lg text-sm font-bold text-ink-950"
          style={{ background: `linear-gradient(135deg, ${color}, color-mix(in oklab, ${color} 55%, #0b0f16))` }}
          title={siteName(site, found.siteId)}
        >
          {found.label.charAt(0).toUpperCase() || <Globe size={16} />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="truncate font-medium text-ink-50">{found.label}</span>
            <span className="rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] font-medium text-ink-300">{tr.links.kinds[found.kind]}</span>
            {magnet ? (
              <span className="inline-flex items-center gap-1 rounded-md bg-violet/15 px-1.5 py-0.5 text-[11px] font-medium text-violet">
                <Download size={12} />
                {tr.links.magnet}
              </span>
            ) : (
              found.direct && (
                <span className="rounded-md bg-accent/12 px-1.5 py-0.5 text-[11px] font-medium text-accent-soft">{tr.links.direct}</span>
              )
            )}
            {found.needsBrowser && (
              <span className="rounded-md bg-warning/12 px-1.5 py-0.5 text-[11px] font-medium text-warning">{tr.links.needsBrowser}</span>
            )}
          </div>
          <div className="mt-0.5 truncate text-xs text-ink-400" title={found.url}>
            {siteName(site, found.siteId)} · {found.url}
          </div>
          {(found.version || found.size) && (
            <div className="mt-1 flex flex-wrap items-center gap-x-2 text-xs text-ink-400">
              {found.version && <span>v{found.version.replace(/^v/i, "")}</span>}
              {found.size && <span>{found.size}</span>}
            </div>
          )}
          {found.notes && <p className="mt-1.5 text-[13px] text-ink-300">{found.notes}</p>}
        </div>
      </div>
      <div className="mt-3 flex flex-wrap items-center justify-end gap-1.5">
        {magnet ? (
          <SmallButton
            tone="primary"
            onClick={enqueue}
            disabled={queue.isPending}
            icon={queue.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Download size={13} />}
          >
            {tr.links.addToQueue}
          </SmallButton>
        ) : found.needsBrowser ? (
          <SmallButton onClick={openInBrowser} icon={<Globe size={13} />}>
            {tr.links.openInBrowser}
          </SmallButton>
        ) : (
          <SmallButton onClick={open} icon={<ExternalLink size={13} />}>
            {tr.links.open}
          </SmallButton>
        )}
        <SmallButton
          tone="primary"
          onClick={add}
          disabled={save.isPending}
          icon={save.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Plus size={13} />}
        >
          {tr.links.addFound}
        </SmallButton>
      </div>
    </div>
  );
}

function hostOf(input: string): { host: string; insecure: boolean } | null {
  const raw = input.trim();
  if (!raw) return null;
  try {
    const url = new URL(raw.includes("://") ? raw : `https://${raw}`);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    return { host: url.hostname, insecure: url.protocol === "http:" };
  } catch {
    return null;
  }
}

/** Checks a set of links one at a time, so a slow or dead host cannot stall the rest. */
function useBulkCheck() {
  const check = useCheckLink();
  const running = useRef(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  const run = async (targets: GameLink[]) => {
    if (running.current || targets.length === 0) return;
    running.current = true;
    setProgress({ done: 0, total: targets.length });
    let failed = 0;
    for (const link of targets) {
      try {
        await check.mutateAsync(link);
      } catch {
        failed += 1;
      }
      setProgress((p) => (p ? { done: p.done + 1, total: p.total } : p));
    }
    setProgress(null);
    running.current = false;
    showToast({
      tone: failed > 0 ? "error" : "success",
      title: failed > 0 ? tr.links.toastCheckFailed(failed) : tr.links.toastChecked(targets.length),
    });
  };

  return { run, progress };
}

function LinkForm({
  appid,
  link,
  siteById,
  onDone,
}: {
  appid: number;
  link: GameLink | null;
  siteById: Map<string, SiteInfo>;
  onDone: () => void;
}) {
  const save = useSaveLink();
  const [url, setUrl] = useState(link?.url ?? "");
  const [label, setLabel] = useState(link?.label ?? "");
  const [kind, setKind] = useState<LinkKind>(link?.kind ?? "download");
  const [platform, setPlatform] = useState<Platform | null>(link?.platform ?? null);
  const [version, setVersion] = useState(link?.version ?? "");
  const [notes, setNotes] = useState(link?.notes ?? "");
  const [error, setError] = useState<string | null>(null);
  const magnet = isMagnet(url);
  const preview = hostOf(url);
  // Site detection happens in Rust; until saved, show the generic site for the typed host.
  const detected = link && preview?.host === link.host ? siteName(siteById.get(link.siteId), link.siteId) : tr.links.sites.generic;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    save.mutate(
      { id: link?.id ?? null, appid, url, label: label || null, kind, platform, version: version || null, notes: notes || null },
      {
        onSuccess: () => {
          showToast({ tone: "success", title: tr.links.toastSaved });
          onDone();
        },
        onError: (err) => setError(errorText(toCmdError(err))),
      },
    );
  };

  const field =
    "h-9 w-full rounded-lg bg-ink-900 px-3 text-sm text-ink-50 ring-1 ring-white/10 outline-none placeholder:text-ink-500 focus:ring-accent/50";

  return (
    <form onSubmit={submit} className="animate-rise mb-3 rounded-xl bg-ink-800 p-4 ring-1 ring-accent/25">
      <div className="mb-3 text-sm font-semibold text-ink-50">{link ? tr.links.form.titleEdit : tr.links.form.titleNew}</div>
      <label className="block">
        <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.url}</span>
        <input
          autoFocus
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder={tr.links.form.urlPlaceholder}
          className={clsx(field, "font-mono text-[13px]")}
          spellCheck={false}
        />
      </label>
      {(preview || magnet) && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-ink-400">
          <Globe size={12} />
          {magnet ? tr.links.form.detected(tr.links.sites.generic, "magnet") : tr.links.form.detected(detected, preview!.host)}
          {preview?.insecure && (
            <span className="inline-flex items-center gap-1 text-warning">
              <ShieldAlert size={12} />
              {tr.links.form.insecureWarning}
            </span>
          )}
        </div>
      )}
      {magnet && <p className="mt-1 text-xs text-ink-500">{tr.links.form.magnetHint}</p>}

      <div className="mt-3 grid grid-cols-2 gap-3">
        <label className="block">
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.label}</span>
          <input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder={tr.links.form.labelPlaceholder}
            maxLength={120}
            className={field}
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.version}</span>
          <input
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder={tr.links.form.versionPlaceholder}
            maxLength={60}
            className={field}
          />
        </label>
        <div>
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.kind}</span>
          <ChoiceRow
            options={[
              { value: "download" as LinkKind, label: tr.links.kinds.download },
              { value: "page" as LinkKind, label: tr.links.kinds.page },
            ]}
            value={kind}
            onChange={setKind}
          />
        </div>
        <div>
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.platform}</span>
          <ChoiceRow
            options={[
              { value: null, label: tr.links.form.platformAny },
              { value: "win" as Platform, label: "Win" },
              { value: "mac" as Platform, label: "Mac" },
              { value: "linux" as Platform, label: "Linux" },
            ]}
            value={platform}
            onChange={setPlatform}
          />
        </div>
      </div>
      <label className="mt-3 block">
        <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.notes}</span>
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder={tr.links.form.notesPlaceholder}
          maxLength={1000}
          rows={2}
          className={clsx(field, "h-auto resize-none py-2")}
        />
      </label>

      {error && <p className="mt-3 rounded-lg bg-danger/10 px-3 py-2 text-sm text-danger ring-1 ring-danger/25">{error}</p>}

      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onDone}
          className="h-9 rounded-lg px-4 text-sm font-medium text-ink-300 hover:bg-white/6 hover:text-white"
        >
          {tr.links.form.cancel}
        </button>
        <button
          type="submit"
          disabled={save.isPending}
          className="inline-flex h-9 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-semibold text-ink-950 transition hover:bg-accent-soft disabled:opacity-60"
        >
          {save.isPending && <LoaderCircle size={15} className="animate-spin" />}
          {save.isPending ? tr.links.form.saving : tr.links.form.save}
        </button>
      </div>
    </form>
  );
}

function ChoiceRow<T>({ options, value, onChange }: { options: { value: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="flex rounded-lg bg-ink-900 p-0.5 ring-1 ring-white/10">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          onClick={() => onChange(o.value)}
          className={clsx(
            "h-8 flex-1 rounded-md text-[12.5px] font-medium transition",
            o.value === value ? "bg-ink-600 text-white" : "text-ink-300 hover:text-white",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
