// "Ayarlar": store accounts, the library folder and data locations.

import clsx from "clsx";
import {
  ArrowUpCircle,
  BadgePercent,
  CircleCheck,
  ExternalLink,
  EyeOff,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  KeyRound,
  LoaderCircle,
  LogIn,
  LogOut,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { errorText, tr } from "../i18n/tr";
import { api, toCmdError } from "../lib/api";
import { formatRelative } from "../lib/format";
import { showToast } from "../lib/toast";
import type { Accounts, AppStatus, CmdError, Store } from "../lib/types";
import {
  useAccounts,
  useAccountsUpdate,
  useHiddenFound,
  useScanInstalled,
  useScanningInstalled,
  useSetFoundHidden,
  useSettings,
  useSettingsUpdate,
} from "../hooks/useData";
import { useCheckUpdate, useInstallUpdate, useUpdateStatus } from "../hooks/useUpdater";
import { FoundMark, StoreMark } from "./badges";
import { ItadKeyForm } from "./PricesSection";
import { IconButton, SmallButton } from "./ui";

export function SettingsView({ status }: { status: AppStatus | undefined }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-8 pt-6 pb-12">
        <h1 className="font-display text-[28px] leading-tight font-semibold tracking-tight text-ink-50">{tr.settings.title}</h1>

        <Section title={tr.accounts.title} icon={<ShieldCheck size={17} className="text-success" />}>
          <GogCard />
          <ItchCard />
          <ItadCard />
          <p className="text-xs leading-relaxed text-ink-500">{tr.accounts.storage}</p>
        </Section>

        <Section title={tr.settings.libraryTitle} icon={<FolderOpen size={17} className="text-accent" />}>
          <LibraryFolder />
        </Section>

        <Section title={tr.found.foldersTitle} icon={<FolderSearch size={17} className="text-accent" />}>
          <ScanFolders />
          <HiddenGames />
        </Section>

        <Section title={tr.update.title} icon={<ArrowUpCircle size={17} className="text-accent" />}>
          <Updates />
        </Section>

        <Section title={tr.settings.dataTitle}>
          <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
            <div className="text-xs text-ink-400">{tr.settings.database}</div>
            <div className="mt-1 font-mono text-[12.5px] break-all text-ink-200">{status?.dbPath ?? "—"}</div>
          </div>
        </Section>
      </div>
    </div>
  );
}

function Section({ title, icon, children }: { title: string; icon?: ReactNode; children: ReactNode }) {
  return (
    <section className="mt-8">
      <h2 className="mb-3 flex items-center gap-2 font-display text-lg font-semibold text-ink-50">
        {icon}
        {title}
      </h2>
      <div className="space-y-3">{children}</div>
    </section>
  );
}

function AccountCard({ store, children, signedIn }: { store: Store; children: ReactNode; signedIn: string | null }) {
  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <div className="flex items-center gap-3">
        <StoreMark store={store} size={32} />
        <div className="min-w-0 flex-1">
          <div className="font-medium text-ink-50">{store === "gog" ? "GOG.com" : "itch.io"}</div>
          {signedIn && (
            <div className="mt-0.5 inline-flex items-center gap-1.5 text-[13px] text-success">
              <CircleCheck size={14} />
              {tr.accounts.signedInAs(signedIn)}
            </div>
          )}
        </div>
      </div>
      <div className="mt-3">{children}</div>
    </div>
  );
}

function useSignOut(store: Store) {
  const update = useAccountsUpdate();
  return () =>
    api
      .signOut(store)
      .then((a) => {
        update(a);
        showToast({ tone: "info", title: tr.accounts.toastSignedOut(tr.storeNames[store]) });
      })
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
}

function useSignedIn(store: Store) {
  const update = useAccountsUpdate();
  return (a: Accounts) => {
    update(a);
    showToast({ tone: "success", title: tr.accounts.toastSignedIn(tr.storeNames[store]) });
  };
}

