// Query hooks for catalog data, media and external links.

import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../lib/api";
import type {
  Accounts,
  Download,
  DownloadList,
  GameLink,
  Installed,
  LinkInput,
  MatchState,
  Settings,
  Store,
  StoreMatch,
  TagInfo,
} from "../lib/types";

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

/** Searches the known sites for this game; read-only, so nothing is invalidated. */
export function useFindLinks() {
  return useMutation({
    mutationFn: (appid: number) => api.findLinks(appid),
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

/** After a product is tied to a game by hand. */
export function useLinkStoreProduct() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ store, productId, appid }: { store: Store; productId: string; appid: number }) =>
      api.linkStoreProduct(store, productId, appid),
    onSuccess: (_, { appid }) => {
      void qc.invalidateQueries({ queryKey: ["stores", appid] });
      void qc.invalidateQueries({ queryKey: ["games"] });
      void qc.invalidateQueries({ queryKey: ["library"] });
      void qc.invalidateQueries({ queryKey: ["status"] });
    },
  });
}

// --- accounts, library, settings ------------------------------------------------------------

export function useAccounts() {
  return useQuery({ queryKey: ["accounts"], queryFn: api.getAccounts });
}

/** Stores the accounts a sign-in or sign-out returned, and refreshes what depends on them. */
export function useAccountsUpdate() {
  const qc = useQueryClient();
  return (accounts: Accounts) => {
    qc.setQueryData(["accounts"], accounts);
    void qc.invalidateQueries({ queryKey: ["library"] });
    void qc.invalidateQueries({ queryKey: ["status"] });
    void qc.invalidateQueries({ queryKey: ["stores"] });
  };
}

export function useLibrary() {
  return useQuery({ queryKey: ["library"], queryFn: () => api.getLibrary(null) });
}

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
}

export function useSettingsUpdate() {
  const qc = useQueryClient();
  return (settings: Settings) => qc.setQueryData(["settings"], settings);
}

// --- downloads ------------------------------------------------------------------------------

/** The download list; `useDownloadEvents` keeps it current between fetches. */
export function useDownloads() {
  return useQuery({ queryKey: ["downloads"], queryFn: api.getDownloads });
}

/** What can be downloaded for a product, asked when the picker opens. */
export function useStoreFiles(store: Store, productId: string, enabled: boolean) {
  return useQuery({
    queryKey: ["store-files", store, productId],
    queryFn: () => api.getStoreFiles(store, productId),
    enabled,
    retry: false,
    staleTime: 5 * 60_000,
  });
}

export function useEnqueueDownload() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ store, productId, optionId }: { store: Store; productId: string; optionId: string }) =>
      api.enqueueDownload(store, productId, optionId),
    onSuccess: (download) => {
      qc.setQueryData<DownloadList>(["downloads"], (list) => upsertDownload(list, download));
    },
  });
}

/** Queues a magnet link or a `.torrent` address; the row arrives like any other download. */
export function useEnqueueTorrent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ appid, title, source }: { appid: number; title: string; source: string }) => api.enqueueTorrent(appid, title, source),
    onSuccess: (download) => {
      qc.setQueryData<DownloadList>(["downloads"], (list) => upsertDownload(list, download));
    },
  });
}

/** Puts a download into the list (replacing an older copy of it), newest first. */
export function upsertDownload(list: DownloadList | undefined, download: Download): DownloadList {
  const items = list?.items ?? [];
  const known = items.some((d) => d.id === download.id);
  return {
    items: known ? items.map((d) => (d.id === download.id ? download : d)) : [download, ...items],
    live: list?.live ?? null,
    installing: list?.installing ?? null,
  };
}

/** The most recent download of a product, if any. */
export function downloadOf(list: DownloadList | undefined, store: Store, productId: string): Download | undefined {
  return list?.items.find((d) => d.store === store && d.productId === productId);
}

// --- installed games ------------------------------------------------------------------------

/** Installed games; `useDownloadEvents` refreshes them when one changes. */
export function useInstalls() {
  return useQuery({ queryKey: ["installs"], queryFn: api.getInstalls });
}

export function installOf(list: Installed[] | undefined, store: Store, productId: string): Installed | undefined {
  return list?.find((i) => i.store === store && i.productId === productId);
}
