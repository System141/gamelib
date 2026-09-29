//! Installing finished downloads, one at a time on a thread of its own, and starting and
//! removing installed games.
//!
//! - GOG's Windows installers (Inno Setup) run silently into the library folder; the game's
//!   `goggame-<id>.info` then says what to start. Windows asks for administrator rights once.
//! - Archives (zip, 7z, tar) are unpacked by GameLib; an itch.io `.itch.toml`, or a guess among
//!   the programs, says what to start.
//! - Someone else's installer (an itch.io upload) only runs once the user agrees.
//! - What GameLib cannot install (RAR, macOS packages, Linux scripts, another system's files)
//!   stays in the downloads folder for the user.

pub mod archive;
pub mod inspect;
pub mod process;
pub mod targets;
#[cfg(windows)]
pub mod windows;

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;

use crate::app::{EventSink, read_settings};
use crate::db::Db;
use crate::db::downloads as queue;
use crate::db::installs::{self, InstallRow};
use crate::downloads::names::safe_name;
use crate::downloads::sources::this_platform;
use crate::downloads::{self, EVENT_STATE};
use crate::model::{Download, InstallMethod, InstallProgress, InstallState, Installed, Store};
use crate::{Error, ErrorInfo, Result, unix_now};
use inspect::FileKind;
use targets::LaunchTarget;

/// Progress of the running install.
pub const EVENT_PROGRESS: &str = "install:progress";
/// An installed game was added, changed or removed: `{store, productId}`.
pub const EVENT_CHANGED: &str = "install:changed";

const EMIT_EVERY: Duration = Duration::from_millis(250);
/// Free space to keep beyond what an install needs.
const DISK_MARGIN: u64 = 512 << 20;
/// Inno Setup's silent install; the folder and log follow.
const INNO_SILENT: &str = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-";
const INNO_UNINSTALL: &str = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART";

pub struct InstallManager {
    shared: Arc<Shared>,
}

struct Shared {
    db_path: PathBuf,
    sink: Arc<dyn EventSink>,
    control: Mutex<Control>,
    wake: Condvar,
}

#[derive(Default)]
struct Control {
    started: bool,
    dirty: bool,
    live: Option<InstallProgress>,
}

enum Outcome {
    Installed(Box<InstallRow>, FileKind),
    /// Someone else's installer: it waits for the user's approval.
    Confirm(FileKind),
    /// GameLib cannot install it.
    Manual(FileKind),
}

impl InstallManager {
    pub fn new(db_path: PathBuf, sink: Arc<dyn EventSink>) -> Self {
        Self {
            shared: Arc::new(Shared {
                db_path,
                sink,
                control: Mutex::new(Control::default()),
                wake: Condvar::new(),
            }),
        }
    }

    /// Starts the install thread (once). Installs interrupted by closing the app run again.
    pub fn start(&self, conn: &Connection) -> Result<()> {
        let mut control = lock(&self.shared.control);
        if control.started {
            return Ok(());
        }
        queue::requeue_installing(conn, unix_now())?;
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("gamelib-installs".into())
            .spawn(move || worker(&shared))
            .map_err(|e| Error::Other(format!("could not start the install thread: {e}")))?;
        control.started = true;
        control.dirty = true;
        self.shared.wake.notify_all();
        Ok(())
    }

    /// Something to call when a download finished, so it is installed.
    pub fn notifier(&self) -> Arc<dyn Fn() + Send + Sync> {
        let shared = self.shared.clone();
        Arc::new(move || shared.poke())
    }

    /// Progress of the running install, if any.
    pub fn live(&self) -> Option<InstallProgress> {
        lock(&self.shared.control).live.clone()
    }

    /// Lets someone else's installer run.
    pub fn approve(&self, conn: &Connection, download_id: i64) -> Result<()> {
        self.transition(
            conn,
            download_id,
            &[Some(InstallState::Confirm)],
            InstallState::Approved,
        )
    }

    /// Tries a failed install again, or installs a download from before installs existed.
    pub fn retry(&self, conn: &Connection, download_id: i64) -> Result<()> {
        self.transition(
            conn,
            download_id,
            &[None, Some(InstallState::Failed)],
            InstallState::Waiting,
        )
    }

    fn transition(
        &self,
        conn: &Connection,
        id: i64,
        from: &[Option<InstallState>],
        to: InstallState,
    ) -> Result<()> {
        queue::get(conn, id)?.ok_or(Error::NotFound)?;
        if queue::transition_install(conn, id, from, to, unix_now())? {
            self.shared.emit_download(conn, id);
            self.shared.poke();
        }
        Ok(())
    }
}

