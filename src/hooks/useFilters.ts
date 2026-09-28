// View, search and filter state, turned into the query sent to Rust.

import { useCallback, useMemo, useState } from "react";
import type { DeckFilter, GameQuery, Platform, SortKey } from "../lib/types";
import { useDebounced, usePersistentState } from "./useUtils";

/** Every page of the app. The grid views list Steam games; the others have their own layout. */
export type View = "all" | "new" | "links" | "gog" | "itch" | "library" | "installed" | "downloads" | "settings";
export type GridView = Exclude<View, "library" | "installed" | "downloads" | "settings">;

export const VIEWS: readonly View[] = ["all", "new", "links", "gog", "itch", "library", "installed", "downloads", "settings"];

export function isGridView(view: View): view is GridView {
  return view !== "library" && view !== "installed" && view !== "downloads" && view !== "settings";
}
export type NewDays = 7 | 30 | 90;

export interface Filters {
  tags: number[];
  platforms: Platform[];
  deck: DeckFilter | null;
  freeOnly: boolean;
  minReviewScore: number | null;
  hasLinks: boolean;
}

export const EMPTY_FILTERS: Filters = {
  tags: [],
  platforms: [],
  deck: null,
  freeOnly: false,
  minReviewScore: null,
  hasLinks: false,
};

/** The query without paging; `useGameWindow` adds offset/limit. */
export type BaseQuery = Omit<GameQuery, "offset" | "limit">;

export function useFilters() {
  const [storedView, setViewRaw] = usePersistentState<View>("gamelib.view", "all");
  const view: View = VIEWS.includes(storedView) ? storedView : "all";
  const [chosenSort, setChosenSort] = usePersistentState<SortKey | null>("gamelib.sort", null);
  const [showAdult, setShowAdult] = usePersistentState<boolean>("gamelib.showAdult", false);
  const [newDays, setNewDays] = usePersistentState<NewDays>("gamelib.newDays", 30);
  const [search, setSearch] = useState("");
  const [filters, setFilters] = useState<Filters>(EMPTY_FILTERS);
  const debouncedSearch = useDebounced(search.trim(), 250);

  const searching = debouncedSearch.length > 0;
  const defaultSort: SortKey = searching ? "relevance" : view === "new" ? "newest" : "popular";
  // A chosen "relevance" only makes sense while searching.
  const sort: SortKey = chosenSort && !(chosenSort === "relevance" && !searching) ? chosenSort : defaultSort;

  const query: BaseQuery = useMemo(
    () => ({
      search: debouncedSearch || null,
      tags: filters.tags,
      platforms: filters.platforms,
      deck: filters.deck,
      freeOnly: filters.freeOnly,
      minReviewScore: filters.minReviewScore,
      showAdult,
      releasedWithinDays: view === "new" ? newDays : null,
      hasLinks: view === "links" || filters.hasLinks,
      stores: view === "gog" || view === "itch" ? [view] : [],
      owned: false,
      sort,
    }),
    [debouncedSearch, filters, showAdult, view, newDays, sort],
  );

  const activeCount =
    filters.tags.length +
    filters.platforms.length +
    (filters.deck ? 1 : 0) +
    (filters.freeOnly ? 1 : 0) +
    (filters.minReviewScore ? 1 : 0) +
    (filters.hasLinks && view !== "links" ? 1 : 0) +
    (showAdult ? 1 : 0);

  const patch = useCallback((p: Partial<Filters>) => setFilters((f) => ({ ...f, ...p })), []);

  const toggleTag = useCallback(
    (tagid: number) =>
      setFilters((f) => ({
        ...f,
        tags: f.tags.includes(tagid) ? f.tags.filter((t) => t !== tagid) : [...f.tags, tagid],
      })),
    [],
  );

  const togglePlatform = useCallback(
    (p: Platform) =>
      setFilters((f) => ({
        ...f,
        platforms: f.platforms.includes(p) ? f.platforms.filter((x) => x !== p) : [...f.platforms, p],
      })),
    [],
  );

  const clear = useCallback(() => {
    setFilters(EMPTY_FILTERS);
    setShowAdult(false);
  }, [setShowAdult]);

  const setView = (v: View) => {
    setViewRaw(v);
    // Each view has its own natural order; drop a sort picked for another view.
    setChosenSort(null);
  };

  return {
    view,
    setView,
    search,
    setSearch,
    searching,
    sort,
    setSort: setChosenSort,
    showAdult,
    setShowAdult,
    newDays,
    setNewDays,
    filters,
    patch,
    toggleTag,
    togglePlatform,
    clear,
    activeCount,
    query,
  };
}

export type FiltersState = ReturnType<typeof useFilters>;
