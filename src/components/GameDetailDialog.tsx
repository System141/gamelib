import clsx from "clsx";
import { ChevronLeft, ChevronRight, Clapperboard, Download, ExternalLink, ImageOff, Play, TriangleAlert, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatDate, formatPercent, formatRelative, isRecent } from "../lib/format";
import { showToast } from "../lib/toast";
import type { GameDetail, Screenshot, Trailer } from "../lib/types";
import { useGame, useGameMedia, useInstalls } from "../hooks/useData";
import { DeckBadge, PlatformIcons, PriceTag, ReviewBadge } from "./badges";
import { GameArt } from "./GameArt";
import { SteamIcon } from "./icons";
import { LinksSection } from "./LinksSection";
import { useInstallActions } from "./InstallActions";
import { ReviewsSection } from "./ReviewsSection";
import { StoresSection } from "./StoresSection";
import { TrailerPlayer } from "./TrailerPlayer";

/** Descriptors whose "mature" screenshots stay hidden unless adult content is enabled. */
const SEXUAL_DESCRIPTORS = [1, 3, 4];

interface Props {
  appid: number | null;
  onClose: () => void;
  tagName: (tagid: number) => string | undefined;
  onTagClick: (tagid: number) => void;
  showAdult: boolean;
  onStoreSync: () => void;
}

export function GameDetailDialog({ appid, onClose, tagName, onTagClick, showAdult, onStoreSync }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const game = useGame(appid);
  const [viewer, setViewer] = useState<number | null>(null);
  const [trailer, setTrailer] = useState<number | null>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (appid != null && !dialog.open) dialog.showModal();
    if (appid == null && dialog.open) dialog.close();
    setViewer(null);
    setTrailer(null);
    dialog.querySelector("[data-scroll]")?.scrollTo({ top: 0 });
  }, [appid]);

  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onCancel={(e) => {
        // Escape closes the image viewer or the trailer first.
        if (viewer != null || trailer != null) {
          e.preventDefault();
          setViewer(null);
          setTrailer(null);
        }
      }}
      onClick={(e) => e.target === ref.current && onClose()}
      className="m-auto h-[min(900px,92vh)] w-[min(1140px,94vw)] max-w-none overflow-hidden rounded-2xl bg-ink-850 p-0 text-ink-100 shadow-2xl shadow-black ring-1 ring-white/10 open:animate-rise"
      aria-label={game.data?.name}
    >
      <button
        type="button"
        onClick={onClose}
        className="glass absolute top-4 right-4 z-20 grid size-10 place-items-center rounded-full text-ink-100 ring-1 ring-white/15 transition hover:text-white hover:ring-white/30"
        aria-label={tr.detail.close}
      >
        <X size={18} />
      </button>
      <div data-scroll className="relative h-full overflow-y-auto">
        {game.data ? (
          <Detail
            game={game.data}
            tagName={tagName}
            onTagClick={onTagClick}
            showAdult={showAdult}
            viewer={viewer}
            setViewer={setViewer}
            trailer={trailer}
            setTrailer={setTrailer}
            onStoreSync={onStoreSync}
          />
        ) : game.isError ? (
          <div className="grid h-full place-items-center text-ink-300">{tr.detail.loadError}</div>
        ) : (
          <DetailSkeleton />
        )}
      </div>
    </dialog>
  );
}