impl Shared {
    fn emit(&self, event: &str, payload: &impl Serialize) {
        self.sink.emit(
            event,
            serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        );
    }

    fn emit_download(&self, conn: &Connection, id: i64) {
        if let Ok(Some(d)) = queue::get(conn, id) {
            self.emit(EVENT_STATE, &d);
        }
    }

    fn poke(&self) {
        lock(&self.control).dirty = true;
        self.wake.notify_all();
    }
}

fn worker(shared: &Arc<Shared>) {
    let db = match Db::open(&shared.db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("install worker: {e}");
            return;
        }
    };
    loop {
        match claim(shared, db.conn()) {
            Some((download, approved)) => run(shared, db.conn(), &download, approved),
            None => {
                let mut control = lock(&shared.control);
                if !control.dirty {
                    control = shared
                        .wake
                        .wait_timeout(control, Duration::from_secs(60))
                        .map(|(c, _)| c)
                        .unwrap_or_else(|p| p.into_inner().0);
                }
                control.dirty = false;
            }
        }
    }
}

/// Takes the oldest finished download waiting to be installed. `true` when the user approved
/// running its installer.
fn claim(shared: &Shared, conn: &Connection) -> Option<(Download, bool)> {
    let _control = lock(&shared.control);
    let download = queue::next_install(conn).ok().flatten()?;
    let approved = download.install_state == Some(InstallState::Approved);
    let from = [download.install_state];
    let claimed = queue::transition_install(
        conn,
        download.id,
        &from,
        InstallState::Installing,
        unix_now(),
    );
    matches!(claimed, Ok(true)).then_some((download, approved))
}

fn run(shared: &Shared, conn: &Connection, d: &Download, approved: bool) {
    shared.emit_download(conn, d.id);
    let mut reporter = Reporter::new(shared, d.id);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        install(conn, d, approved, &mut reporter)
    }))
    .unwrap_or_else(|_| Err(Error::Other("the install stopped unexpectedly".into())));
    lock(&shared.control).live = None;
    let now = unix_now();
    let _ = match outcome {
        Ok(Outcome::Installed(row, kind)) => {
            let saved = installs::upsert(conn, &row);
            if saved.is_ok() {
                shared.emit(
                    EVENT_CHANGED,
                    &serde_json::json!({ "store": d.store, "productId": d.product_id }),
                );
            }
            match saved {
                Ok(()) => queue::set_install_state(
                    conn,
                    d.id,
                    Some(InstallState::Installed),
                    Some(kind.as_str()),
                    None,
                    now,
                ),
                Err(e) => queue::set_install_state(
                    conn,
                    d.id,
                    Some(InstallState::Failed),
                    Some(kind.as_str()),
                    Some(&e.into()),
                    now,
                ),
            }
        }
        Ok(Outcome::Confirm(kind)) => queue::set_install_state(
            conn,
            d.id,
            Some(InstallState::Confirm),
            Some(kind.as_str()),
            None,
            now,
        ),
        Ok(Outcome::Manual(kind)) => queue::set_install_state(
            conn,
            d.id,
            Some(InstallState::Manual),
            Some(kind.as_str()),
            None,
            now,
        ),
        Err(e) => {
            let info = ErrorInfo::from(e);
            queue::set_install_state(
                conn,
                d.id,
                Some(InstallState::Failed),
                None,
                Some(&info),
                now,
            )
        }
    };
    shared.emit_download(conn, d.id);
}

/// Reports install progress without flooding the UI.
struct Reporter<'a> {
    shared: &'a Shared,
    download_id: i64,
    stage: &'static str,
    last: Option<Instant>,
}

impl<'a> Reporter<'a> {
    fn new(shared: &'a Shared, download_id: i64) -> Self {
        Self {
            shared,
            download_id,
            stage: "",
            last: None,
        }
    }

    fn report(&mut self, stage: &'static str, done: u64, total: u64) {
        let now = Instant::now();
        let changed = stage != self.stage;
        if !changed
            && self
                .last
                .is_some_and(|t| now.duration_since(t) < EMIT_EVERY)
        {
            return;
        }
        self.stage = stage;
        self.last = Some(now);
        let progress = InstallProgress {
            download_id: self.download_id,
            stage: stage.into(),
            done,
            total,
        };
        lock(&self.shared.control).live = Some(progress.clone());
        self.shared.emit(EVENT_PROGRESS, &progress);
    }
}

