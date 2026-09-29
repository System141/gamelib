// Pure helpers for the prices section: money, the price history as steps, and how a price
// compares with the lowest ever.

import type { LowestPrice, Money, PricePoint } from "./types";

const moneyFormats = new Map<string, Intl.NumberFormat>();

/** "$9,99", "₺299,00". */
export function formatMoney(money: Money): string {
  let format = moneyFormats.get(money.currency);
  if (!format) {
    try {
      format = new Intl.NumberFormat("tr-TR", { style: "currency", currency: money.currency });
    } catch {
      format = new Intl.NumberFormat("tr-TR", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    }
    moneyFormats.set(money.currency, format);
  }
  return format.format(money.amount);
}

export interface PriceStep {
  from: number;
  to: number;
  price: number;
  regular: number;
  cut: number;
}

/** Each price change holds until the next one; the last until `now`. */
export function priceSteps(history: PricePoint[], now: number): PriceStep[] {
  return history.map((p, i) => ({
    from: p.at,
    to: Math.max(p.at, history[i + 1]?.at ?? now),
    price: p.price,
    regular: p.regular,
    cut: p.cut,
  }));
}

export interface PriceVerdict {
  kind: "lowest" | "near" | "above";
  /** How far above the lowest ever, in whole percent. */
  percent: number;
}

/** How the best price now compares with the lowest ever: at it, within 15% of it, or above. Null
 *  when they can't be compared: other currencies, or the game was once given away. */
export function priceVerdict(best: Money | null | undefined, lowest: LowestPrice | null | undefined): PriceVerdict | null {
  if (!best || !lowest || best.currency !== lowest.price.currency) return null;
  const low = lowest.price.amount;
  if (best.amount <= low + 0.005) return { kind: "lowest", percent: 0 };
  if (low <= 0) return null;
  const percent = Math.round((best.amount / low - 1) * 100);
  return { kind: percent <= 15 ? "near" : "above", percent };
}
