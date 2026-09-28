// Virtualized grid over the whole result set: only visible rows are rendered and only the pages
// under them are loaded.

import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowUp } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { tr } from "../i18n/tr";
import { computeGridLayout } from "../lib/grid";
import type { BaseQuery } from "../hooks/useFilters";
import { PAGE_SIZE, type PageRange, useGameWindow } from "../hooks/useGameWindow";
import { useElementWidth } from "../hooks/useUtils";
import { GameCard, SkeletonCard } from "./GameCard";

const LOADING_ROWS = 3;

interface Props {
  query: BaseQuery;
  tagName: (tagid: number) => string | undefined;
  onOpen: (appid: number) => void;
  relativeDates?: boolean;
  empty: ReactNode;
  onTotal?: (total: number | undefined) => void;
}

export function GameGrid({ query, tagName, onOpen, relativeDates = false, empty, onTotal }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(scrollRef);
  const layout = computeGridLayout(width || 1200);
  // The visible page range belongs to one query; a new query starts at the top.
  const queryKey = JSON.stringify(query);
  const [range, setRange] = useState<PageRange & { key: string }>({ key: queryKey, first: 0, last: 0 });
  const current = range.key === queryKey ? range : { key: queryKey, first: 0, last: 0 };
  const { total, getItem, key, error } = useGameWindow(query, current);

  useEffect(() => onTotal?.(total), [total, onTotal]);

  const rowCount = total == null ? LOADING_ROWS : Math.ceil(total / layout.cols);
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => layout.rowHeight,
    overscan: 2,
    paddingStart: 8,
    paddingEnd: layout.padding * 2,
    useFlushSync: false,
  });

  // Row heights depend on the width; re-measure when the layout changes.
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, layout.rowHeight, layout.cols]);

  // New search or filters: start from the top.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [key]);

  const rows = virtualizer.getVirtualItems();
  const firstIndex = rows.length ? rows[0]!.index * layout.cols : 0;
  const lastIndex = rows.length ? (rows[rows.length - 1]!.index + 1) * layout.cols - 1 : 0;
  const maxPage = total ? Math.max(0, Math.ceil(total / PAGE_SIZE) - 1) : 0;
  const first = Math.min(Math.floor(firstIndex / PAGE_SIZE), maxPage);
  const last = Math.min(Math.floor(lastIndex / PAGE_SIZE), maxPage);
  useEffect(() => {
    setRange((r) => (r.key === queryKey && r.first === first && r.last === last ? r : { key: queryKey, first, last }));
  }, [queryKey, first, last]);

  const [showTop, setShowTop] = useState(false);
  const onScroll = useCallback(() => setShowTop((scrollRef.current?.scrollTop ?? 0) > 1600), []);

  if (total === 0 && !error) {
    return <div className="flex h-full items-center justify-center p-8">{empty}</div>;
  }

  return (
    <div className="relative h-full">
      <div ref={scrollRef} onScroll={onScroll} className="h-full overflow-x-hidden overflow-y-auto" role="list" aria-label={tr.grid.label}>
        <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
          {rows.map((row) => (
            <div
              key={row.key}
              className="absolute inset-x-0 top-0 grid"
              style={{
                transform: `translateY(${row.start}px)`,
                gridTemplateColumns: `repeat(${layout.cols}, minmax(0, 1fr))`,
                columnGap: layout.gap,
                paddingInline: layout.padding,
              }}
            >
              {Array.from({ length: layout.cols }, (_, col) => {
                const index = row.index * layout.cols + col;
                if (total != null && index >= total) return <div key={`pad-${col}`} />;
                const game = total == null ? undefined : getItem(index);
                return (
                  <div key={game ? game.appid : `s-${index}`} role="listitem" className="min-w-0">
                    {game ? <GameCard game={game} tagName={tagName} onOpen={onOpen} relativeDate={relativeDates} /> : <SkeletonCard />}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>

      {showTop && (
        <button
          type="button"
          onClick={() => scrollRef.current?.scrollTo({ top: 0, behavior: "smooth" })}
          className="glass animate-rise absolute right-6 bottom-6 grid size-11 place-items-center rounded-full text-ink-100 shadow-xl shadow-black/50 ring-1 ring-white/10 transition hover:text-white hover:ring-accent/50"
          aria-label={tr.grid.backToTop}
        >
          <ArrowUp size={18} />
        </button>
      )}
    </div>
  );
}
