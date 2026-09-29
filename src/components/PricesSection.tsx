// "Fiyatlar ve fırsatlar": what the game costs in legitimate shops today, its lowest prices, the
// subscriptions and bundles that include it and Steam's price over two years, from IsThereAnyDeal
// with the user's own API key.

import clsx from "clsx";
import { BadgePercent, CircleCheck, Clock, ExternalLink, KeyRound, LoaderCircle, Package, Sparkles } from "lucide-react";
import { type FormEvent, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatDate, nowSeconds } from "../lib/format";
import { formatMoney, type PriceVerdict, priceVerdict } from "../lib/prices";
import { showToast } from "../lib/toast";
import type { Bundle, Deal, GamePrices, Subscription } from "../lib/types";
import { useAccountsUpdate, useGamePrices } from "../hooks/useData";
import { PriceChart } from "./PriceChart";
import { SmallButton } from "./ui";

/** Where a key is made: an "app" on IsThereAnyDeal. */
const KEYS_PAGE = "https://isthereanydeal.com/apps/my/";

/** Offers, bundles and game pages open through IsThereAnyDeal, which forwards to the shop. */
function openLink(url: string) {
  void api.openPriceLink(url).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
}

export function PricesSection({ appid }: { appid: number }) {
  const prices = useGamePrices(appid);
  const error = prices.isError ? toCmdError(prices.error) : null;
  const url = prices.data?.url;
  return (
    <section>
      <div className="mb-3 flex items-center gap-2">
        <BadgePercent size={18} className="text-accent" />
        <h3 className="font-display text-lg font-semibold text-ink-50">{tr.prices.title}</h3>
        {url && (
          <button
            type="button"
            onClick={() => openLink(url)}
            className="ml-auto inline-flex items-center gap-1 text-[13px] text-ink-400 hover:text-white"
          >
            {tr.prices.source}
            <ExternalLink size={12} />
          </button>
        )}
      </div>
      {prices.isLoading ? (
        <div className="shimmer h-40 rounded-xl" />
      ) : error?.kind === "invalid" && error.message === "itad_key" ? (
        // A revoked key: a new one replaces it.
        <KeyPrompt message={errorText(error)} warn />
      ) : error ? (
        <p className="text-sm text-ink-400">
          {tr.prices.loadError}{" "}
          <button type="button" onClick={() => void prices.refetch()} className="text-accent-soft hover:underline">
            {tr.prices.retry}
          </button>
        </p>
      ) : prices.data === null ? (
        <KeyPrompt message={tr.prices.needKey} />
      ) : prices.data && !prices.data.found ? (
        <p className="text-sm text-ink-400">{tr.prices.notFound}</p>
      ) : prices.data ? (
        <Prices data={prices.data} />
      ) : null}
    </section>
  );
}

function KeyPrompt({ message, warn = false }: { message: string; warn?: boolean }) {
  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <p className={clsx("text-[13px] leading-relaxed", warn ? "text-warning" : "text-ink-300")}>{message}</p>
      <ItadKeyForm />
    </div>
  );
}

/** Pasting and checking an IsThereAnyDeal key; shared with Settings. */
export function ItadKeyForm() {
  const update = useAccountsUpdate();
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);

  const save = (e: FormEvent) => {
    e.preventDefault();
    setSaving(true);
    api
      .itadSetKey(key)
      .then((accounts) => {
        setKey("");
        update(accounts);
        showToast({ tone: "success", title: tr.accounts.toastItadSaved });
      })
      .catch((err) => showToast({ tone: "error", title: errorText(toCmdError(err)) }))
      .finally(() => setSaving(false));
  };

  return (
    <form onSubmit={save} className="mt-3 flex flex-wrap items-center gap-2">
      <div className="relative min-w-[240px] flex-1">
        <KeyRound size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-500" />
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={tr.accounts.keyPlaceholder}
          autoComplete="off"
          className="h-9 w-full rounded-lg bg-ink-900 pr-3 pl-8 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/10 outline-none placeholder:font-sans placeholder:text-ink-500 focus:ring-accent/50"
          aria-label={tr.accounts.itadKeyLabel}
        />
      </div>
      <button
        type="submit"
        disabled={!key.trim() || saving}
        className="inline-flex h-9 items-center gap-2 rounded-lg bg-accent/15 px-3.5 text-sm font-semibold text-accent-soft ring-1 ring-accent/35 transition hover:bg-accent/25 disabled:opacity-50"
      >
        {saving && <LoaderCircle size={14} className="animate-spin" />}
        {saving ? tr.accounts.checking : tr.accounts.save}
      </button>
      <LinkButton url={KEYS_PAGE}>{tr.accounts.itadCreate}</LinkButton>
    </form>
  );
}