fn install(
    conn: &Connection,
    d: &Download,
    approved: bool,
    reporter: &mut Reporter,
) -> Result<Outcome> {
    let settings = read_settings(conn)?;
    let library = PathBuf::from(&settings.library_dir);
    let dir = PathBuf::from(&d.dir);
    let files: Vec<PathBuf> = queue::files(conn, d.id)?
        .into_iter()
        .filter_map(|f| f.file_name.map(|name| dir.join(name)))
        .collect();
    if files.is_empty() || !files.iter().all(|f| f.is_file()) {
        return Err(Error::Invalid("install_files"));
    }
    // GOG's installer comes first, followed by its data files.
    let main = &files[0];
    reporter.report("checking", 0, 0);
    let kind = inspect::inspect(main)?;
    if !runs_here(kind) {
        return Ok(Outcome::Manual(kind));
    }
    if kind.is_installer() && d.store != Store::Gog && !approved {
        return Ok(Outcome::Confirm(kind));
    }
    #[cfg(windows)]
    for file in &files {
        windows::mark_downloaded(file, store_page(d.store))?;
    }
    let target = install_dir(conn, &library, d)?;
    let platform = this_platform();
    let (folder, launch, method) = match kind {
        FileKind::InnoSetup => {
            ensure_space(&library, d.total_bytes)?;
            let log = dir.join("install.log");
            let params = format!(
                "{INNO_SILENT} /DIR=\"{}\" /LOG=\"{}\"",
                target.display(),
                log.display()
            );
            reporter.report("installing", 0, 0);
            match process::run_and_wait(main, &params, &dir)? {
                0 => {}
                // Cancelled in the wizard.
                2 | 5 => return Err(Error::Cancelled),
                code => return Err(Error::Failed("installer_failed", code.to_string())),
            }
            let launch = targets::gog_play_task(&target, &d.product_id)
                .or_else(|| targets::guess(&target, platform));
            let method = if d.store == Store::Gog {
                InstallMethod::Gog
            } else {
                InstallMethod::Installer
            };
            (Some(target), launch, method)
        }
        FileKind::Nsis => {
            ensure_space(&library, d.total_bytes)?;
            reporter.report("installing", 0, 0);
            // NSIS wants /D last and unquoted, even with spaces.
            match process::run_and_wait(main, &format!("/S /D={}", target.display()), &dir)? {
                0 => {}
                code => return Err(Error::Failed("installer_failed", code.to_string())),
            }
            let launch = targets::guess(&target, platform);
            (Some(target), launch, InstallMethod::Installer)
        }
        FileKind::Msi | FileKind::Installer => {
            reporter.report("installing", 0, 0);
            // These choose their own folder, so the user picks what to start afterwards.
            let code = if kind == FileKind::Msi {
                process::run_and_wait(
                    &system_program("msiexec.exe"),
                    &format!("/i \"{}\"", main.display()),
                    &dir,
                )?
            } else {
                process::run_and_wait(main, "", &dir)?
            };
            match code {
                // 3010: done, a restart is needed.
                0 | 3010 => {}
                1602 => return Err(Error::Cancelled),
                code => return Err(Error::Failed("installer_failed", code.to_string())),
            }
            (None, None, InstallMethod::Installer)
        }
        FileKind::Exe | FileKind::LinuxProgram => {
            fs::create_dir_all(&target).map_err(|e| io(&target, e))?;
            let name = main.file_name().ok_or(Error::Invalid("install_files"))?;
            let exe = target.join(name);
            fs::copy(main, &exe).map_err(|e| io(&exe, e))?;
            #[cfg(unix)]
            process::make_executable(&exe);
            (
                Some(target),
                Some(LaunchTarget::plain(exe)),
                InstallMethod::Portable,
            )
        }
        k if k.is_archive() => {
            if let Some(size) = archive::unpacked_size(k, main)? {
                ensure_space(&library, size)?;
            }
            let temp = temp_folder(&library, d.id);
            let _ = fs::remove_dir_all(&temp);
            let never = AtomicBool::new(false);
            let unpacked = archive::unpack(k, main, &temp, &never, &mut |done, total| {
                reporter.report("unpacking", done, total)
            });
            if let Err(e) = unpacked {
                let _ = fs::remove_dir_all(&temp);
                return Err(e);
            }
            let root = single_folder(&temp).unwrap_or_else(|| temp.clone());
            let moved = move_into(&root, &target);
            let _ = fs::remove_dir_all(&temp);
            moved?;
            let launch = targets::itch_manifest(&target, platform)
                .or_else(|| targets::guess(&target, platform));
            (Some(target), launch, InstallMethod::Archive)
        }
        _ => return Ok(Outcome::Manual(kind)),
    };

    let candidates = folder
        .as_deref()
        .map(|f| targets::candidates(f, platform))
        .unwrap_or_default();
    let uninstaller = match method {
        InstallMethod::Gog | InstallMethod::Installer => {
            folder.as_deref().and_then(process::find_uninstaller)
        }
        _ => None,
    };
    let row = InstallRow {
        installed: Installed {
            store: d.store,
            product_id: d.product_id.clone(),
            appid: d.appid,
            title: d.title.clone(),
            dir: folder.as_deref().map(display),
            exe: launch.as_ref().map(|l| display(&l.exe)),
            args: launch.as_ref().map(|l| l.args.clone()).unwrap_or_default(),
            workdir: launch
                .as_ref()
                .and_then(|l| l.workdir.as_deref())
                .map(display),
            method,
            candidates: candidates.iter().map(|c| display(c)).collect(),
            option_label: d.option_label.clone(),
            installed_at: unix_now(),
            external: false,
            steam_header: None,
        },
        uninstaller: uninstaller.as_deref().map(display),
    };
    if !settings.keep_installers {
        reporter.report("cleaning", 0, 0);
        downloads::remove_files(&d.dir);
    }
    Ok(Outcome::Installed(Box::new(row), kind))
}

