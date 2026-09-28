//! Running installers and starting games.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::targets::LaunchTarget;
#[cfg(unix)]
use super::targets::split_args;
use crate::{Error, Result};

/// Runs a program and waits for it, returning its exit code. On Windows it goes through the
/// shell, so an installer that needs administrator rights asks for them; refusing is
/// `Cancelled`.
pub fn run_and_wait(program: &Path, params: &str, workdir: &Path) -> Result<u32> {
    #[cfg(windows)]
    {
        super::windows::shell_execute(program, params, workdir, true).map(|code| code.unwrap_or(0))
    }
    #[cfg(not(windows))]
    {
        let status = Command::new(program)
            .args(split_args(params))
            .current_dir(workdir)
            .status()
            .map_err(|e| Error::Other(format!("{}: {e}", program.display())))?;
        Ok(status.code().map_or(1, |c| c as u32))
    }
}

/// Starts a game from its working folder and returns at once.
pub fn launch(target: &LaunchTarget) -> Result<()> {
    let exe = &target.exe;
    if !exe.exists() {
        return Err(Error::Invalid("launch_target"));
    }
    let workdir = target
        .workdir
        .clone()
        .filter(|w| w.is_dir())
        .or_else(|| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    start(exe, &target.args, &workdir)
}

#[cfg(windows)]
fn start(exe: &Path, args: &str, workdir: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    if exe
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        let mut cmd = Command::new(exe);
        cmd.current_dir(workdir);
        if !args.trim().is_empty() {
            cmd.raw_arg(args);
        }
        match cmd.spawn() {
            Ok(child) => {
                reap(child);
                return Ok(());
            }
            // ERROR_ELEVATION_REQUIRED: the game asks for administrator rights; the shell asks
            // the user.
            Err(e) if e.raw_os_error() == Some(740) => {}
            Err(e) => return Err(Error::Other(format!("{}: {e}", exe.display()))),
        }
    }
    // Shortcuts, batch files and elevated programs.
    super::windows::shell_execute(exe, args, workdir, false).map(drop)
}

#[cfg(unix)]
fn start(exe: &Path, args: &str, workdir: &Path) -> Result<()> {
    let args = split_args(args);
    let is = |ext: &str| exe.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
    let mut cmd = if cfg!(target_os = "macos") && is("app") {
        let mut c = Command::new("open");
        c.arg("-a").arg(exe);
        if !args.is_empty() {
            c.arg("--args").args(&args);
        }
        c
    } else if is("sh") {
        let mut c = Command::new("sh");
        c.arg(exe).args(&args);
        c
    } else {
        make_executable(exe);
        let mut c = Command::new(exe);
        c.args(&args);
        c
    };
    let child = cmd
        .current_dir(workdir)
        .spawn()
        .map_err(|e| Error::Other(format!("{}: {e}", exe.display())))?;
    reap(child);
    Ok(())
}

/// Waits for a started game on a thread of its own, so it leaves no zombie process behind.
fn reap(mut child: std::process::Child) {
    let _ = std::thread::Builder::new()
        .name("gamelib-game".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

/// Adds the execute bits a Linux game lost (for example in a zip made on Windows).
#[cfg(unix)]
pub fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = path.metadata()
        && meta.is_file()
    {
        let mode = meta.permissions().mode();
        if mode & 0o111 == 0 {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755));
        }
    }
}

/// An uninstaller in an install folder: Inno Setup's `unins000.exe` (the lowest number), or
/// another installer's `uninstall.exe`.
pub fn find_uninstaller(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name.ends_with(".exe") && (name.starts_with("unins") || name.starts_with("uninstall"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_uninstallers() {
        let dir = std::env::temp_dir().join(format!("gamelib-unins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["game.exe", "unins001.exe", "unins000.exe", "unins000.dat"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(find_uninstaller(&dir), Some(dir.join("unins000.exe")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn launches_and_runs_programs() {
        let dir = std::env::temp_dir().join(format!("gamelib-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("game.sh");
        std::fs::write(&script, "#!/bin/sh\necho \"$1\" > started.txt\n").unwrap();
        launch(&LaunchTarget {
            exe: script,
            args: "\"hello world\"".into(),
            workdir: None,
        })
        .unwrap();
        let marker = dir.join("started.txt");
        for _ in 0..100 {
            if std::fs::read_to_string(&marker).is_ok_and(|s| !s.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            "hello world"
        );
        assert!(matches!(
            launch(&LaunchTarget {
                exe: dir.join("missing"),
                args: String::new(),
                workdir: None
            }),
            Err(Error::Invalid("launch_target"))
        ));
        assert_eq!(
            run_and_wait(Path::new("/bin/sh"), "-c \"exit 3\"", &dir).unwrap(),
            3
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