function GogCard() {
  const accounts = useAccounts();
  const signedIn = useSignedIn("gog");
  const signOut = useSignOut("gog");
  const [waiting, setWaiting] = useState(false);
  const [codeOpen, setCodeOpen] = useState(false);
  const [redirect, setRedirect] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const account = accounts.data?.gog ?? null;

  const login = () => {
    setWaiting(true);
    api
      .gogLogin()
      .then(signedIn)
      .catch((e) => {
        const err = toCmdError(e);
        if (err.kind === "cancelled") return;
        if (err.kind === "invalid" && err.message === "desktop_only") {
          setCodeOpen(true);
          return;
        }
        showToast({ tone: "error", title: errorText(err) });
      })
      .finally(() => setWaiting(false));
  };

  const finish = (e: FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    api
      .gogLoginWithCode(redirect)
      .then((a) => {
        setRedirect("");
        setCodeOpen(false);
        signedIn(a);
      })
      .catch((err) => showToast({ tone: "error", title: errorText(toCmdError(err)) }))
      .finally(() => setSubmitting(false));
  };

  return (
    <AccountCard store="gog" signedIn={account?.username ?? null}>
      {account ? (
        <SmallButton onClick={signOut} icon={<LogOut size={13} />}>
          {tr.accounts.signOut}
        </SmallButton>
      ) : (
        <>
          <p className="text-[13px] leading-relaxed text-ink-300">{tr.accounts.gogDesc}</p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={login}
              disabled={waiting}
              className="inline-flex h-9 items-center gap-2 rounded-lg bg-gog/15 px-3.5 text-sm font-semibold text-gog ring-1 ring-gog/40 transition hover:bg-gog/25 disabled:opacity-70"
            >
              {waiting ? <LoaderCircle size={15} className="animate-spin" /> : <LogIn size={15} />}
              {waiting ? tr.accounts.gogWaiting : tr.accounts.gogLogin}
            </button>
            <button
              type="button"
              onClick={() => setCodeOpen((o) => !o)}
              aria-expanded={codeOpen}
              className="text-[13px] text-ink-400 underline-offset-2 hover:text-ink-100 hover:underline"
            >
              {tr.accounts.codeTitle}
            </button>
          </div>
          {codeOpen && (
            <form onSubmit={finish} className="mt-3 space-y-2 rounded-lg bg-white/3 p-3 ring-1 ring-white/6">
              <p className="text-xs leading-relaxed text-ink-400">{tr.accounts.codeHint}</p>
              <SmallButton onClick={() => api.openGogLoginPage().catch(() => undefined)} icon={<ExternalLink size={13} />}>
                {tr.accounts.openLogin}
              </SmallButton>
              <div className="flex gap-2">
                <input
                  value={redirect}
                  onChange={(e) => setRedirect(e.target.value)}
                  placeholder={tr.accounts.codePlaceholder}
                  spellCheck={false}
                  className="h-9 min-w-0 flex-1 rounded-lg bg-ink-900 px-3 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/10 outline-none placeholder:text-ink-500 focus:ring-accent/50"
                  aria-label={tr.accounts.codeTitle}
                />
                <button
                  type="submit"
                  disabled={!redirect.trim() || submitting}
                  className="inline-flex h-9 items-center gap-2 rounded-lg bg-accent/15 px-3 text-sm font-medium text-accent-soft ring-1 ring-accent/35 hover:bg-accent/25 disabled:opacity-50"
                >
                  {submitting && <LoaderCircle size={14} className="animate-spin" />}
                  {tr.accounts.finish}
                </button>
              </div>
            </form>
          )}
          <p className="mt-3 text-[11.5px] leading-relaxed text-ink-500">{tr.accounts.gogUnofficial}</p>
        </>
      )}
    </AccountCard>
  );
}

function ItchCard() {
  const accounts = useAccounts();
  const signedIn = useSignedIn("itch");
  const signOut = useSignOut("itch");
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const account = accounts.data?.itch ?? null;

  const save = (e: FormEvent) => {
    e.preventDefault();
    setSaving(true);
    api
      .itchSetKey(key)
      .then((a) => {
        setKey("");
        signedIn(a);
      })
      .catch((err) => showToast({ tone: "error", title: errorText(toCmdError(err)) }))
      .finally(() => setSaving(false));
  };

  return (
    <AccountCard store="itch" signedIn={account?.username ?? null}>
      {account ? (
        <SmallButton onClick={signOut} icon={<LogOut size={13} />}>
          {tr.accounts.signOut}
        </SmallButton>
      ) : (
        <>
          <p className="text-[13px] leading-relaxed text-ink-300">{tr.accounts.itchDesc}</p>
          <form onSubmit={save} className="mt-3 flex flex-wrap gap-2">
            <div className="relative min-w-[240px] flex-1">
              <KeyRound size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-500" />
              <input
                type="password"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder={tr.accounts.keyPlaceholder}
                autoComplete="off"
                className="h-9 w-full rounded-lg bg-ink-900 pr-3 pl-8 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/10 outline-none placeholder:font-sans placeholder:text-ink-500 focus:ring-accent/50"
                aria-label={tr.accounts.keyPlaceholder}
              />
            </div>
            <button
              type="submit"
              disabled={!key.trim() || saving}
              className="inline-flex h-9 items-center gap-2 rounded-lg bg-itch/15 px-3.5 text-sm font-semibold text-itch ring-1 ring-itch/40 transition hover:bg-itch/25 disabled:opacity-50"
            >
              {saving && <LoaderCircle size={14} className="animate-spin" />}
              {saving ? tr.accounts.checking : tr.accounts.save}
            </button>
            <SmallButton onClick={() => api.openAccountPage("itch").catch(() => undefined)} icon={<ExternalLink size={13} />}>
              {tr.accounts.createKey}
            </SmallButton>
          </form>
        </>
      )}
    </AccountCard>
  );
}