function Prices({ data }: { data: GamePrices }) {
  const best = data.deals[0] ?? null;
  const lowest = data.lowest;
  const verdict = priceVerdict(best?.price, lowest);
  // Steam's history carries no currency: it is the one Steam sells in here.
  const currency = data.deals.find((d) => d.shop === "Steam")?.price.currency ?? lowest?.price.currency ?? best?.price.currency ?? "USD";

  return (
    <div className="space-y-5">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Tile
          label={tr.prices.bestNow}
          main={best ? formatMoney(best.price) : "—"}
          detail={
            best ? [tr.prices.at(best.shop), best.cut > 0 ? tr.prices.cut(best.cut) : null].filter(Boolean).join(" · ") : tr.prices.noDeals
          }
        />
        <Tile
          label={tr.prices.lowestEver}
          main={lowest ? formatMoney(lowest.price) : "—"}
          detail={lowest ? tr.prices.lowestWhen(lowest.shop, formatDate(lowest.at), lowest.cut) : null}
        />
        <Tile label={tr.prices.lowestYear} main={data.lowestYear ? formatMoney(data.lowestYear) : "—"} />
        <Tile label={tr.prices.lowestMonths} main={data.lowestMonths ? formatMoney(data.lowestMonths) : "—"} />
      </div>

      {verdict && <VerdictLine verdict={verdict} />}
      {data.subscriptions.length > 0 && <Subscriptions subscriptions={data.subscriptions} />}

      <Deals deals={data.deals} />
      {data.bundles.length > 0 && <Bundles bundles={data.bundles} />}

      <div>
        <h4 className="mb-2 text-sm font-semibold text-ink-100">{tr.prices.history}</h4>
        <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
          <PriceChart history={data.history} currency={currency} now={nowSeconds()} />
        </div>
      </div>

      <p className="text-xs leading-relaxed text-ink-500">{tr.prices.note}</p>
    </div>
  );
}

function Tile({ label, main, detail }: { label: string; main: string; detail?: string | null }) {
  return (
    <div className="rounded-xl bg-ink-800/70 px-4 py-3 ring-1 ring-white/6">
      <div className="text-xs text-ink-400">{label}</div>
      <div className="mt-1 text-lg font-semibold text-ink-50 tabular-nums">{main}</div>
      {detail && <div className="mt-0.5 text-xs leading-snug text-ink-400">{detail}</div>}
    </div>
  );
}

function VerdictLine({ verdict }: { verdict: PriceVerdict }) {
  const above = verdict.kind === "above";
  return (
    <div
      className={clsx(
        "flex items-center gap-2.5 rounded-xl px-4 py-2.5 text-sm font-medium ring-1",
        above ? "bg-warning/8 text-warning ring-warning/25" : "bg-success/8 text-success ring-success/25",
      )}
    >
      {above ? <Clock size={16} className="shrink-0" /> : <CircleCheck size={16} className="shrink-0" />}
      {verdict.kind === "lowest"
        ? tr.prices.verdict.lowest
        : verdict.kind === "near"
          ? tr.prices.verdict.near
          : tr.prices.verdict.above(verdict.percent)}
    </div>
  );
}

