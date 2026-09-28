// Loads only the pages of results that are on screen.
//
// Pages are fetched by position (offset = page × PAGE_SIZE), so the virtual grid can span the whole
// result set — the scrollbar reflects all 130 000 games and can be dragged anywhere — while only a
// few hundred cards' worth of data is ever in memory.

import { useQueries, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useRef } from "react";
import { api } from "../lib/api";
import type { GameCard, GamePage } from "../lib/types";
import type { BaseQuery } from "./useFilters";

export const PAGE_SIZE = 120;

export interface PageRange {
  first: number;
  last: number;
}

export function useGameWindow(query: BaseQuery, range: PageRange) {
  const qc = useQueryClient();
  const key = useMemo(() => JSON.stringify(query), [query]);

  const pages: number[] = [];
  for (let p = range.first; p <= range.last; p += 1) pages.push(p);

  const results = useQueries({
    queries: pages.map((page) => ({
      queryKey: ["games", key, page],
      queryFn: () => api.queryGames({ ...query, offset: page * PAGE_SIZE, limit: PAGE_SIZE }),
    })),
  });

  // Remember the last total per query so the grid keeps its height while pages refetch.
  const totals = useRef(new Map<string, number>());
  let total: number | undefined;
  for (const r of results) {
    if (r.data) {
      total = r.data.total;
      totals.current.set(key, r.data.total);
      break;
    }
  }
  total ??= totals.current.get(key);
  const error = results.find((r) => r.error)?.error ?? null;

  const getItem = useCallback(
    (index: number): GameCard | undefined => {
      const page = Math.floor(index / PAGE_SIZE);
      const data = qc.getQueryData<GamePage>(["games", key, page]);
      return data?.items[index - page * PAGE_SIZE];
    },
    // Re-created whenever a visible page changes so rows re-render with fresh data.
    [qc, key, results.map((r) => r.dataUpdatedAt).join(",")],
  );

  return { total, getItem, error, key };
}
