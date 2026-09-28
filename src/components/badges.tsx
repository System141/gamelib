import clsx from "clsx";
import type { ReactNode } from "react";
import { deckLabel, reviewLabel, tr } from "../i18n/tr";
import { formatPercent, reviewTone, type ReviewTone } from "../lib/format";
import type { DeckCompat, GameCard } from "../lib/types";
import { AppleIcon, LinuxIcon, SteamIcon, WindowsIcon } from "./icons";

const TONE_TEXT: Record<ReviewTone, string> = {
  pos: "text-review-pos",
  mixed: "text-review-mixed",
  neg: "text-review-neg",
  none: "text-ink-400",
};

const TONE_DOT: Record<ReviewTone, string> = {
  pos: "bg-review-pos shadow-[0_0_10px] shadow-review-pos/60",
  mixed: "bg-review-mixed shadow-[0_0_10px] shadow-review-mixed/50",
  neg: "bg-review-neg shadow-[0_0_10px] shadow-review-neg/50",
  none: "bg-ink-500",
};

/** Compact review indicator for cards: coloured dot and "%96". */
export function ReviewDot({ game }: { game: Pick<GameCard, "reviewScore" | "reviewPct" | "reviewCount"> }) {
  const tone = reviewTone(game.reviewScore);
  return (
    <span className={clsx("inline-flex items-center gap-1.5 tabular-nums", TONE_TEXT[tone])} title={reviewLabel(game.reviewScore, game.reviewCount)}>
      <span className={clsx("size-1.5 rounded-full", TONE_DOT[tone])} />
      {game.reviewCount > 0 ? formatPercent(game.reviewPct) : "—"}
    </span>
  );
}

/** Full review badge for the detail view. */
export function ReviewBadge({ score, count }: { score: number; count: number }) {
  const tone = reviewTone(score);
  return (
    <span
      className={clsx(
        "inline-flex items-center gap-2 rounded-full px-3 py-1 text-sm font-medium ring-1",
        tone === "pos" && "bg-review-pos/10 ring-review-pos/30",
        tone === "mixed" && "bg-review-mixed/10 ring-review-mixed/30",
        tone === "neg" && "bg-review-neg/10 ring-review-neg/30",
        tone === "none" && "bg-white/5 ring-white/10",
        TONE_TEXT[tone],
      )}
    >
      <span className={clsx("size-2 rounded-full", TONE_DOT[tone])} />
      {reviewLabel(score, count)}
    </span>
  );
}

export function DeckBadge({ deck, compact = false }: { deck: DeckCompat; compact?: boolean }) {
  if (deck === 0) return null;
  const color = deck === 3 ? "text-deck-verified" : deck === 2 ? "text-deck-playable" : "text-ink-400";
  return (
    <span
      className={clsx("inline-flex items-center gap-1.5 rounded-full bg-white/5 font-medium ring-1 ring-white/10", compact ? "px-2 py-0.5 text-xs" : "px-3 py-1 text-sm")}
      title={`${tr.detail.deck}: ${deckLabel(deck)}`}
    >
      <SteamIcon size={compact ? 12 : 14} className={color} />
      <span className={color}>{deckLabel(deck)}</span>
    </span>
  );
}

export function PlatformIcons({ game, size = 13, className }: { game: Pick<GameCard, "win" | "mac" | "linux">; size?: number; className?: string }) {
  return (
    <span className={clsx("inline-flex items-center gap-1.5", className)}>
      {game.win && <WindowsIcon size={size} aria-label={tr.platforms.win} />}
      {game.mac && <AppleIcon size={size} aria-label={tr.platforms.mac} />}
      {game.linux && <LinuxIcon size={size} aria-label={tr.platforms.linux} />}
    </span>
  );
}

/** Price chip: free, discounted (with old price) or regular. */
export function PriceTag({
  game,
  size = "sm",
  compact = false,
}: {
  game: Pick<GameCard, "isFree" | "price" | "originalPrice" | "discountPct">;
  size?: "sm" | "lg";
  /** Cards: final price only (the discount is already shown on the cover). */
  compact?: boolean;
}) {
  const big = size === "lg";
  if (game.isFree) {
    return <span className={clsx("shrink-0 rounded-md bg-accent/12 font-semibold whitespace-nowrap text-accent-soft", big ? "px-3 py-1 text-base" : "px-1.5 py-0.5 text-xs")}>{tr.card.free}</span>;
  }
  if (!game.price) return null;
  if (game.discountPct > 0 && compact) {
    return <span className="shrink-0 rounded-md bg-[#4c6b22]/70 px-1.5 py-0.5 text-xs font-semibold whitespace-nowrap text-[#beee11]">{game.price}</span>;
  }
  if (game.discountPct > 0) {
    return (
      <span className={clsx("inline-flex items-center overflow-hidden rounded-md font-semibold", big ? "text-base" : "text-xs")}>
        <span className={clsx("bg-[#4c6b22] text-[#beee11]", big ? "px-2.5 py-1" : "px-1.5 py-0.5")}>-%{game.discountPct}</span>
        <span className={clsx("flex items-center gap-1.5 bg-white/6", big ? "px-2.5 py-1" : "px-1.5 py-0.5")}>
          {game.originalPrice && <span className="text-ink-400 line-through decoration-ink-400/70">{game.originalPrice}</span>}
          <span className="text-[#beee11]">{game.price}</span>
        </span>
      </span>
    );
  }
  return <span className={clsx("shrink-0 rounded-md bg-white/6 font-semibold whitespace-nowrap text-ink-100", big ? "px-3 py-1 text-base" : "px-1.5 py-0.5 text-xs")}>{game.price}</span>;
}

export function Pill({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={clsx("inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold tracking-wide", className)}>{children}</span>;
}
