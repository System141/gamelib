// Query hooks for catalog data, media and external links.

import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../lib/api";
import type { GameLink, LinkInput, MatchState, StoreMatch, TagInfo } from "../lib/types";

export function useStatus() {
  return useQuery({
    queryKey: ["status"],
    queryFn: api.getStatus,
    // Counts grow during a download; progress itself arrives through events.
    refetchInterval: (q) => (q.state.data?.worker ? 4000 : false),
  });
}

export function useTags() {
  const query = useQuery({ queryKey: ["tags"], queryFn: api.listTags });
  const byId = useMemo(() => new Map<number, TagInfo>((query.data ?? []).map((t) => [t.tagid, t])), [query.data]);
  return { tags: query.data ?? [], byId, isLoading: query.isLoading };
}

export function useGame(appid: number | null) {
  return useQuery({
    queryKey: ["game", appid],
    queryFn: () => api.getGame(appid!),
    enabled: appid != null,
  });
}

export function useGameMedia(appid: number | null) {
  return useQuery({
    queryKey: ["media", appid],
    queryFn: () => api.getGameMedia(appid!),
    enabled: appid != null,
  });
}

export function useSites() {
  return useQuery({ queryKey: ["sites"], queryFn: api.listSites });
}

export function useLinks(appid: number | null) {
  return useQuery({
    queryKey: ["links", appid],
    queryFn: () => api.listLinks(appid!),
    enabled: appid != null,
  });
}

/** After any link change: refresh the game's links, link counts in the grid and the status. */
function useInvalidateLinks() {
  const qc = useQueryClient();
  return (appid: number) => {
    void qc.invalidateQueries({ queryKey: ["links", appid] });
    void qc.invalidateQueries({ queryKey: ["games"] });
    void qc.invalidateQueries({ queryKey: ["status"] });
  };
}

export function useSaveLink() {
  const invalidate = useInvalidateLinks();
  return useMutation({
    mutationFn: (input: LinkInput) => api.saveLink(input),
    onSuccess: (link) => invalidate(link.appid),
  });
}

export function useDeleteLink() {
  const invalidate = useInvalidateLinks();
  return useMutation({
    mutationFn: (link: GameLink) => api.deleteLink(link.id),
    onSuccess: (_, link) => invalidate(link.appid),
  });
}

export function useCheckLink() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (link: GameLink) => api.checkLink(link.id),
    onSuccess: (_, link) => void qc.invalidateQueries({ queryKey: ["links", link.appid] }),
  });
}

// --- other stores ---------------------------------------------------------------------------

export function useStoreMatches(appid: number | null) {
  return useQuery({
    queryKey: ["stores", appid],
    queryFn: () => api.getStoreMatches(appid!),
    enabled: appid != null,
  });
}

/** Asks GOG's GamesDB about the game (Rust does this at most monthly) and stores the answer in
 *  the matches cache. Failures stay quiet: the local matches are still shown. */
export function useStoreLookup(appid: number | null) {
  const qc = useQueryClient();
  return useQuery({
    queryKey: ["stores-lookup", appid],
    queryFn: async () => {
      const matches = await api.refreshStoreMatches(appid!);
      qc.setQueryData(["stores", appid], matches);
      return true;
    },
    enabled: appid != null,
    retry: false,
  });
}

export function useSetMatchState() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ match, appid, state }: { match: StoreMatch; appid: number; state: MatchState }) =>
      api.setMatchState(match.store, match.productId, appid, state),
    onSuccess: (_, { appid }) => {
      void qc.invalidateQueries({ queryKey: ["stores", appid] });
      void qc.invalidateQueries({ queryKey: ["games"] });
      void qc.invalidateQueries({ queryKey: ["status"] });
    },
  });
}
