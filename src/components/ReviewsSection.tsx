// "İncelemeler": Steam's review summaries (all languages, Turkish, the latest 100) and the most
// helpful reviews of the past year, Turkish ones first.

import clsx from "clsx";
import { ExternalLink, MessageSquareText, ThumbsDown, ThumbsUp } from "lucide-react";
import { useState } from "react";
import { reviewLabel, tr } from "../i18n/tr";
import { formatDate, type ReviewTone, reviewTone } from "../lib/format";
import type { RecentReviews, Review, ReviewScore, ReviewSummaries } from "../lib/types";
import { useGameReviews } from "../hooks/useData";

const TONE_TEXT: Record<ReviewTone, string> = {
  pos: "text-review-pos",
  mixed: "text-review-mixed",
  neg: "text-review-neg",
  none: "text-ink-300",
};

/** Steam's bands for a share of positive reviews: 70% and up positive, 40–69% mixed. */
function percentTone(percent: number): ReviewTone {
  if (percent >= 70) return "pos";
  if (percent >= 40) return "mixed";
  return "neg";
}

export function ReviewsSection({
  appid,
  summaries,
  onOpenSteam,
}: {
  appid: number;
  summaries: ReviewSummaries | null | undefined;
  onOpenSteam: () => void;
}) {
  const reviews = useGameReviews(appid);
  return (
    <section>
      <div className="mb-3 flex items-center gap-2">
        <MessageSquareText size={18} className="text-accent" />
        <h3 className="font-display text-lg font-semibold text-ink-50">{tr.reviews.title}</h3>
        <button
          type="button"
          onClick={onOpenSteam}
          className="ml-auto inline-flex items-center gap-1 text-[13px] text-ink-400 hover:text-white"
        >
          {tr.reviews.seeAll}
          <ExternalLink size={12} />
        </button>
      </div>

      <div className="grid gap-3 sm:grid-cols-3">
        <ScoreTile label={tr.reviews.all} score={summaries?.all ?? null} />
        <ScoreTile label={tr.reviews.turkish} score={summaries?.turkish ?? null} empty={tr.reviews.noTurkish} />
        <RecentTile recent={reviews.data?.recent ?? null} loading={reviews.isLoading} />
      </div>

      <div className="mt-4 space-y-3">
        {reviews.isLoading ? (
          [0, 1].map((i) => <div key={i} className="shimmer h-28 rounded-xl" />)
        ) : reviews.isError ? (
          <p className="text-sm text-ink-400">
            {tr.reviews.loadError}{" "}
            <button type="button" onClick={() => void reviews.refetch()} className="text-accent-soft hover:underline">
              {tr.reviews.retry}
            </button>
          </p>
        ) : reviews.data && reviews.data.top.length > 0 ? (
          reviews.data.top.map((r) => <ReviewCard key={r.id} review={r} />)
        ) : (
          <p className="text-sm text-ink-400">{tr.reviews.none}</p>
        )}
      </div>
    </section>
  );
}

function Tile({ label, tone, main, detail }: { label: string; tone: ReviewTone; main: string; detail: string }) {
  return (
    <div className="rounded-xl bg-ink-800/70 px-4 py-3 ring-1 ring-white/6">
      <div className="text-xs text-ink-400">{label}</div>
      <div className={clsx("mt-1 font-medium", TONE_TEXT[tone])}>{main}</div>
      <div className="mt-0.5 text-xs text-ink-400">{detail}</div>
    </div>
  );
}

function ScoreTile({ label, score, empty }: { label: string; score: ReviewScore | null; empty?: string }) {
  if (!score) return <Tile label={label} tone="none" main={empty ?? "—"} detail="" />;
  return (
    <Tile
      label={label}
      tone={reviewTone(score.score)}
      main={reviewLabel(score.score, score.count)}
      detail={`${tr.reviews.positiveShare(score.percent)} · ${tr.reviews.count(score.count)}`}
    />
  );
}

function RecentTile({ recent, loading }: { recent: RecentReviews | null; loading: boolean }) {
  if (loading) return <div className="shimmer h-[76px] rounded-xl" />;
  if (!recent || recent.count === 0) return <Tile label={tr.reviews.recent} tone="none" main="—" detail="" />;
  const percent = (recent.positive / recent.count) * 100;
  return (
    <Tile
      label={tr.reviews.recent}
      tone={percentTone(percent)}
      main={tr.reviews.positiveShare(percent)}
      detail={tr.reviews.recentDetail(recent.count, tr.reviews.span(recent.to - recent.from))}
    />
  );
}

function ReviewCard({ review }: { review: Review }) {
  const [open, setOpen] = useState(false);
  const long = review.text.length > 320 || review.text.split("\n").length > 5;
  return (
    <article className="rounded-xl bg-ink-800/60 p-4 ring-1 ring-white/6">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px]">
        <span className={clsx("inline-flex items-center gap-1.5 font-medium", review.positive ? "text-review-pos" : "text-review-neg")}>
          {review.positive ? <ThumbsUp size={15} /> : <ThumbsDown size={15} />}
          {review.positive ? tr.reviews.recommended : tr.reviews.notRecommended}
        </span>
        <span className="text-ink-400">{tr.reviews.hours(review.hoursAtReview, review.hoursTotal)}</span>
        <span className="text-ink-500">{formatDate(review.created)}</span>
        {review.language !== "turkish" && (
          <span className="rounded bg-white/6 px-1.5 py-0.5 text-[11px] text-ink-300 ring-1 ring-white/8">{tr.reviews.english}</span>
        )}
      </header>
      <p className={clsx("mt-2 text-[14px] leading-relaxed whitespace-pre-line text-ink-200", !open && "line-clamp-5")}>{review.text}</p>
      <footer className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-500">
        {long && (
          <button type="button" onClick={() => setOpen(!open)} className="text-accent-soft hover:underline">
            {open ? tr.reviews.less : tr.reviews.more}
          </button>
        )}
        {review.helpful > 0 && <span>{tr.reviews.helpful(review.helpful)}</span>}
        {review.earlyAccess && <span>{tr.reviews.earlyAccess}</span>}
        {review.receivedForFree && <span>{tr.reviews.free}</span>}
      </footer>
    </article>
  );
}