/** The IsThereAnyDeal key behind the prices in game details. */
function ItadCard() {
  const accounts = useAccounts();
  const update = useAccountsUpdate();
  const saved = accounts.data?.itad ?? null;
  const remove = () =>
    api
      .itadRemoveKey()
      .then((a) => {
        update(a);
        showToast({ tone: "info", title: tr.accounts.toastItadRemoved });
      })
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));

  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <div className="flex items-center gap-3">
        <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-accent/15 text-accent-soft ring-1 ring-accent/30">
          <BadgePercent size={17} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="font-medium text-ink-50">{tr.accounts.itadTitle}</div>
          {saved && (
            <div className="mt-0.5 inline-flex items-center gap-1.5 text-[13px] text-success">
              <CircleCheck size={14} />
              {tr.accounts.itadSaved}
            </div>
          )}
        </div>
      </div>
      <div className="mt-3">
        {saved ? (
          <SmallButton onClick={remove} icon={<Trash2 size={13} />}>
            {tr.accounts.itadRemove}
          </SmallButton>
        ) : (
          <>
            <p className="text-[13px] leading-relaxed text-ink-300">{tr.accounts.itadDesc}</p>
            <ItadKeyForm />
          </>
        )}
      </div>
    </div>
  );
}

function LibraryFolder() {
  const settings = useSettings();
  const update = useSettingsUpdate();
  const [editing, setEditing] = useState<string | null>(null);
  const dir = settings.data?.libraryDir ?? "";

  const fail = (e: unknown) => showToast({ tone: "error", title: errorText(toCmdError(e)) });
  const pick = () =>
    api
      .pickLibraryDir()
      .then((s) => s && update(s))
      .catch((e) => {
        const err: CmdError = toCmdError(e);
        // The browser preview has no folder dialog: edit the path as text instead.
        if (err.kind === "invalid" && err.message === "desktop_only") setEditing(dir);
        else fail(err);
      });
  const save = (e: FormEvent) => {
    e.preventDefault();
    if (editing == null) return;
    api
      .updateSettings({ libraryDir: editing })
      .then((s) => {
        update(s);
        setEditing(null);
        showToast({ tone: "success", title: tr.settings.saved });
      })
      .catch(fail);
  };
  const toggleKeep = () => settings.data && api.updateSettings({ keepInstallers: !settings.data.keepInstallers }).then(update).catch(fail);

  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <p className="text-[13px] leading-relaxed text-ink-300">{tr.settings.libraryHint}</p>
      {editing == null ? (
        <div className="mt-3 flex items-center gap-2">
          <div
            className="min-w-0 flex-1 truncate rounded-lg bg-ink-900 px-3 py-2 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/8"
            title={dir}
          >
            {dir || "—"}
          </div>
          <SmallButton onClick={pick} icon={<FolderOpen size={13} />}>
            {tr.settings.change}
          </SmallButton>
        </div>
      ) : (
        <form onSubmit={save} className="mt-3 flex items-center gap-2">
          <input
            value={editing}
            onChange={(e) => setEditing(e.target.value)}
            spellCheck={false}
            className="h-9 min-w-0 flex-1 rounded-lg bg-ink-900 px-3 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/10 outline-none focus:ring-accent/50"
            aria-label={tr.settings.libraryTitle}
          />
          <button
            type="submit"
            className="inline-flex h-9 items-center rounded-lg bg-accent/15 px-3 text-sm font-medium text-accent-soft ring-1 ring-accent/35 hover:bg-accent/25"
          >
            {tr.settings.save}
          </button>
        </form>
      )}
      <label className="mt-4 flex cursor-pointer items-start gap-3">
        <Switch on={settings.data?.keepInstallers ?? false} onChange={toggleKeep} label={tr.settings.keepInstallers} />
        <span>
          <span className="block text-sm text-ink-100">{tr.settings.keepInstallers}</span>
          <span className="block text-xs text-ink-400">{tr.settings.keepInstallersHint}</span>
        </span>
      </label>
    </div>
  );
}

