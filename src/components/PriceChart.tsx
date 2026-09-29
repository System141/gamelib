// Steam's price over the past two years as steps, the regular price as a gray reference and the
// lowest point marked. Hovering or the arrow keys move a crosshair with a readout; a table view
// lists every change, so no value depends on hovering.

import { useId, useState } from "react";
import { tr } from "../i18n/tr";
import { formatDate } from "../lib/format";
import { formatMoney, priceSteps } from "../lib/prices";
import type { PricePoint } from "../lib/types";

const W = 720;
const H = 200;
const PAD = { left: 56, right: 92, top: 18, bottom: 28 };

/** A round top for the y scale: 1, 2, 2.5 or 5 times a power of ten. */
function niceMax(value: number): number {
  if (value <= 0) return 1;
  const power = 10 ** Math.floor(Math.log10(value));
  const step = [1, 2, 2.5, 5, 10].find((s) => s * power >= value) ?? 10;
  return step * power;
}

export function PriceChart({ history, currency, now }: { history: PricePoint[]; currency: string; now: number }) {
  const steps = priceSteps(history, now);
  const [active, setActive] = useState<number | null>(null);
  const tableId = useId();
  if (steps.length === 0) return <p className="text-sm text-ink-400">{tr.prices.noHistory}</p>;

  const money = (amount: number) => formatMoney({ amount, currency });
  const start = steps[0]!.from;
  const span = Math.max(1, now - start);
  const top = niceMax(Math.max(...steps.map((s) => Math.max(s.price, s.regular))));
  const x = (t: number) => PAD.left + ((t - start) / span) * (W - PAD.left - PAD.right);
  const y = (v: number) => PAD.top + (1 - v / top) * (H - PAD.top - PAD.bottom);
  const path = (value: (i: number) => number) =>
    steps.map((s, i) => `${i === 0 ? "M" : "L"}${x(s.from).toFixed(1)},${y(value(i)).toFixed(1)} H${x(s.to).toFixed(1)}`).join(" ");
  const pricePath = path((i) => steps[i]!.price);
  const regularPath = path((i) => steps[i]!.regular);
  const area = `${pricePath} V${y(0)} H${x(start)} Z`;
  const low = steps.reduce((best, s, i) => (s.price < steps[best]!.price ? i : best), 0);
  const lowStep = steps[low]!;
  // Marked only when the price moved; the label stays inside the plot and goes below the point
  // when there is no room above it.
  const marked = steps.some((s) => s.price !== lowStep.price);
  const lowX = (x(lowStep.from) + x(lowStep.to)) / 2;
  const lowY = y(lowStep.price);
  const last = steps[steps.length - 1]!;
  const ticks = [0, top / 2, top];
  // New Year's days in view, clear of the start and end labels.
  const years: { t: number; year: number }[] = [];
  for (let year = new Date(start * 1000).getFullYear() + 1; ; year++) {
    const t = new Date(year, 0, 1).getTime() / 1000;
    if (t >= now) break;
    if (x(t) - PAD.left > 90 && W - PAD.right - x(t) > 40) years.push({ t, year });
  }
  const shown = active != null ? steps[active] : null;

  const pick = (clientX: number, rect: DOMRect) => {
    const t = start + ((((clientX - rect.left) / rect.width) * W - PAD.left) / (W - PAD.left - PAD.right)) * span;
    const i = steps.findIndex((s) => t >= s.from && t < s.to);
    setActive(i === -1 ? (t < start ? 0 : steps.length - 1) : i);
  };

  return (
    <div>
      <div className="mb-2 flex items-center gap-4 text-xs text-ink-300">
        <span className="inline-flex items-center gap-1.5">
          <span className="h-0.5 w-4 rounded bg-chart" />
          {tr.prices.price}
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="h-0.5 w-4 rounded bg-ink-400" />
          {tr.prices.regular}
        </span>
      </div>
      <div className="relative">
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className="w-full touch-none outline-none select-none focus-visible:ring-2 focus-visible:ring-accent/60"
          role="img"
          aria-label={tr.prices.historyLabel(formatDate(start), tr.prices.today)}
          aria-describedby={tableId}
          tabIndex={0}
          onPointerMove={(e) => pick(e.clientX, e.currentTarget.getBoundingClientRect())}
          onPointerLeave={() => setActive(null)}
          onFocus={() => setActive(steps.length - 1)}
          onBlur={() => setActive(null)}
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft") setActive((a) => Math.max(0, (a ?? steps.length) - 1));
            if (e.key === "ArrowRight") setActive((a) => Math.min(steps.length - 1, (a ?? -1) + 1));
          }}
        >
          {ticks.map((v) => (
            <g key={v}>
              <line x1={PAD.left} x2={W - PAD.right} y1={y(v)} y2={y(v)} className="stroke-ink-700" strokeWidth={1} />
              <text x={PAD.left - 8} y={y(v)} dy="0.32em" textAnchor="end" className="fill-ink-400 text-[11px] tabular-nums">
                {money(v)}
              </text>
            </g>
          ))}
          <text x={PAD.left} y={H - 8} className="fill-ink-400 text-[11px]">
            {formatDate(start)}
          </text>
          <text x={W - PAD.right} y={H - 8} textAnchor="end" className="fill-ink-400 text-[11px]">
            {tr.prices.today}
          </text>
          {years.map(({ t, year }) => (
            <g key={year}>
              <line x1={x(t)} x2={x(t)} y1={y(0)} y2={y(0) + 4} className="stroke-ink-500" strokeWidth={1} />
              <text x={x(t)} y={H - 8} textAnchor="middle" className="fill-ink-400 text-[11px] tabular-nums">
                {year}
              </text>
            </g>
          ))}

          <path d={area} className="fill-chart/10" />
          <path d={regularPath} fill="none" className="stroke-ink-400" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
          <path d={pricePath} fill="none" className="stroke-chart" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />

          {/* Direct labels at the right end. */}
          <text x={W - PAD.right + 8} y={y(last.price)} dy="0.32em" className="fill-ink-100 text-[11px] font-medium tabular-nums">
            {money(last.price)}
          </text>
          {Math.abs(y(last.regular) - y(last.price)) > 14 && (
            <text x={W - PAD.right + 8} y={y(last.regular)} dy="0.32em" className="fill-ink-400 text-[11px] tabular-nums">
              {money(last.regular)}
            </text>
          )}

          {/* The lowest price, with a surface ring so it reads over the lines and a halo so the
              lines don't cross its label. */}
          {marked && (
            <>
              <circle cx={lowX} cy={lowY} r={4.5} className="fill-chart stroke-ink-800" strokeWidth={2} />
              <text
                x={Math.min(Math.max(lowX, PAD.left + 44), W - PAD.right - 44)}
                y={lowY - 10 < PAD.top + 8 ? lowY + 18 : lowY - 10}
                textAnchor="middle"
                strokeWidth={4}
                strokeLinejoin="round"
                className="fill-ink-200 stroke-ink-800 text-[11px] tabular-nums [paint-order:stroke]"
              >
                {tr.prices.lowestMark} {money(lowStep.price)}
              </text>
            </>
          )}

          {shown && (
            <line
              x1={x(shown.from)}
              x2={x(shown.from)}
              y1={PAD.top}
              y2={H - PAD.bottom}
              className="stroke-ink-300"
              strokeWidth={1}
              pointerEvents="none"
            />
          )}
        </svg>
        {shown && (
          <div
            className="pointer-events-none absolute top-2 z-10 w-max rounded-lg bg-ink-900/95 px-3 py-2 text-xs shadow-lg ring-1 ring-white/10"
            style={{
              left: `${(x(shown.from) / W) * 100}%`,
              transform: x(shown.from) > W / 2 ? "translateX(calc(-100% - 8px))" : "translateX(8px)",
            }}
            role="status"
          >
            <div className="text-ink-400">
              {formatDate(shown.from)} – {shown === last ? tr.prices.today : formatDate(shown.to)}
            </div>
            <div className="mt-1 flex items-center gap-2">
              <span className="h-0.5 w-3 rounded bg-chart" />
              <span className="text-sm font-semibold text-ink-50 tabular-nums">{money(shown.price)}</span>
              {shown.cut > 0 && <span className="text-ink-300">{tr.prices.cut(shown.cut)}</span>}
            </div>
            <div className="mt-0.5 flex items-center gap-2 text-ink-300">
              <span className="h-0.5 w-3 rounded bg-ink-400" />
              {tr.prices.regular} {money(shown.regular)}
            </div>
          </div>
        )}
      </div>
      <details className="mt-2 text-xs text-ink-400">
        <summary className="cursor-pointer select-none hover:text-ink-200">{tr.prices.showTable}</summary>
        <table id={tableId} className="mt-2 w-full text-left tabular-nums">
          <thead className="text-ink-500">
            <tr>
              <th className="py-1 font-medium">{tr.prices.date}</th>
              <th className="py-1 font-medium">{tr.prices.price}</th>
              <th className="py-1 font-medium">{tr.prices.regular}</th>
              <th className="py-1 font-medium">{tr.prices.discount}</th>
            </tr>
          </thead>
          <tbody className="text-ink-200">
            {steps.map((s) => (
              <tr key={s.from} className="border-t border-white/6">
                <td className="py-1">{formatDate(s.from)}</td>
                <td className="py-1">{money(s.price)}</td>
                <td className="py-1">{money(s.regular)}</td>
                <td className="py-1">{s.cut > 0 ? `%${s.cut}` : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </div>
  );
}