/// Whether GameLib can install this kind of file on this computer.
fn runs_here(kind: FileKind) -> bool {
    match kind {
        FileKind::InnoSetup
        | FileKind::Nsis
        | FileKind::Msi
        | FileKind::Installer
        | FileKind::Exe => cfg!(windows),
        FileKind::LinuxProgram => cfg!(target_os = "linux"),
        k => k.is_archive(),
    }
}

/// The folder a game goes into: where it already is when reinstalling, otherwise a free one
/// named after it in the library.
fn install_dir(conn: &Connection, library: &Path, d: &Download) -> Result<PathBuf> {
    if let Some(row) = installs::get(conn, d.store, &d.product_id)?
        && let Some(dir) = row.installed.dir
    {
        return Ok(PathBuf::from(dir));
    }
    let base = safe_name(&d.title, "Game");
    for n in 1..100 {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base} ({n})")
        };
        let path = library.join(name);
        if !path.exists() || is_empty_dir(&path) {
            return Ok(path);
        }
    }
    Err(Error::Other(format!(
        "no free folder for {} in {}",
        d.title,
        library.display()
    )))
}

/// `<library>/.gamelib/tmp/<id>`: unpacked here first, then moved into place.
fn temp_folder(library: &Path, id: i64) -> PathBuf {
    library.join(".gamelib").join("tmp").join(id.to_string())
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

/// The only folder inside `dir`, when an archive wraps everything in one (macOS's `__MACOSX`
/// leftovers aside).
fn single_folder(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_none_or(|n| n != "__MACOSX"))
        .collect();
    match entries.as_slice() {
        [only] if only.is_dir() => Some(only.clone()),
        _ => None,
    }
}