/** Folders whose game folders count as installed games; the library folder always does. */
function ScanFolders() {
  const settings = useSettings();
  const update = useSettingsUpdate();
  const scan = useScanInstalled();
  const scanning = useScanningInstalled();
  const [typing, setTyping] = useState<string | null>(null);
  const dirs = settings.data?.scanDirs ?? [];

  const fail = (e: unknown) => showToast({ tone: "error", title: errorText(toCmdError(e)) });
  const saved = (s: typeof settings.data) => {
    if (!s) return;
    update(s);
    scan.mutate();
  };
  /** Resolves to whether the folders were saved. */
  const save = (next: string[]) =>
    api
      .updateSettings({ scanDirs: next })
      .then((s) => (saved(s), true))
      .catch((e) => (fail(e), false));
  const add = () =>
    api
      .pickScanDir()
      .then((s) => s && saved(s))
      .catch((e) => {
        const err = toCmdError(e);
        // The browser preview has no folder dialog: type the path instead.
        if (err.kind === "invalid" && err.message === "desktop_only") setTyping("");
        else fail(err);
      });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (typing == null || !typing.trim()) return;
    // A refused path stays in the field to be fixed.
    void save([...dirs, typing.trim()]).then((ok) => ok && setTyping(null));
  };

  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <p className="text-[13px] leading-relaxed text-ink-300">{tr.found.foldersHint}</p>
      <ul className="mt-3 space-y-1.5" aria-label={tr.found.foldersTitle}>
        <li className="flex items-center gap-2 rounded-lg bg-ink-900 px-3 py-2 ring-1 ring-white/8">
          <span className="min-w-0 flex-1">
            <span className="block truncate font-mono text-[12.5px] text-ink-100" title={settings.data?.libraryDir}>
              {settings.data?.libraryDir ?? "—"}
            </span>
            <span className="block text-[11.5px] text-ink-500">{tr.found.libraryAlways}</span>
          </span>
        </li>
        {dirs.map((dir) => (
          <li key={dir} className="flex items-center gap-2 rounded-lg bg-ink-900 px-3 py-2 ring-1 ring-white/8">
            <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] text-ink-100" title={dir}>
              {dir}
            </span>
            <IconButton label={tr.found.removeFolder} icon={<X size={13} />} onClick={() => void save(dirs.filter((d) => d !== dir))} />
          </li>
        ))}
      </ul>
      {typing != null && (
        <form onSubmit={submit} className="mt-2 flex items-center gap-2">
          <input
            value={typing}
            onChange={(e) => setTyping(e.target.value)}
            placeholder={tr.found.folderPlaceholder}
            spellCheck={false}
            autoFocus
            className="h-9 min-w-0 flex-1 rounded-lg bg-ink-900 px-3 font-mono text-[12.5px] text-ink-100 ring-1 ring-white/10 outline-none placeholder:text-ink-500 focus:ring-accent/50"
            aria-label={tr.found.addFolder}
          />
          <button
            type="submit"
            disabled={!typing.trim()}
            className="inline-flex h-9 items-center rounded-lg bg-accent/15 px-3 text-sm font-medium text-accent-soft ring-1 ring-accent/35 hover:bg-accent/25 disabled:opacity-50"
          >
            {tr.found.add}
          </button>
        </form>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <SmallButton onClick={() => void add()} icon={<FolderPlus size={13} />}>
          {tr.found.addFolder}
        </SmallButton>
        <SmallButton
          onClick={() =>
            scan.mutate(undefined, {
              onSuccess: (r) => showToast({ tone: r.added > 0 ? "success" : "info", title: tr.found.toastScanned(r.found, r.added) }),
              onError: fail,
            })
          }
          disabled={scanning}
          icon={scanning ? <LoaderCircle size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        >
          {scanning ? tr.found.scanning : tr.found.rescan}
        </SmallButton>
      </div>
    </div>
  );
}

/** Found games the user took off the installed list, to bring back. */
function HiddenGames() {
  const hidden = useHiddenFound();
  const setHidden = useSetFoundHidden();
  const games = hidden.data ?? [];
  if (games.length === 0) return null;
  const unhide = (productId: string, title: string) =>
    setHidden.mutate(
      { productId, hidden: false },
      {
        onSuccess: () => showToast({ tone: "success", title: tr.found.toastUnhidden(title) }),
        onError: (e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }),
      },
    );
  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <div className="flex items-center gap-2 text-sm font-medium text-ink-100">
        <EyeOff size={15} className="text-ink-400" />
        {tr.found.hiddenTitle(games.length)}
      </div>
      <ul className="mt-2 divide-y divide-white/6">
        {games.map((g) => (
          <li key={g.productId} className="flex items-center gap-3 py-2">
            {g.source && <FoundMark source={g.source} size={20} />}
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] text-ink-100">{g.title}</span>
              <span className="block truncate font-mono text-[11.5px] text-ink-500" title={g.dir ?? undefined}>
                {g.dir}
              </span>
            </span>
            <SmallButton onClick={() => unhide(g.productId, g.title)} icon={<Undo2 size={13} />}>
              {tr.found.unhide}
            </SmallButton>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The version, a manual check, installing a found update and the automatic-check setting. */
function Updates() {
  const status = useUpdateStatus().data;
  const check = useCheckUpdate();
  const { install, installing, progress } = useInstallUpdate();
  const settings = useSettings();
  const update = useSettingsUpdate();
  const [checking, setChecking] = useState(false);
  const run = () => {
    setChecking(true);
    check()
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }))
      .finally(() => setChecking(false));
  };
  const toggleAuto = () =>
    settings.data &&
    api
      .updateSettings({ autoUpdate: !settings.data.autoUpdate })
      .then(update)
      .catch((e) => showToast({ tone: "error", title: errorText(toCmdError(e)) }));
  const found = status?.update;
  const pct = progress?.total ? Math.min(100, (progress.downloaded / progress.total) * 100) : null;

  return (
    <div className="rounded-xl bg-ink-800/70 p-4 ring-1 ring-white/6">
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="font-medium text-ink-50">{status ? tr.update.version(status.currentVersion) : "GameLib"}</div>
          <div className="mt-0.5 text-[13px] text-ink-400">
            {installing
              ? tr.update.downloading(found?.version ?? "")
              : checking
                ? tr.update.checking
                : !status
                  ? ""
                  : !status.configured
                    ? tr.update.notConfigured
                    : found
                      ? tr.update.ready(found.version)
                      : status.checkedAt != null
                        ? tr.update.upToDate(formatRelative(status.checkedAt))
                        : tr.update.notChecked}
          </div>
        </div>
        {found && status?.configured ? (
          <SmallButton
            tone="primary"
            onClick={install}
            disabled={installing}
            icon={installing ? <LoaderCircle size={13} className="animate-spin" /> : <ArrowUpCircle size={13} />}
          >
            {installing && pct != null ? `%${Math.round(pct)}` : tr.update.install}
          </SmallButton>
        ) : status && !status.configured ? (
          <SmallButton onClick={() => void api.openReleasePage(null).catch(() => undefined)} icon={<ExternalLink size={13} />}>
            {tr.update.releases}
          </SmallButton>
        ) : null}
        <SmallButton
          onClick={run}
          disabled={checking || installing}
          icon={checking ? <LoaderCircle size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        >
          {tr.update.check}
        </SmallButton>
      </div>
      <label className="mt-4 flex cursor-pointer items-start gap-3">
        <Switch on={settings.data?.autoUpdate ?? true} onChange={toggleAuto} label={tr.update.auto} />
        <span>
          <span className="block text-sm text-ink-100">{tr.update.auto}</span>
          <span className="block text-xs text-ink-400">{tr.update.autoHint}</span>
        </span>
      </label>
    </div>
  );
}

function Switch({ on, onChange, label }: { on: boolean; onChange: () => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={onChange}
      className={clsx(
        "relative mt-0.5 inline-flex h-5 w-9 shrink-0 items-center rounded-full ring-1 transition",
        on ? "bg-accent/70 ring-accent/60" : "bg-ink-700 ring-white/10",
      )}
    >
      <span className={clsx("size-4 rounded-full bg-white shadow transition", on ? "translate-x-[18px]" : "translate-x-0.5")} />
    </button>
  );
}
