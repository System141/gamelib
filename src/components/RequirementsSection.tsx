// "Bilgisayarım kaldırır mı?": a game's minimum and recommended requirements next to this
// computer. What can be measured (memory, video memory, free space, SSD, DirectX, Windows) is
// marked met or short; processors and graphics cards get a link to compare the models.

import clsx from "clsx";
import { CircleCheck, CircleHelp, CircleX, ExternalLink, MonitorCheck } from "lucide-react";
import type { ReactNode } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatBytes } from "../lib/format";
import { CHECK_ROW, comparableModel, type ListVerdict, listVerdict, worst } from "../lib/requirements";
import { showToast } from "../lib/toast";
import type { GameRequirements, RequirementKind, RequirementLine, RequirementList, ThisPc, Verdict } from "../lib/types";
import { useGameRequirements } from "../hooks/useData";

const ROW_ORDER: RequirementKind[] = ["os", "processor", "memory", "graphics", "directx", "storage", "sound", "network", "notes", "other"];

export function RequirementsSection({ appid }: { appid: number }) {
  const req = useGameRequirements(appid);
  const notFound = req.isError && toCmdError(req.error).kind === "not_found";
  return (
    <section>
      <div className="mb-3 flex items-center gap-2">
        <MonitorCheck size={18} className="text-accent" />
        <h3 className="font-display text-lg font-semibold text-ink-50">{tr.requirements.title}</h3>
      </div>
      {req.isLoading ? (
        <div className="shimmer h-56 rounded-xl" />
      ) : notFound ? (
        <p className="text-sm text-ink-400">{tr.requirements.empty}</p>
      ) : req.isError ? (
        <p className="text-sm text-ink-400">
          {tr.requirements.loadError}{" "}
          <button type="button" onClick={() => void req.refetch()} className="text-accent-soft hover:underline">
            {tr.requirements.retry}
          </button>
        </p>
      ) : req.data ? (
        <Requirements data={req.data} />
      ) : null}
    </section>
  );
}