/// Moves the contents of `from` into `to`: a rename when `to` is new, otherwise file by file,
/// replacing older files and keeping the rest (saves stored next to the game survive an update).
fn move_into(from: &Path, to: &Path) -> Result<()> {
    if !to.exists() {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        if fs::rename(from, to).is_ok() {
            return Ok(());
        }
    }
    fs::create_dir_all(to).map_err(|e| io(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| io(from, e))?.flatten() {
        let source = entry.path();
        let dest = to.join(entry.file_name());
        if source.is_dir() {
            move_into(&source, &dest)?;
        } else {
            if dest.is_dir() {
                fs::remove_dir_all(&dest).map_err(|e| io(&dest, e))?;
            }
            if fs::rename(&source, &dest).is_err() {
                let _ = fs::remove_file(&dest);
                fs::copy(&source, &dest).map_err(|e| io(&dest, e))?;
            }
        }
    }
    Ok(())
}

fn ensure_space(library: &Path, needed: u64) -> Result<()> {
    fs::create_dir_all(library).map_err(|e| io(library, e))?;
    let free = fs4::available_space(library).unwrap_or(u64::MAX);
    if free < needed.saturating_add(DISK_MARGIN) {
        return Err(Error::Invalid("disk_space"));
    }
    Ok(())
}

/// A program in Windows' System32 folder.
fn system_program(name: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(name)
}

/// Where a store's files come from, for the downloaded-file mark.
#[cfg(windows)]
fn store_page(store: Store) -> &'static str {
    match store {
        Store::Gog => "https://www.gog.com/",
        Store::Itch => "https://itch.io/",
        // Web downloads have no store page to mark.
        Store::Web => "",
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Other(format!("{}: {e}", path.display()))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// --- installed games ---------------------------------------------------------------------------

/// Every installed game: GameLib's own installs, plus (on Windows) GOG games registered by GOG
/// Galaxy or a GOG installer run by hand.
pub fn list(conn: &Connection) -> Result<Vec<Installed>> {
    let mut all: Vec<Installed> = installs::list(conn)?
        .into_iter()
        .map(|r| r.installed)
        .collect();
    #[cfg(windows)]
    for game in windows::gog_registered_games() {
        if !all
            .iter()
            .any(|i| i.store == Store::Gog && i.product_id == game.product_id)
        {
            all.push(external(conn, &game)?);
        }
    }
    for game in &mut all {
        if let Some(appid) = game.appid {
            game.steam_header = crate::db::stores::steam_header(conn, appid)?;
        }
    }
    Ok(all)
}

#[cfg(windows)]
fn external(conn: &Connection, game: &windows::RegisteredGame) -> Result<Installed> {
    let known = crate::db::stores::product_summary(conn, Store::Gog, &game.product_id)?;
    let (title, appid) = known.unwrap_or_else(|| (game.title.clone(), None));
    Ok(Installed {
        store: Store::Gog,
        product_id: game.product_id.clone(),
        appid,
        title,
        dir: Some(display(&game.dir)),
        exe: game.exe.as_deref().map(display),
        args: game.args.clone(),
        workdir: game.workdir.as_deref().map(display),
        method: InstallMethod::Galaxy,
        candidates: Vec::new(),
        option_label: None,
        installed_at: 0,
        external: true,
        steam_header: None,
    })
}

/// An installed game and its uninstaller.
fn find(conn: &Connection, store: Store, product_id: &str) -> Result<(Installed, Option<PathBuf>)> {
    if let Some(row) = installs::get(conn, store, product_id)? {
        return Ok((row.installed, row.uninstaller.map(PathBuf::from)));
    }
    #[cfg(windows)]
    if store == Store::Gog
        && let Some(game) = windows::gog_registered_games()
            .into_iter()
            .find(|g| g.product_id == product_id)
    {
        let uninstaller = game.uninstaller.clone();
        return Ok((external(conn, &game)?, uninstaller));
    }
    Err(Error::NotFound)
}

/// Starts an installed game.
pub fn launch(conn: &Connection, store: Store, product_id: &str) -> Result<()> {
    let (game, _) = find(conn, store, product_id)?;
    let exe = game.exe.ok_or(Error::Invalid("launch_target"))?;
    process::launch(&LaunchTarget {
        exe: PathBuf::from(exe),
        args: game.args,
        workdir: game.workdir.map(PathBuf::from),
    })
}

/// The folder of an installed game.
pub fn folder(conn: &Connection, store: Store, product_id: &str) -> Result<PathBuf> {
    let (game, _) = find(conn, store, product_id)?;
    game.dir
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .ok_or(Error::NotFound)
}

/// Chooses what "Oyna" starts: any existing program, e.g. one of the candidates or a file the
/// user picked.
pub fn set_launch_target(
    conn: &Connection,
    store: Store,
    product_id: &str,
    exe: &str,
) -> Result<Installed> {
    let path = Path::new(exe);
    let program =
        path.is_file() || (cfg!(target_os = "macos") && path.is_dir() && exe.ends_with(".app"));
    if !path.is_absolute() || !program {
        return Err(Error::Invalid("launch_target"));
    }
    let workdir = path.parent().map(display);
    if !installs::set_launch(conn, store, product_id, exe, workdir.as_deref())? {
        return Err(Error::NotFound);
    }
    Ok(installs::get(conn, store, product_id)?
        .ok_or(Error::NotFound)?
        .installed)
}

/// What removing an installed game needs, read before anything runs.
pub struct Removal {
    game: Installed,
    uninstaller: Option<PathBuf>,
}

pub fn removal(conn: &Connection, store: Store, product_id: &str) -> Result<Removal> {
    let (game, uninstaller) = find(conn, store, product_id)?;
    Ok(Removal { game, uninstaller })
}

/// Removes an installed game from the disk: its uninstaller when it has one (GOG's runs
/// silently), otherwise its folder when GameLib put it into the library. Someone else's
/// installer without an uninstaller leaves the files; Windows' settings can remove them.
/// Touches no database, so it can wait for an uninstaller without holding a connection.
pub fn remove(removal: &Removal, library: &Path) -> Result<()> {
    let game = &removal.game;
    let dir = game.dir.as_deref().map(PathBuf::from);
    let in_library = dir.as_deref().is_some_and(|d| {
        d.starts_with(library) && d != library && !d.starts_with(library.join(".gamelib"))
    });
    match game.method {
        InstallMethod::Gog | InstallMethod::Galaxy | InstallMethod::Installer => {
            let uninstaller = removal
                .uninstaller
                .clone()
                .filter(|u| u.is_file())
                .or_else(|| dir.as_deref().and_then(process::find_uninstaller));
            if let Some(uninstaller) = uninstaller {
                let inno = uninstaller
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("unins0"));
                let params = if inno { INNO_UNINSTALL } else { "" };
                let workdir = uninstaller
                    .parent()
                    .map_or_else(std::env::temp_dir, Path::to_path_buf);
                match process::run_and_wait(&uninstaller, params, &workdir)? {
                    0 => {}
                    code => return Err(Error::Failed("uninstaller_failed", code.to_string())),
                }
                // The uninstaller leaves the folder when it holds saves; an empty one goes.
                if let Some(dir) = dir.as_deref()
                    && in_library
                    && is_empty_dir(dir)
                {
                    let _ = fs::remove_dir(dir);
                }
            } else if game.method == InstallMethod::Gog
                && let Some(dir) = dir.as_deref()
                && in_library
            {
                fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
            }
        }
        InstallMethod::Archive | InstallMethod::Portable => {
            if let Some(dir) = dir.as_deref()
                && in_library
                && dir.exists()
            {
                fs::remove_dir_all(dir).map_err(|e| io(dir, e))?;
            }
        }
    }
    Ok(())
}

/// Forgets a removed game.
pub fn forget(conn: &Connection, store: Store, product_id: &str) -> Result<()> {
    installs::delete(conn, store, product_id)?;
    queue::forget_installed(conn, store, product_id, unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gamelib-install-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn moving_into_an_existing_folder_keeps_other_files() {
        let base = temp("move");
        let from = base.join("new");
        let to = base.join("game");
        fs::create_dir_all(from.join("data")).unwrap();
        fs::write(from.join("game.exe"), b"v2").unwrap();
        fs::write(from.join("data").join("a.pak"), b"v2").unwrap();
        fs::create_dir_all(to.join("saves")).unwrap();
        fs::write(to.join("game.exe"), b"v1").unwrap();
        fs::write(to.join("saves").join("slot1.sav"), b"mine").unwrap();
        move_into(&from, &to).unwrap();
        assert_eq!(fs::read(to.join("game.exe")).unwrap(), b"v2");
        assert_eq!(fs::read(to.join("data").join("a.pak")).unwrap(), b"v2");
        assert_eq!(
            fs::read(to.join("saves").join("slot1.sav")).unwrap(),
            b"mine"
        );
        // A new folder is a plain rename.
        let fresh = base.join("fresh");
        fs::create_dir_all(base.join("new2")).unwrap();
        fs::write(base.join("new2").join("x"), b"1").unwrap();
        move_into(&base.join("new2"), &fresh).unwrap();
        assert!(fresh.join("x").exists() && !base.join("new2").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn single_wrapping_folders_are_unwrapped() {
        let base = temp("single");
        fs::create_dir_all(base.join("Game").join("bin")).unwrap();
        fs::create_dir_all(base.join("__MACOSX")).unwrap();
        assert_eq!(single_folder(&base), Some(base.join("Game")));
        fs::write(base.join("readme.txt"), b"x").unwrap();
        assert_eq!(single_folder(&base), None);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn what_installs_where() {
        assert!(runs_here(FileKind::Zip) && runs_here(FileKind::TarGz));
        assert_eq!(runs_here(FileKind::InnoSetup), cfg!(windows));
        assert_eq!(runs_here(FileKind::LinuxProgram), cfg!(target_os = "linux"));
        assert!(
            !runs_here(FileKind::Rar)
                && !runs_here(FileKind::MacPackage)
                && !runs_here(FileKind::Script)
        );
    }
}