function Detail({
  game,
  tagName,
  onTagClick,
  showAdult,
  viewer,
  setViewer,
  trailer,
  setTrailer,
  onStoreSync,
}: {
  game: GameDetail;
  tagName: (tagid: number) => string | undefined;
  onTagClick: (tagid: number) => void;
  showAdult: boolean;
  viewer: number | null;
  setViewer: (i: number | null) => void;
  trailer: number | null;
  setTrailer: (i: number | null) => void;
  onStoreSync: () => void;
}) {
  const media = useGameMedia(game.appid);
  const allowMature = showAdult || !game.descriptors.some((d) => SEXUAL_DESCRIPTORS.includes(d));
  const shots = (media.data?.screenshots ?? []).filter((s) => allowMature || !s.mature);
  const trailers = (media.data?.trailers ?? []).filter((t) => allowMature || !t.mature);
  const description = media.data?.descriptionTr ?? game.shortDescription;
  const englishOnly = media.isSuccess && !media.data?.descriptionTr && !!game.shortDescription;
  const backdrop = game.hero ?? game.header ?? game.capsule;

  const openSteam = (target: "web" | "client" | "install") =>
    api.openInSteam(game.appid, target).catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <>
      {/* Hero */}
      <div className="relative h-[340px] overflow-hidden">
        {backdrop && (
          <img
            src={backdrop}
            alt=""
            aria-hidden
            className={clsx("absolute inset-0 size-full object-cover", !game.hero && "scale-110 opacity-70 blur-2xl")}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-ink-850 via-ink-850/55 to-ink-850/5" />
        <div className="absolute inset-0 bg-gradient-to-r from-ink-850/80 via-transparent to-transparent" />
      </div>

      <div className="relative -mt-52 flex gap-7 px-8">
        <div className="relative aspect-[2/3] w-48 shrink-0 overflow-hidden rounded-xl shadow-2xl shadow-black/70 ring-1 ring-white/15">
          <GameArt game={game} eager />
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-end pb-1">
          <div className="flex flex-wrap items-center gap-2">
            {isRecent(game.releaseDate, 7) && (
              <span className="rounded-full bg-gradient-to-r from-accent to-violet px-2.5 py-0.5 text-[11px] font-bold text-ink-950">
                {tr.detail.new}
              </span>
            )}
            {game.isEarlyAccess && (
              <span className="rounded-full bg-warning/15 px-2.5 py-0.5 text-[11px] font-semibold text-warning ring-1 ring-warning/30">
                {tr.detail.earlyAccess}
              </span>
            )}
          </div>
          <h2 className="mt-2 font-display text-[40px] leading-[1.05] font-semibold tracking-tight text-white drop-shadow-lg">
            {game.name}
          </h2>
          {(game.developers.length > 0 || game.publishers.length > 0) && (
            <p className="mt-2 truncate text-sm text-ink-300">{[...new Set([...game.developers, ...game.publishers])].join(" · ")}</p>
          )}
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <ReviewBadge score={game.reviewScore} count={game.reviewCount} />
            {game.reviewCount > 0 && (
              <span className="text-sm text-ink-300 tabular-nums">
                {formatPercent(game.reviewPct)} · {tr.detail.reviewCount(game.reviewCount)}
              </span>
            )}
            <DeckBadge deck={game.deck} />
          </div>
          <div className="mt-5 flex flex-wrap items-center gap-3">
            <PriceTag game={game} size="lg" />
            <PlayInstalled appid={game.appid} />
            <button
              type="button"
              onClick={() => openSteam("web")}
              className="inline-flex h-10 items-center gap-2 rounded-lg bg-gradient-to-r from-accent-strong to-violet-strong px-4 text-sm font-semibold text-white shadow-lg shadow-accent/20 transition hover:brightness-110"
            >
              <ExternalLink size={16} />
              {tr.detail.openInSteam}
            </button>
            <button
              type="button"
              onClick={() => openSteam("client")}
              className="inline-flex h-10 items-center gap-2 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10"
            >
              <SteamIcon size={16} />
              {tr.detail.openInClient}
            </button>
            <button
              type="button"
              onClick={() => openSteam("install")}
              title={tr.detail.installWithSteamHint}
              className="inline-flex h-10 items-center gap-2 rounded-lg bg-white/6 px-4 text-sm font-medium text-ink-100 ring-1 ring-white/10 transition hover:bg-white/10"
            >
              <Download size={16} />
              {tr.detail.installWithSteam}
            </button>
          </div>
        </div>
      </div>

      {game.delisted && (
        <div className="mx-8 mt-6 flex items-center gap-2 rounded-xl bg-warning/10 px-4 py-3 text-sm text-warning ring-1 ring-warning/25">
          <TriangleAlert size={16} />
          {tr.detail.delisted}
        </div>
      )}

      <div className="grid gap-8 px-8 pt-8 pb-10 lg:grid-cols-[minmax(0,1fr)_320px]">
        <div className="min-w-0 space-y-9">
          <section>
            <h3 className="mb-2 font-display text-lg font-semibold text-ink-50">{tr.detail.about}</h3>
            {description ? (
              <p className="max-w-3xl text-[15px] leading-relaxed text-ink-200">{description}</p>
            ) : (
              <p className="text-sm text-ink-400">{tr.detail.noDescription}</p>
            )}
            {englishOnly && <p className="mt-2 text-xs text-ink-500">{tr.detail.englishDescription}</p>}
          </section>

          <section>
            <div className="mb-3 flex items-center gap-2">
              <h3 className="font-display text-lg font-semibold text-ink-50">
                {trailers.length > 0 ? tr.media.title : tr.detail.screenshots}
              </h3>
              <button
                type="button"
                onClick={() =>
                  void api
                    .openSearch("youtube", `${game.name} gameplay`)
                    .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }))
                }
                className="ml-auto inline-flex items-center gap-1.5 text-[13px] text-ink-400 hover:text-white"
              >
                <Clapperboard size={14} />
                {tr.media.gameplay}
                <ExternalLink size={12} />
              </button>
            </div>
            <MediaStrip
              trailers={trailers}
              shots={shots}
              loading={media.isLoading}
              failed={media.isError}
              onOpen={setViewer}
              onPlay={setTrailer}
            />
          </section>

          <ReviewsSection appid={game.appid} summaries={media.data?.reviews} onOpenSteam={() => void openSteam("web")} />

          <StoresSection appid={game.appid} onStoreSync={onStoreSync} />

          <LinksSection appid={game.appid} gameTitle={game.name} />
        </div>

        <aside className="h-fit space-y-4 rounded-2xl bg-ink-800/70 p-5 ring-1 ring-white/6">
          <h3 className="text-xs font-semibold tracking-wider text-ink-400 uppercase">{tr.detail.info}</h3>
          <InfoRow label={tr.detail.releaseDate}>{formatDate(game.releaseDate)}</InfoRow>
          {game.developers.length > 0 && <InfoRow label={tr.detail.developer}>{game.developers.join(", ")}</InfoRow>}
          {game.publishers.length > 0 && <InfoRow label={tr.detail.publisher}>{game.publishers.join(", ")}</InfoRow>}
          {game.franchises.length > 0 && <InfoRow label={tr.detail.franchise}>{game.franchises.join(", ")}</InfoRow>}
          <InfoRow label={tr.detail.platforms}>
            <span className="inline-flex items-center gap-2">
              <PlatformIcons game={game} size={14} className="text-ink-200" />
              <span>
                {[game.win && tr.platforms.win, game.mac && tr.platforms.mac, game.linux && tr.platforms.linux].filter(Boolean).join(", ")}
              </span>
            </span>
          </InfoRow>
          {game.deck > 0 && (
            <InfoRow label={tr.detail.deck}>
              <DeckBadge deck={game.deck} compact />
            </InfoRow>
          )}
          <InfoRow label={tr.detail.price}>
            {game.isFree ? tr.detail.free : (game.price ?? tr.detail.noPrice)}
            {game.price && <span className="mt-1 block text-[11px] leading-snug text-ink-500">{tr.detail.priceNote}</span>}
          </InfoRow>
          {game.tags.length > 0 && (
            <div>
              <div className="mb-2 text-xs text-ink-400">{tr.detail.tags}</div>
              <div className="flex flex-wrap gap-1.5">
                {game.tags.map((t) => {
                  const name = tagName(t);
                  return name ? (
                    <button
                      key={t}
                      type="button"
                      onClick={() => onTagClick(t)}
                      className="rounded-md bg-white/6 px-2 py-1 text-xs text-ink-200 ring-1 ring-white/6 transition hover:bg-accent/15 hover:text-accent-soft hover:ring-accent/35"
                    >
                      {name}
                    </button>
                  ) : null;
                })}
              </div>
              <p className="mt-2 text-[11px] text-ink-500">{tr.detail.tagHint}</p>
            </div>
          )}
          <p className="border-t border-white/6 pt-3 text-[11px] text-ink-500">
            #{game.appid} · {tr.detail.lastUpdated(formatRelative(game.syncedAt))}
          </p>
        </aside>
      </div>

      {viewer != null && shots[viewer] && <Viewer shots={shots} index={viewer} onChange={setViewer} />}
      {trailer != null && trailers[trailer] && (
        <TrailerPlayer trailer={trailers[trailer]} onClose={() => setTrailer(null)} onOpenSteam={() => void openSteam("web")} />
      )}
    </>
  );
}

function InfoRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="text-xs text-ink-400">{label}</div>
      <div className="mt-0.5 text-sm text-ink-100">{children}</div>
    </div>
  );
}

function MediaStrip({
  trailers,
  shots,
  loading,
  failed,
  onOpen,
  onPlay,
}: {
  trailers: Trailer[];
  shots: Screenshot[];
  loading: boolean;
  failed: boolean;
  onOpen: (i: number) => void;
  onPlay: (i: number) => void;
}) {
  if (loading) {
    return (
      <div className="flex gap-3 overflow-hidden">
        {[0, 1, 2].map((i) => (
          <div key={i} className="shimmer aspect-video w-72 shrink-0 rounded-xl" />
        ))}
      </div>
    );
  }
  if (failed || shots.length + trailers.length === 0) {
    return (
      <div className="flex items-center gap-2 rounded-xl bg-white/3 px-4 py-6 text-sm text-ink-400 ring-1 ring-white/6">
        <ImageOff size={16} />
        {failed ? tr.detail.mediaError : tr.detail.noScreenshots}
      </div>
    );
  }
  return (
    <div className="-mx-1 flex snap-x gap-3 overflow-x-auto px-1 pb-3">
      {trailers.map((t, i) => (
        <button
          key={t.stream}
          type="button"
          onClick={() => onPlay(i)}
          aria-label={tr.media.play(t.name || tr.media.trailer)}
          className="group relative aspect-video w-72 shrink-0 snap-start overflow-hidden rounded-xl bg-ink-800 ring-1 ring-white/8 transition hover:ring-accent/50"
        >
          {t.poster && (
            <img
              src={t.poster}
              alt=""
              loading="lazy"
              decoding="async"
              className="size-full object-cover transition duration-500 group-hover:scale-105"
            />
          )}
          <span className="absolute inset-0 grid place-items-center">
            <span className="glass grid size-12 place-items-center rounded-full ring-1 ring-white/25 transition group-hover:scale-110 group-hover:ring-white/50">
              <Play size={20} className="translate-x-px fill-white text-white" />
            </span>
          </span>
          <span className="absolute inset-x-0 bottom-0 truncate bg-gradient-to-t from-black/85 to-transparent px-3 pt-6 pb-2 text-left text-xs text-ink-100">
            {t.name || tr.media.trailer}
          </span>
        </button>
      ))}
      {shots.map((s, i) => (
        <button
          key={s.thumb}
          type="button"
          onClick={() => onOpen(i)}
          className="group relative aspect-video w-72 shrink-0 snap-start overflow-hidden rounded-xl bg-ink-800 ring-1 ring-white/8 transition hover:ring-accent/50"
        >
          <img
            src={s.thumb}
            alt=""
            loading="lazy"
            decoding="async"
            className="size-full object-cover transition duration-500 group-hover:scale-105"
          />
        </button>
      ))}
    </div>
  );
}

