// "GameLib X hazır": a bar under the top bar when a newer version is out, with its progress
// while it installs.

import { ArrowUpCircle, ExternalLink, LoaderCircle, X } from "lucide-react";
import { useState } from "react";
import { tr } from "../i18n/tr";
import { api } from "../lib/api";
import { formatPercent } from "../lib/format";
import { useInstallUpdate, useUpdateStatus } from "../hooks/useUpdater";

export function UpdateBanner() {
  const status = useUpdateStatus().data;
  const { install, installing, progress } = useInstallUpdate();
  const [dismissed, setDismissed] = useState<string | null>(null);
  const update = status?.update;
  if (!update || (dismissed === update.version && !installing)) return null;
  const pct = progress?.total ? Math.min(100, (progress.downloaded / progress.total) * 100) : null;

  return (
    <div className="relative shrink-0 border-b border-accent/20 bg-accent/8" role="status">
      <div className="flex h-10 items-center gap-3 px-8 text-[13px]">
        {installing ? (
          <>
            <LoaderCircle size={14} className="animate-spin text-accent" />
            <span className="text-ink-100">{pct != null && pct >= 100 ? tr.update.restarting : tr.update.downloading(update.version)}</span>
            {pct != null && pct < 100 && <span className="text-ink-300 tabular-nums">{formatPercent(pct)}</span>}
          </>
        ) : (
          <>
            <ArrowUpCircle size={15} className="text-accent" />
            <span className="text-ink-100">{tr.update.ready(update.version)}</span>
            <button
              type="button"
              onClick={() => void api.openReleasePage(update.version).catch(() => undefined)}
              className="inline-flex items-center gap-1 text-ink-300 underline-offset-2 hover:text-white hover:underline"
            >
              {tr.update.notes}
              <ExternalLink size={12} />
            </button>
            <span className="ml-auto flex items-center gap-2">
              <button
                type="button"
                onClick={install}
                className="h-7 rounded-md bg-accent px-3 text-[12.5px] font-semibold text-ink-950 transition hover:bg-accent-soft"
              >
                {tr.update.install}
              </button>
              <button
                type="button"
                onClick={() => setDismissed(update.version)}
                className="grid size-7 place-items-center rounded-md text-ink-400 hover:bg-white/8 hover:text-white"
                aria-label={tr.update.later}
                title={tr.update.later}
              >
                <X size={15} />
              </button>
            </span>
          </>
        )}
      </div>
      {installing && (
        <div className="absolute inset-x-0 bottom-0 h-0.5 bg-ink-700">
          <div
            className="h-full bg-gradient-to-r from-accent to-violet transition-[width] duration-300"
            style={{ width: `${pct ?? 0}%` }}
          />
        </div>
      )}
    </div>
  );
}
