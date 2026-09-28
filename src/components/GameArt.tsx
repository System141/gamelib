// Cover art with a fallback chain:
//   1. portrait library capsule (1x/2x)
//   2. landscape header centred over a blurred copy of itself
//   3. gradient placeholder with the game's name

import clsx from "clsx";
import { useState } from "react";
import type { GameCard } from "../lib/types";

/** URLs that already failed (404, network) — never retried in this session. */
const failed = new Set<string>();

type ArtGame = Pick<GameCard, "appid" | "name" | "capsule" | "capsule2x" | "header">;

export function GameArt({ game, className, eager = false }: { game: ArtGame; className?: string; eager?: boolean }) {
  // Keyed by app id so a recycled component starts over for a different game.
  return <ArtInner key={game.appid} game={game} className={className} eager={eager} />;
}

function ArtInner({ game, className, eager }: { game: ArtGame; className?: string; eager: boolean }) {
  const usable = (url: string | null) => (url && !failed.has(url) ? url : null);
  const capsule = usable(game.capsule);
  const header = usable(game.header);
  const [stage, setStage] = useState<"capsule" | "header" | "placeholder">(capsule ? "capsule" : header ? "header" : "placeholder");
  const [loaded, setLoaded] = useState(false);

  const fail = (url: string, next: typeof stage) => {
    failed.add(url);
    setLoaded(false);
    setStage(next);
  };

  return (
    <div className={clsx("absolute inset-0 overflow-hidden", className)}>
      {!loaded && stage !== "placeholder" && <div className="shimmer absolute inset-0" />}

      {stage === "capsule" && capsule && (
        <img
          src={capsule}
          srcSet={game.capsule2x && !failed.has(game.capsule2x) ? `${capsule} 1x, ${game.capsule2x} 2x` : undefined}
          alt={game.name}
          loading={eager ? "eager" : "lazy"}
          decoding="async"
          draggable={false}
          onLoad={() => setLoaded(true)}
          onError={() => fail(capsule, header ? "header" : "placeholder")}
          className={clsx("absolute inset-0 size-full object-cover transition-opacity duration-500", loaded ? "opacity-100" : "opacity-0")}
        />
      )}

      {stage === "header" && header && (
        <>
          <img
            src={header}
            alt=""
            aria-hidden
            className="absolute inset-0 size-full scale-125 object-cover opacity-50 blur-2xl saturate-150"
          />
          <div className="absolute inset-0 bg-gradient-to-b from-ink-950/40 via-transparent to-ink-950/60" />
          <img
            src={header}
            alt={game.name}
            loading={eager ? "eager" : "lazy"}
            decoding="async"
            draggable={false}
            onLoad={() => setLoaded(true)}
            onError={() => fail(header, "placeholder")}
            className={clsx(
              "absolute inset-x-0 top-1/2 w-full -translate-y-1/2 object-contain shadow-2xl shadow-black/60 transition-opacity duration-500",
              loaded ? "opacity-100" : "opacity-0",
            )}
          />
        </>
      )}

      {stage === "placeholder" && <Placeholder game={game} />}
    </div>
  );
}

function Placeholder({ game }: { game: ArtGame }) {
  const hue = (game.appid * 47) % 360;
  return (
    <div
      className="absolute inset-0 flex items-end p-4"
      style={{
        background: `radial-gradient(120% 80% at 20% 10%, hsl(${hue} 70% 45% / 0.55), transparent 60%), linear-gradient(160deg, hsl(${(hue + 40) % 360} 45% 22%), hsl(${(hue + 200) % 360} 40% 10%))`,
      }}
    >
      <span className="line-clamp-4 font-display text-lg leading-tight font-semibold text-white/90 drop-shadow">{game.name}</span>
    </div>
  );
}