function Viewer({ shots, index, onChange }: { shots: Screenshot[]; index: number; onChange: (i: number | null) => void }) {
  const go = useCallback((delta: number) => onChange((index + delta + shots.length) % shots.length), [index, shots.length, onChange]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") go(1);
      if (e.key === "ArrowLeft") go(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go]);

  const shot = shots[index]!;
  return (
    <div className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-ink-950/95 p-10" onClick={() => onChange(null)}>
      <img
        src={shot.full}
        alt=""
        className="max-h-full max-w-full rounded-lg shadow-2xl shadow-black"
        onClick={(e) => e.stopPropagation()}
      />
      <button
        type="button"
        onClick={(e) => (e.stopPropagation(), go(-1))}
        className="glass absolute left-6 grid size-12 place-items-center rounded-full ring-1 ring-white/15 hover:ring-white/40"
        aria-label={tr.detail.viewerPrev}
      >
        <ChevronLeft size={22} />
      </button>
      <button
        type="button"
        onClick={(e) => (e.stopPropagation(), go(1))}
        className="glass absolute right-6 grid size-12 place-items-center rounded-full ring-1 ring-white/15 hover:ring-white/40"
        aria-label={tr.detail.viewerNext}
      >
        <ChevronRight size={22} />
      </button>
      <div className="glass absolute bottom-6 rounded-full px-4 py-1.5 text-sm text-ink-200 ring-1 ring-white/10">
        {tr.detail.viewerCounter(index + 1, shots.length)}
      </div>
      <button
        type="button"
        onClick={() => onChange(null)}
        className="glass absolute top-6 right-6 grid size-10 place-items-center rounded-full ring-1 ring-white/15 hover:ring-white/40"
        aria-label={tr.detail.close}
      >
        <X size={18} />
      </button>
    </div>
  );
}

function DetailSkeleton() {
  return (
    <div>
      <div className="shimmer h-[340px]" />
      <div className="-mt-52 flex gap-7 px-8">
        <div className="shimmer aspect-[2/3] w-48 rounded-xl ring-1 ring-white/10" />
        <div className="flex flex-1 flex-col justify-end gap-3 pb-2">
          <div className="shimmer h-10 w-2/3 rounded-lg" />
          <div className="shimmer h-4 w-1/3 rounded" />
          <div className="shimmer h-8 w-1/2 rounded-full" />
        </div>
      </div>
      <div className="space-y-3 px-8 pt-10">
        <div className="shimmer h-4 w-full rounded" />
        <div className="shimmer h-4 w-5/6 rounded" />
        <div className="shimmer h-4 w-3/4 rounded" />
      </div>
    </div>
  );
}

/** "Oyna" for a game installed from another store. */
function PlayInstalled({ appid }: { appid: number }) {
  const installs = useInstalls();
  const act = useInstallActions();
  const game = installs.data?.find((i) => i.appid === appid && i.exe);
  if (!game) return null;
  return (
    <button
      type="button"
      onClick={() => void act.play(game)}
      title={`${tr.install.play} · ${tr.storeNames[game.store]}`}
      className="inline-flex h-10 items-center gap-2 rounded-lg bg-success px-4 text-sm font-semibold text-ink-950 shadow-lg shadow-success/20 transition hover:brightness-110"
    >
      <Play size={16} fill="currentColor" />
      {tr.install.play}
    </button>
  );
}