function Requirements({ data }: { data: GameRequirements }) {
  const { minimum, recommended, pc } = data;
  if (minimum.lines.length === 0 && recommended.lines.length === 0) {
    return <p className="text-sm text-ink-400">{tr.requirements.empty}</p>;
  }
  const hasRecommended = recommended.lines.length > 0;
  const rows = ROW_ORDER.filter((kind) => minimum.lines.some((l) => l.kind === kind) || recommended.lines.some((l) => l.kind === kind));
  const notThisSystem = data.platform === "win" && pc.windows == null;

  return (
    <div className="space-y-3">
      <div className={clsx("grid gap-3", hasRecommended && "sm:grid-cols-2")}>
        <VerdictCard label={tr.requirements.minimum} verdict={listVerdict(minimum)} />
        {hasRecommended && <VerdictCard label={tr.requirements.recommended} verdict={listVerdict(recommended)} />}
      </div>

      <div className="overflow-x-auto rounded-xl ring-1 ring-white/6">
        <table className="w-full min-w-[640px] border-collapse text-left text-[13px]">
          <thead className="bg-ink-800/80 text-xs text-ink-400">
            <tr>
              <th className="w-28 px-3 py-2 font-medium" />
              <th className="px-3 py-2 font-medium">{tr.requirements.minimum}</th>
              {hasRecommended && <th className="px-3 py-2 font-medium">{tr.requirements.recommended}</th>}
              <th className="px-3 py-2 font-medium">{tr.requirements.thisPc}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((kind) => (
              <tr key={kind} className="border-t border-white/6 align-top">
                <th scope="row" className="px-3 py-2.5 text-xs font-medium text-ink-400">
                  {tr.requirements.kinds[kind]}
                </th>
                <RequirementCell lines={minimum.lines} list={minimum} kind={kind} />
                {hasRecommended && <RequirementCell lines={recommended.lines} list={recommended} kind={kind} />}
                <td className="px-3 py-2.5 text-ink-100">
                  <ThisPcCell kind={kind} pc={pc} requirement={text(minimum.lines, kind) || text(recommended.lines, kind)} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="space-y-1 text-xs leading-relaxed text-ink-500">
        {notThisSystem && <p>{tr.requirements.otherPlatform}</p>}
        <p>{tr.requirements.manualHint}</p>
        {pc.diskPath && <p>{tr.requirements.diskHint(pc.diskPath)}</p>}
      </div>
    </div>
  );
}

/** A kind's lines as one text; unknown kinds keep the store's label. */
function text(lines: RequirementLine[], kind: RequirementKind): string {
  return lines
    .filter((l) => l.kind === kind)
    .map((l) => (l.label ? `${l.label}: ${l.text}` : l.text))
    .join("\n");
}

function RequirementCell({ lines, list, kind }: { lines: RequirementLine[]; list: RequirementList; kind: RequirementKind }) {
  const verdict = worst(list.checks.filter((c) => CHECK_ROW[c.kind] === kind));
  const value = text(lines, kind);
  return (
    <td className="px-3 py-2.5 text-ink-200">
      {value ? (
        <span className="flex items-start gap-1.5">
          {verdict && <VerdictIcon verdict={verdict} />}
          <span className="whitespace-pre-line">{value}</span>
        </span>
      ) : (
        <span className="text-ink-600">—</span>
      )}
    </td>
  );
}

function VerdictIcon({ verdict }: { verdict: Verdict }) {
  const common = "mt-0.5 shrink-0";
  if (verdict === "ok") return <CircleCheck size={14} className={clsx(common, "text-success")} aria-label={tr.requirements.verdictOk} />;
  if (verdict === "short")
    return <CircleX size={14} className={clsx(common, "text-danger")} aria-label={tr.requirements.verdictShort("")} />;
  return <CircleHelp size={14} className={clsx(common, "text-ink-500")} aria-label={tr.requirements.verdictNone} />;
}

function ThisPcCell({ kind, pc, requirement }: { kind: RequirementKind; pc: ThisPc; requirement: string }) {
  const unknown = <span className="text-ink-500">{tr.requirements.unknown}</span>;
  switch (kind) {
    case "os":
      return <>{pc.os || unknown}</>;
    case "processor":
      return pc.cpu ? (
        <Compare mine={pc.cpu} requirement={requirement}>
          {pc.cpu}
          {pc.cores != null && <span className="text-ink-400"> · {tr.requirements.cores(pc.cores)}</span>}
        </Compare>
      ) : (
        unknown
      );
    case "memory":
      return pc.memory != null ? <>{formatBytes(pc.memory)}</> : unknown;
    case "graphics":
      return pc.gpu ? (
        <Compare mine={pc.gpu} requirement={requirement}>
          {pc.gpu}
          {pc.videoMemory != null && <span className="text-ink-400"> · {tr.requirements.videoMemory(formatBytes(pc.videoMemory))}</span>}
        </Compare>
      ) : (
        unknown
      );
    case "directx":
      return pc.directx != null ? <>{tr.requirements.directxLevel(pc.directx)}</> : unknown;
    case "storage":
      return pc.diskFree != null ? (
        <>
          {tr.requirements.free(formatBytes(pc.diskFree))}
          {pc.diskSsd != null && <span className="text-ink-400"> · {pc.diskSsd ? tr.requirements.ssd : tr.requirements.hdd}</span>}
        </>
      ) : (
        unknown
      );
    default:
      return null;
  }
}

/** This computer's part, with a web search comparing it to the required model of the same maker. */
function Compare({ mine, requirement, children }: { mine: string; requirement: string; children: ReactNode }) {
  const model = comparableModel(requirement, mine);
  return (
    <span>
      {children}
      {model && (
        <button
          type="button"
          onClick={() =>
            void api.openSearch("google", `${mine} vs ${model}`).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }))
          }
          className="ml-2 inline-flex items-center gap-1 text-xs text-accent-soft hover:underline"
          title={`${mine} vs ${model}`}
        >
          {tr.requirements.compare}
          <ExternalLink size={11} />
        </button>
      )}
    </span>
  );
}

function VerdictCard({ label, verdict }: { label: string; verdict: ListVerdict }) {
  const [tone, icon, message] =
    verdict.kind === "ok"
      ? (["ok", <CircleCheck key="i" size={18} />, tr.requirements.verdictOk] as const)
      : verdict.kind === "short"
        ? ([
            "short",
            <CircleX key="i" size={18} />,
            tr.requirements.verdictShort(verdict.missing.map((m) => tr.requirements.checks[m]).join(", ")),
          ] as const)
        : verdict.kind === "partial"
          ? (["partial", <CircleCheck key="i" size={18} />, tr.requirements.verdictPartial] as const)
          : (["none", <CircleHelp key="i" size={18} />, tr.requirements.verdictNone] as const);
  return (
    <div
      className={clsx(
        "flex items-center gap-3 rounded-xl px-4 py-3 ring-1",
        tone === "ok" && "bg-success/8 text-success ring-success/25",
        tone === "short" && "bg-danger/8 text-danger ring-danger/25",
        tone === "partial" && "bg-success/5 text-success/90 ring-success/15",
        tone === "none" && "bg-white/3 text-ink-300 ring-white/8",
      )}
    >
      {icon}
      <div>
        <div className="text-xs text-ink-400">{label}</div>
        <div className="font-medium">{message}</div>
      </div>
    </div>
  );
}
