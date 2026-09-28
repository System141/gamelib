import clsx from "clsx";
import { Link2 } from "lucide-react";
import { memo } from "react";
import { tr } from "../i18n/tr";
import { formatRelative, formatYear, isRecent } from "../lib/format";
import type { GameCard as Game } from "../lib/types";
import { Pill, PlatformIcons, PriceTag, ReviewDot } from "./badges";
import { GameArt } from "./GameArt";

interface Props {
  game: Game;
  tagName: (tagid: number) => string | undefined;
  onOpen: (appid: number) => void;
  /** Show "3 gün önce" instead of the year (new releases view). */
  relativeDate?: boolean;
}

export const GameCard = memo(function GameCard({ game, tagName, onOpen, relativeDate = false }: Props) {
  const fresh = isRecent(game.releaseDate, 7);
  const tags = game.topTags.map(tagName).filter(Boolean) as string[];

  return (
    <button
      type="button"
      onClick={() => onOpen(game.appid)}
      className="group relative flex w-full min-w-0 flex-col text-left outline-none"
      aria-label={game.name}
    >
      <div
        className={clsx(
          "relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-ink-800 shadow-lg shadow-black/40 ring-1 ring-white/6",
          "transition-[transform,box-shadow] duration-300 ease-out will-change-transform",
          "group-hover:-translate-y-1.5 group-hover:card-glow group-focus-visible:-translate-y-1.5 group-focus-visible:card-glow",
        )}
      >
        <GameArt game={game} />

        <div className="absolute inset-x-0 top-0 flex items-start justify-between gap-1 p-2">
          <div className="flex flex-wrap gap-1">
            {fresh && <Pill className="bg-gradient-to-r from-accent to-violet text-ink-950 shadow-md shadow-black/40">{tr.card.new}</Pill>}
            {game.isEarlyAccess && (
              <Pill className="bg-ink-950/75 text-warning ring-1 ring-warning/30 backdrop-blur">{tr.card.earlyAccess}</Pill>
            )}
          </div>
          {game.discountPct > 0 && <Pill className="bg-[#4c6b22] text-[#beee11] shadow-md shadow-black/40">-%{game.discountPct}</Pill>}
        </div>

        {game.linkCount > 0 && (
          <span
            className="absolute right-2 bottom-2 inline-flex items-center gap-1 rounded-full bg-ink-950/80 px-2 py-0.5 text-[11px] font-semibold text-accent-soft ring-1 ring-accent/30 backdrop-blur transition-opacity group-hover:opacity-0"
            title={tr.card.links(game.linkCount)}
          >
            <Link2 size={12} />
            {game.linkCount}
          </span>
        )}

        <div className="pointer-events-none absolute inset-x-0 bottom-0 translate-y-3 bg-gradient-to-t from-black/95 via-black/70 to-transparent px-3 pt-12 pb-3 opacity-0 transition duration-300 ease-out group-hover:translate-y-0 group-hover:opacity-100 group-focus-visible:translate-y-0 group-focus-visible:opacity-100">
          {tags.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {tags.map((t) => (
                <span key={t} className="rounded-md bg-white/12 px-1.5 py-0.5 text-[10.5px] font-medium text-ink-50 backdrop-blur">
                  {t}
                </span>
              ))}
            </div>
          )}
          <PlatformIcons game={game} className="mt-2 text-ink-200" size={12} />
        </div>
      </div>

      <div className="mt-2.5 min-w-0 px-0.5">
        <div
          className="truncate text-[13.5px] leading-5 font-semibold text-ink-50 transition-colors group-hover:text-white"
          title={game.name}
        >
          {game.name}
        </div>
        <div className="mt-1 flex h-5 items-center justify-between gap-2 text-xs text-ink-300">
          <span className="flex min-w-0 items-center gap-1.5">
            <ReviewDot game={game} />
            <span className="text-ink-600">•</span>
            <span className="truncate">{relativeDate ? formatRelative(game.releaseDate) : formatYear(game.releaseDate)}</span>
          </span>
          <PriceTag game={game} compact />
        </div>
      </div>
    </button>
  );
});

export function SkeletonCard() {
  return (
    <div className="flex w-full flex-col" aria-hidden>
      <div className="shimmer aspect-[2/3] w-full rounded-xl ring-1 ring-white/5" />
      <div className="shimmer mt-3 h-3.5 w-4/5 rounded" />
      <div className="shimmer mt-2 h-3 w-2/5 rounded" />
    </div>
  );
}
