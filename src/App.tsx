import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { EmptyState, SyncBanner, Toasts } from "./components/Feedback";
import { FilterSheet } from "./components/FilterSheet";
import { FirstRun } from "./components/FirstRun";
import { GameDetailDialog } from "./components/GameDetailDialog";
import { GameGrid } from "./components/GameGrid";
import { TopBar } from "./components/TopBar";
import { ViewHeader } from "./components/ViewHeader";
import { useStatus, useTags } from "./hooks/useData";
import { useFilters } from "./hooks/useFilters";
import { useSyncEvents } from "./hooks/useSyncEvents";
import { errorText } from "./i18n/tr";
import { api, toCmdError } from "./lib/api";
import { showToast } from "./lib/toast";
import type { AppStatus, WorkerKind } from "./lib/types";

export function App() {
  useSyncEvents();
  const qc = useQueryClient();
  const status = useStatus();
  const f = useFilters();
  const { tags, byId } = useTags();
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [selected, setSelected] = useState<number | null>(null);
  const [total, setTotal] = useState<number | undefined>();

  const tagName = useCallback((id: number) => byId.get(id)?.name, [byId]);
  const openGame = useCallback((appid: number) => setSelected(appid), []);

  const start = (kind: WorkerKind, run: () => Promise<void>) => {
    // Reflect the running job immediately; progress events follow.
    qc.setQueryData<AppStatus>(["status"], (s) => (s ? { ...s, worker: kind, progress: null } : s));
    run().catch((e) => {
      void qc.invalidateQueries({ queryKey: ["status"] });
      showToast({ tone: "error", title: errorText(toCmdError(e)) });
    });
  };
  const fullSync = () => start("full", () => api.startSync(false));
  const newReleases = () => start("new_releases", () => api.fetchNewReleases());
  const cancel = () => void api.cancelSync();

  const empty = status.data?.gameCount === 0;

  return (
    <div className="app-backdrop flex h-full flex-col">
      <TopBar
        f={f}
        status={status.data}
        catalogEmpty={empty}
        onOpenFilters={() => setFiltersOpen(true)}
        onFullSync={fullSync}
        onNewReleases={newReleases}
        onCancelSync={cancel}
      />
      <SyncBanner status={status.data} onResume={fullSync} onRefresh={fullSync} />

      <main className="flex min-h-0 flex-1 flex-col">
        {status.isLoading ? null : empty ? (
          <FirstRun status={status.data} onFullSync={fullSync} onNewReleases={newReleases} onCancel={cancel} />
        ) : (
          <>
            <ViewHeader f={f} total={total} status={status.data} tagName={tagName} onNewReleases={newReleases} />
            <div className="min-h-0 flex-1">
              <GameGrid
                query={f.query}
                tagName={tagName}
                onOpen={openGame}
                relativeDates={f.view === "new"}
                onTotal={setTotal}
                empty={<EmptyState view={f.view} canClear={f.activeCount > 0 || f.search.length > 0} onClear={() => (f.clear(), f.setSearch(""))} />}
              />
            </div>
          </>
        )}
      </main>

      <FilterSheet open={filtersOpen} onClose={() => setFiltersOpen(false)} f={f} tags={tags} />
      <GameDetailDialog
        appid={selected}
        onClose={() => setSelected(null)}
        tagName={tagName}
        onTagClick={(tag) => {
          if (!f.filters.tags.includes(tag)) f.toggleTag(tag);
          setSelected(null);
        }}
        showAdult={f.showAdult}
      />
      <Toasts />
    </div>
  );
}
