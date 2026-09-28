import { Database, LoaderCircle, Sparkles, X } from "lucide-react";
import type { ReactNode } from "react";
import { tr } from "../i18n/tr";
import { formatDuration, nowSeconds } from "../lib/format";
import type { AppStatus } from "../lib/types";
import { Logo } from "./icons";

interface Props {
  status: AppStatus | undefined;
  onFullSync: () => void;
  onNewReleases: () => void;
  onCancel: () => void;
}

export function FirstRun({ status, onFullSync, onNewReleases, onCancel }: Props) {
  const running = status?.worker != null;
  return (
    <div className="flex h-full items-center justify-center overflow-y-auto px-8 py-12">
      <div className="animate-rise w-full max-w-3xl text-center">
        <div className="mx-auto mb-7 w-fit rounded-[28px] shadow-2xl shadow-accent/20">
          <Logo size={88} />
        </div>
        <p className="text-sm font-semibold tracking-[0.2em] text-accent uppercase">{tr.firstRun.eyebrow}</p>
        <h1 className="mt-3 font-display text-5xl font-semibold tracking-tight text-ink-50">
          <span className="text-gradient">{tr.firstRun.title}</span>
        </h1>
        <p className="mx-auto mt-5 max-w-2xl text-base leading-relaxed text-ink-300">{tr.firstRun.subtitle}</p>

        {running ? (
          <Progress status={status!} onCancel={onCancel} />
        ) : (
          <div className="mt-10 grid gap-4 text-left sm:grid-cols-2">
            <Choice
              icon={<Database size={22} className="text-accent" />}
              title={tr.firstRun.fullTitle}
              desc={tr.firstRun.fullDesc}
              cta={tr.firstRun.fullCta}
              primary
              onClick={onFullSync}
            />
            <Choice
              icon={<Sparkles size={22} className="text-violet" />}
              title={tr.firstRun.newTitle}
              desc={tr.firstRun.newDesc}
              cta={tr.firstRun.newCta}
              onClick={onNewReleases}
            />
          </div>
        )}

        <p className="mx-auto mt-10 max-w-xl text-xs leading-relaxed text-ink-500">{tr.firstRun.note}</p>
        <p className="mx-auto mt-2 max-w-xl text-xs text-ink-600">{tr.firstRun.legal}</p>
      </div>
    </div>
  );
}

function Choice({ icon, title, desc, cta, primary = false, onClick }: { icon: ReactNode; title: string; desc: string; cta: string; primary?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        primary
          ? "group relative overflow-hidden rounded-2xl bg-gradient-to-br from-accent/18 via-ink-800 to-violet/12 p-6 ring-1 ring-accent/35 transition hover:-translate-y-0.5 hover:ring-accent/60 hover:shadow-2xl hover:shadow-accent/15"
          : "group relative overflow-hidden rounded-2xl bg-ink-800/80 p-6 ring-1 ring-white/8 transition hover:-translate-y-0.5 hover:ring-violet/45 hover:shadow-2xl hover:shadow-violet/10"
      }
    >
      <span className="grid size-11 place-items-center rounded-xl bg-white/6 ring-1 ring-white/10">{icon}</span>
      <span className="mt-4 block font-display text-xl font-semibold text-ink-50">{title}</span>
      <span className="mt-1 block text-sm text-ink-400">{desc}</span>
      <span
        className={
          primary
            ? "mt-5 inline-flex h-10 items-center rounded-lg bg-gradient-to-r from-accent-strong to-violet-strong px-4 text-sm font-semibold text-white shadow-lg shadow-accent/20"
            : "mt-5 inline-flex h-10 items-center rounded-lg bg-white/6 px-4 text-sm font-semibold text-ink-100 ring-1 ring-white/10 group-hover:bg-white/10"
        }
      >
        {cta}
      </span>
    </button>
  );
}

function Progress({ status, onCancel }: { status: AppStatus; onCancel: () => void }) {
  const p = status.progress;
  const known = p != null && p.total > 0;
  const pct = known ? Math.min(100, (p.fetched / p.total) * 100) : null;
  const elapsed = p ? Math.max(1, nowSeconds() - p.startedAt) : 0;
  const eta = known && p.fetched > 0 && p.phase === "catalog" ? ((p.total - p.fetched) * elapsed) / p.fetched : null;

  return (
    <div className="mx-auto mt-10 max-w-xl rounded-2xl bg-ink-800/80 p-6 text-left ring-1 ring-white/8">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <LoaderCircle size={20} className="animate-spin text-accent" />
          <span className="font-medium text-ink-50">{tr.sync.phases[p?.phase ?? "starting"]}</span>
        </div>
        <button type="button" onClick={onCancel} className="inline-flex h-8 items-center gap-1.5 rounded-lg px-3 text-sm text-ink-300 ring-1 ring-white/10 hover:bg-white/6 hover:text-white">
          <X size={14} />
          {tr.firstRun.cancel}
        </button>
      </div>
      <div className="relative mt-5 h-2.5 overflow-hidden rounded-full bg-ink-700">
        {pct != null ? (
          <div className="h-full rounded-full bg-gradient-to-r from-accent to-violet transition-[width] duration-500" style={{ width: `${Math.max(2, pct)}%` }} />
        ) : (
          <div className="animate-progress absolute inset-y-0 w-1/3 rounded-full bg-gradient-to-r from-transparent via-accent to-transparent" />
        )}
      </div>
      <div className="mt-3 flex justify-between text-sm text-ink-400 tabular-nums">
        <span>{known ? tr.firstRun.progress(p.fetched, p.total) : p ? tr.sync.newProgress(p.fetched) : ""}</span>
        <span>{eta != null ? tr.firstRun.eta(formatDuration(eta)) : pct != null ? `%${Math.round(pct)}` : ""}</span>
      </div>
    </div>
  );
}
