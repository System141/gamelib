// "Steam dışı bağlantılar": user-added links to other stores, official sites or downloads.

import clsx from "clsx";
import {
  CircleCheck,
  CircleX,
  ExternalLink,
  Globe,
  Link2,
  LoaderCircle,
  Pencil,
  Plus,
  Radar,
  ShieldAlert,
  Trash,
} from "lucide-react";
import { type FormEvent, type ReactNode, useMemo, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { fileKind, formatBytes, formatRelative } from "../lib/format";
import { showToast } from "../lib/toast";
import type { GameLink, LinkKind, Platform, SiteInfo } from "../lib/types";
import { useCheckLink, useDeleteLink, useLinks, useSaveLink, useSites } from "../hooks/useData";
import { AppleIcon, LinuxIcon, WindowsIcon } from "./icons";

export function LinksSection({ appid }: { appid: number }) {
  const links = useLinks(appid);
  const sites = useSites();
  const [editing, setEditing] = useState<GameLink | "new" | null>(null);
  const siteById = useMemo(() => new Map((sites.data ?? []).map((s) => [s.id, s])), [sites.data]);
  const list = links.data ?? [];

  return (
    <section>
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 font-display text-lg font-semibold text-ink-50">
          <Link2 size={18} className="text-accent" />
          {tr.links.title}
          {list.length > 0 && <span className="rounded-full bg-white/8 px-2 text-xs font-medium text-ink-300">{list.length}</span>}
        </h3>
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
      </div>
      <p className="mb-4 text-[13px] leading-relaxed text-ink-400">{tr.links.hint}</p>

      {editing != null && (
        <LinkForm appid={appid} link={editing === "new" ? null : editing} siteById={siteById} onDone={() => setEditing(null)} />
      )}

      <div className="space-y-2">
        {list.map((link) =>
          editing !== "new" && editing?.id === link.id ? null : (
            <LinkRow key={link.id} link={link} site={siteById.get(link.siteId)} onEdit={() => setEditing(link)} />
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

function LinkRow({ link, site, onEdit }: { link: GameLink; site: SiteInfo | undefined; onEdit: () => void }) {
  const check = useCheckLink();
  const remove = useDeleteLink();
  const [confirming, setConfirming] = useState(false);
  const color = site?.color ?? "#8b93a7";
  const last = link.lastCheck;
  const ok = last?.status === "ok";

  const open = () => api.openLink(link.id).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <div className="group rounded-xl bg-ink-800/80 p-3.5 ring-1 ring-white/6 transition hover:ring-white/12">
      <div className="flex items-start gap-3">
        <span
          className="grid size-10 shrink-0 place-items-center rounded-lg text-sm font-bold text-ink-950"
          style={{ background: `linear-gradient(135deg, ${color}, color-mix(in oklab, ${color} 55%, #0b0f16))` }}
          title={siteName(site, link.siteId)}
        >
          {link.host.replace(/^www\./, "").charAt(0).toUpperCase() || <Globe size={16} />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="truncate font-medium text-ink-50">{link.label ?? link.host}</span>
            <span className="rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] font-medium text-ink-300">{tr.links.kinds[link.kind]}</span>
            {link.platform && (
              <span className="inline-flex items-center gap-1 rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300">
                <PlatformGlyph platform={link.platform} />
                {tr.platforms[link.platform]}
              </span>
            )}
            {link.version && <span className="rounded-md bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300">v{link.version.replace(/^v/i, "")}</span>}
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
                {ok && (
                  <span className="text-ink-300">
                    {last.isFile
                      ? [tr.links.file, fileKind(last.contentType, last.fileName), last.sizeBytes ? formatBytes(last.sizeBytes) : null].filter(Boolean).join(" · ")
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
            ) : (
              <span className="text-ink-500">{tr.links.notChecked}</span>
            )}
          </div>
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
            <SmallButton
              onClick={() => check.mutate(link, { onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }) })}
              disabled={check.isPending}
              icon={check.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Radar size={13} />}
            >
              {check.isPending ? tr.links.checking : tr.links.check}
            </SmallButton>
            <SmallButton tone="primary" onClick={open} icon={<ExternalLink size={13} />}>
              {tr.links.open}
            </SmallButton>
          </>
        )}
      </div>
    </div>
  );
}

function SmallButton({
  children,
  onClick,
  icon,
  tone = "default",
  disabled,
}: {
  children: ReactNode;
  onClick: () => void;
  icon?: ReactNode;
  tone?: "default" | "primary" | "danger";
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={clsx(
        "inline-flex h-7.5 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium ring-1 transition disabled:opacity-60",
        tone === "primary" && "bg-accent/15 text-accent-soft ring-accent/35 hover:bg-accent/25",
        tone === "danger" && "bg-danger/15 text-danger ring-danger/35 hover:bg-danger/25",
        tone === "default" && "bg-white/4 text-ink-200 ring-white/8 hover:bg-white/8 hover:text-white",
      )}
    >
      {icon}
      {children}
    </button>
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

function LinkForm({ appid, link, siteById, onDone }: { appid: number; link: GameLink | null; siteById: Map<string, SiteInfo>; onDone: () => void }) {
  const save = useSaveLink();
  const [url, setUrl] = useState(link?.url ?? "");
  const [label, setLabel] = useState(link?.label ?? "");
  const [kind, setKind] = useState<LinkKind>(link?.kind ?? "download");
  const [platform, setPlatform] = useState<Platform | null>(link?.platform ?? null);
  const [version, setVersion] = useState(link?.version ?? "");
  const [notes, setNotes] = useState(link?.notes ?? "");
  const [error, setError] = useState<string | null>(null);
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

  const field = "h-9 w-full rounded-lg bg-ink-900 px-3 text-sm text-ink-50 ring-1 ring-white/10 outline-none placeholder:text-ink-500 focus:ring-accent/50";

  return (
    <form onSubmit={submit} className="animate-rise mb-3 rounded-xl bg-ink-800 p-4 ring-1 ring-accent/25">
      <div className="mb-3 text-sm font-semibold text-ink-50">{link ? tr.links.form.titleEdit : tr.links.form.titleNew}</div>
      <label className="block">
        <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.url}</span>
        <input autoFocus value={url} onChange={(e) => setUrl(e.target.value)} placeholder={tr.links.form.urlPlaceholder} className={clsx(field, "font-mono text-[13px]")} spellCheck={false} />
      </label>
      {preview && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-ink-400">
          <Globe size={12} />
          {tr.links.form.detected(detected, preview.host)}
          {preview.insecure && (
            <span className="inline-flex items-center gap-1 text-warning">
              <ShieldAlert size={12} />
              {tr.links.form.insecureWarning}
            </span>
          )}
        </div>
      )}

      <div className="mt-3 grid grid-cols-2 gap-3">
        <label className="block">
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.label}</span>
          <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder={tr.links.form.labelPlaceholder} maxLength={120} className={field} />
        </label>
        <label className="block">
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.version}</span>
          <input value={version} onChange={(e) => setVersion(e.target.value)} placeholder={tr.links.form.versionPlaceholder} maxLength={60} className={field} />
        </label>
        <div>
          <span className="mb-1 block text-xs font-medium text-ink-300">{tr.links.form.kind}</span>
          <ChoiceRow options={[{ value: "download" as LinkKind, label: tr.links.kinds.download }, { value: "page" as LinkKind, label: tr.links.kinds.page }]} value={kind} onChange={setKind} />
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
        <textarea value={notes} onChange={(e) => setNotes(e.target.value)} placeholder={tr.links.form.notesPlaceholder} maxLength={1000} rows={2} className={clsx(field, "h-auto resize-none py-2")} />
      </label>

      {error && <p className="mt-3 rounded-lg bg-danger/10 px-3 py-2 text-sm text-danger ring-1 ring-danger/25">{error}</p>}

      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={onDone} className="h-9 rounded-lg px-4 text-sm font-medium text-ink-300 hover:bg-white/6 hover:text-white">
          {tr.links.form.cancel}
        </button>
        <button type="submit" disabled={save.isPending} className="inline-flex h-9 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-semibold text-ink-950 transition hover:bg-accent-soft disabled:opacity-60">
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
          className={clsx("h-8 flex-1 rounded-md text-[12.5px] font-medium transition", o.value === value ? "bg-ink-600 text-white" : "text-ink-300 hover:text-white")}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