function Subscriptions({ subscriptions }: { subscriptions: Subscription[] }) {
  return (
    <div className="flex gap-3 rounded-xl bg-accent/8 px-4 py-3 ring-1 ring-accent/25">
      <Sparkles size={17} className="mt-0.5 shrink-0 text-accent-soft" />
      <div className="min-w-0">
        <div className="text-sm font-medium text-ink-50">{tr.prices.subscriptions}</div>
        <ul className="mt-1.5 flex flex-wrap gap-2">
          {subscriptions.map((s) => (
            <li key={s.name} className="rounded-lg bg-white/6 px-2.5 py-1 text-[13px] text-ink-100 ring-1 ring-white/8">
              {s.name}
              {s.leaving != null && <span className="text-ink-400"> · {tr.prices.leaving(formatDate(s.leaving))}</span>}
            </li>
          ))}
        </ul>
        <p className="mt-2 text-xs text-ink-300">{tr.prices.subscriptionsHint}</p>
      </div>
    </div>
  );
}

function Deals({ deals }: { deals: Deal[] }) {
  return (
    <div>
      <h4 className="mb-2 text-sm font-semibold text-ink-100">{tr.prices.deals}</h4>
      {deals.length === 0 ? (
        <p className="text-sm text-ink-400">{tr.prices.noDeals}</p>
      ) : (
        <ul className="divide-y divide-white/6 overflow-hidden rounded-xl bg-ink-800/70 ring-1 ring-white/6">
          {deals.map((d) => (
            <li key={d.url} className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-ink-50">{d.shop}</span>
                  {d.drm.map((drm) => (
                    <span key={drm} className="rounded bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300 ring-1 ring-white/8">
                      {drm}
                    </span>
                  ))}
                </div>
                <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-ink-400">
                  {d.expiry != null && <span>{tr.prices.endsOn(formatDate(d.expiry))}</span>}
                  {d.storeLow && <span>{tr.prices.storeLow(formatMoney(d.storeLow))}</span>}
                </div>
              </div>
              <DealPrice deal={d} />
              <LinkButton url={d.url}>{tr.prices.goToShop}</LinkButton>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function LinkButton({ url, children }: { url: string; children: string }) {
  return (
    <SmallButton onClick={() => openLink(url)} icon={<ExternalLink size={13} />}>
      {children}
    </SmallButton>
  );
}

/** Steam's discount look, as on the game's price tag. */
function DealPrice({ deal }: { deal: Deal }) {
  if (deal.cut <= 0) return <span className="text-sm font-semibold text-ink-50 tabular-nums">{formatMoney(deal.price)}</span>;
  return (
    <span className="inline-flex items-center overflow-hidden rounded-md text-sm font-semibold tabular-nums">
      <span className="bg-[#4c6b22] px-2 py-0.5 text-[#beee11]">-%{deal.cut}</span>
      <span className="flex items-center gap-1.5 bg-white/6 px-2 py-0.5">
        <span className="text-xs text-ink-400 line-through decoration-ink-400/70">{formatMoney(deal.regular)}</span>
        <span className="text-[#beee11]">{formatMoney(deal.price)}</span>
      </span>
    </span>
  );
}

function Bundles({ bundles }: { bundles: Bundle[] }) {
  return (
    <div>
      <h4 className="mb-2 text-sm font-semibold text-ink-100">{tr.prices.bundles}</h4>
      <ul className="divide-y divide-white/6 overflow-hidden rounded-xl bg-ink-800/70 ring-1 ring-white/6">
        {bundles.map((b) => (
          <li key={`${b.store}-${b.title}`} className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
            <Package size={16} className="shrink-0 text-ink-400" />
            <div className="min-w-0 flex-1">
              <div className="font-medium text-ink-50">{b.title}</div>
              <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-ink-400">
                <span>{b.store}</span>
                {b.price && <span>{tr.prices.bundleTier(formatMoney(b.price))}</span>}
                {b.expiry != null && <span>{tr.prices.endsOn(formatDate(b.expiry))}</span>}
              </div>
            </div>
            {b.url && <LinkButton url={b.url}>{tr.prices.seeBundle}</LinkButton>}
          </li>
        ))}
      </ul>
    </div>
  );
}
