//! What to start once a game is installed: the store's own description when it has one (GOG's
//! `goggame-<id>.info`, an itch.io `.itch.toml`), otherwise a guess among the executables, the
//! way itch.io's app picks one.

use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::Platform;

/// How deep to look for executables.
const MAX_DEPTH: usize = 5;
/// Candidates offered when the guess is uncertain.
const MAX_CANDIDATES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    /// Absolute path of the program.
    pub exe: PathBuf,
    /// Arguments as one command-line string (GOG stores them that way).
    pub args: String,
    pub workdir: Option<PathBuf>,
}

impl LaunchTarget {
    pub fn plain(exe: PathBuf) -> Self {
        Self {
            exe,
            args: String::new(),
            workdir: None,
        }
    }
}

/// The primary play task of an installed GOG game.
pub fn gog_play_task(dir: &Path, product_id: &str) -> Option<LaunchTarget> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Info {
        #[serde(default)]
        play_tasks: Vec<Task>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Task {
        #[serde(default)]
        is_primary: bool,
        #[serde(rename = "type")]
        kind: Option<String>,
        category: Option<String>,
        path: Option<String>,
        arguments: Option<String>,
        working_dir: Option<String>,
    }

    let preferred = dir.join(format!("goggame-{product_id}.info"));
    let file = if preferred.is_file() {
        preferred
    } else {
        // DLC or pack installs name the file after the base game.
        fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("goggame-") && n.ends_with(".info"))
            })?
    };
    let info: Info = serde_json::from_str(&fs::read_to_string(file).ok()?).ok()?;
    let files: Vec<&Task> = info
        .play_tasks
        .iter()
        .filter(|t| t.kind.as_deref() == Some("FileTask") && t.path.is_some())
        .collect();
    let task = files
        .iter()
        .find(|t| t.is_primary)
        .or_else(|| files.iter().find(|t| t.category.as_deref() == Some("game")))
        .or_else(|| files.first())?;
    let exe = join_relative(dir, task.path.as_deref()?)?;
    Some(LaunchTarget {
        exe,
        args: task.arguments.clone().unwrap_or_default(),
        workdir: task
            .working_dir
            .as_deref()
            .and_then(|w| join_relative(dir, w)),
    })
}

/// The "play" action of an itch.io manifest (`.itch.toml`) for this platform.
pub fn itch_manifest(dir: &Path, platform: Platform) -> Option<LaunchTarget> {
    #[derive(Deserialize)]
    struct Manifest {
        #[serde(default)]
        actions: Vec<Action>,
    }
    #[derive(Deserialize)]
    struct Action {
        name: Option<String>,
        path: Option<String>,
        platform: Option<String>,
        #[serde(default)]
        args: Vec<String>,
    }

    let text = fs::read_to_string(dir.join(".itch.toml")).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    let ours = |a: &&Action| match a.platform.as_deref() {
        None => true,
        Some("windows") => platform == Platform::Win,
        Some("osx") | Some("macos") => platform == Platform::Mac,
        Some("linux") => platform == Platform::Linux,
        Some(_) => false,
    };
    let actions: Vec<&Action> = manifest.actions.iter().filter(ours).collect();
    let action = actions
        .iter()
        .find(|a| {
            a.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("play"))
        })
        .or_else(|| actions.first())?;
    let ext = if platform == Platform::Win {
        ".exe"
    } else {
        ""
    };
    let path = action.path.as_deref()?.replace("{{EXT}}", ext);
    if path.contains("://") {
        return None;
    }
    let exe = join_relative(dir, &path)?;
    exe.exists().then(|| LaunchTarget {
        exe,
        args: join_args(&action.args),
        workdir: None,
    })
}

/// Programs in `dir` that could be the game, most likely first: shallow before deep, names that
/// do not look like tools first, then larger files.
pub fn candidates(dir: &Path, platform: Platform) -> Vec<PathBuf> {
    let mut found: Vec<(usize, bool, u64, PathBuf)> = Vec::new();
    walk(dir, 0, platform, &mut found);
    found.sort_by_key(|(depth, tool, size, path)| (*depth, *tool, Reverse(*size), path.clone()));
    found
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(.., p)| p)
        .collect()
}

/// The best guess, if any.
pub fn guess(dir: &Path, platform: Platform) -> Option<LaunchTarget> {
    candidates(dir, platform)
        .into_iter()
        .next()
        .map(LaunchTarget::plain)
}

fn walk(
    dir: &Path,
    depth: usize,
    platform: Platform,
    found: &mut Vec<(usize, bool, u64, PathBuf)>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            if platform == Platform::Mac && name.ends_with(".app") {
                found.push((depth, looks_like_tool(&name), dir_size(&path), path));
            } else if !IGNORED_DIRS.contains(&name.as_str()) {
                walk(&path, depth + 1, platform, found);
            }
            continue;
        }
        if is_program(&name, &meta, platform) && !is_helper(&name) {
            found.push((depth, looks_like_tool(&name), meta.len(), path));
        }
    }
}

/// Folders of redistributables and installers bundled with games.
const IGNORED_DIRS: &[&str] = &[
    "__redist",
    "_redist",
    "redist",
    "_commonredist",
    "commonredist",
    "redistributables",
    "directx",
    "vcredist",
    "dotnetfx",
    "__installer",
    "__macosx",
];

/// Programs that come with games but never are the game.
fn is_helper(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "unins",
        "vcredist",
        "vc_redist",
        "dxsetup",
        "dxwebsetup",
        "dotnet",
        "ndp4",
        "oalinst",
        "physx",
    ];
    const PARTS: &[&str] = &[
        "crashhandler",
        "crashreport",
        "crashpad",
        "ueprereq",
        "ue4prereq",
        "notification_helper",
        "createdump",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p)) || PARTS.iter().any(|p| name.contains(p))
}

/// Names that suggest a tool rather than the game; they are only picked when nothing else is.
fn looks_like_tool(name: &str) -> bool {
    [
        "setup",
        "install",
        "config",
        "settings",
        "editor",
        "server",
        "update",
        "patch",
        "benchmark",
        "uninstall",
    ]
    .iter()
    .any(|w| name.contains(w))
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

fn is_program(name: &str, meta: &fs::Metadata, platform: Platform) -> bool {
    match platform {
        Platform::Win => name.ends_with(".exe"),
        Platform::Linux => {
            let library = name.ends_with(".so") || name.contains(".so.");
            let known = [".x86_64", ".x86", ".sh", ".appimage"]
                .iter()
                .any(|e| name.ends_with(e));
            !library && (known || (is_executable(meta) && !name.contains('.')))
        }
        // Mac games are .app bundles, found as folders.
        Platform::Mac => false,
    }
}

fn dir_size(dir: &Path) -> u64 {
    fn go(dir: &Path, depth: usize) -> u64 {
        if depth > 8 {
            return 0;
        }
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| match e.metadata() {
                        Ok(m) if m.is_dir() => go(&e.path(), depth + 1),
                        Ok(m) => m.len(),
                        Err(_) => 0,
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    go(dir, 0)
}

/// `bin\x64\game.exe` under `dir`, refusing anything that leaves it.
fn join_relative(dir: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = dir.to_path_buf();
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') => return None,
            p => out.push(p),
        }
    }
    Some(out)
}

/// Arguments as one command line, quoted the Windows way when they contain spaces.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains([' ', '\t', '"']) {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits a command line into arguments (double quotes group words), for Linux and macOS.
pub fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
                any = true;
            }
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    args.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gamelib-targets-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, size: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; size]).unwrap();
    }

    #[test]
    fn gog_primary_task() {
        let dir = temp_dir("gog");
        fs::write(
            dir.join("goggame-1207664643.info"),
            r#"{"gameId":"1207664643","name":"The Witcher 3","playTasks":[
                {"category":"document","type":"FileTask","path":"manual.pdf","name":"Manual"},
                {"category":"game","isPrimary":true,"type":"FileTask","path":"bin\\x64\\witcher3.exe","workingDir":"bin\\x64","arguments":"-dx12","name":"The Witcher 3"},
                {"category":"launcher","type":"URLTask","link":"https://www.gog.com"}]}"#,
        )
        .unwrap();
        let target = gog_play_task(&dir, "1207664643").unwrap();
        assert_eq!(target.exe, dir.join("bin").join("x64").join("witcher3.exe"));
        assert_eq!(target.args, "-dx12");
        assert_eq!(target.workdir, Some(dir.join("bin").join("x64")));
        // Any goggame file works when the product id differs (a pack).
        assert!(gog_play_task(&dir, "999").is_some());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn itch_manifest_for_this_platform() {
        let dir = temp_dir("itch");
        touch(&dir.join("Game.exe"), 10);
        touch(&dir.join("game.x86_64"), 10);
        fs::write(
            dir.join(".itch.toml"),
            r#"
[[actions]]
name = "play"
path = "game.x86_64"
platform = "linux"

[[actions]]
name = "play"
path = "Game{{EXT}}"
args = ["--fullscreen", "two words"]
platform = "windows"
"#,
        )
        .unwrap();
        let win = itch_manifest(&dir, Platform::Win).unwrap();
        assert_eq!(win.exe, dir.join("Game.exe"));
        assert_eq!(win.args, "--fullscreen \"two words\"");
        assert_eq!(
            itch_manifest(&dir, Platform::Linux).unwrap().exe,
            dir.join("game.x86_64")
        );
        assert_eq!(itch_manifest(&dir, Platform::Mac), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn guesses_the_game_among_programs() {
        let dir = temp_dir("guess");
        touch(&dir.join("Game").join("Game.exe"), 5_000);
        touch(&dir.join("Game").join("UnityCrashHandler64.exe"), 9_000);
        touch(&dir.join("Game").join("Config.exe"), 20_000);
        touch(&dir.join("Game").join("unins000.exe"), 30_000);
        touch(
            &dir.join("Game")
                .join("_CommonRedist")
                .join("vcredist_x64.exe"),
            90_000,
        );
        touch(&dir.join("Game").join("Tools").join("Big.exe"), 99_000);
        let found = candidates(&dir, Platform::Win);
        assert_eq!(
            found,
            [
                dir.join("Game").join("Game.exe"),
                dir.join("Game").join("Config.exe"),
                dir.join("Game").join("Tools").join("Big.exe"),
            ]
        );
        assert_eq!(
            guess(&dir, Platform::Win).unwrap().exe,
            dir.join("Game").join("Game.exe")
        );
        assert_eq!(guess(&dir, Platform::Mac), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn arguments() {
        assert_eq!(join_args(&["-a".into(), "b c".into()]), "-a \"b c\"");
        assert_eq!(
            split_args(r#"-dx12 "C:\My Games" --x="y z" \"q\""#),
            ["-dx12", r"C:\My Games", "--x=y z", "\"q\""]
        );
        assert!(split_args("   ").is_empty());
        assert_eq!(split_args(r#""""#), [""]);
    }

    #[test]
    fn relative_paths_stay_inside() {
        let dir = Path::new("/g");
        assert_eq!(
            join_relative(dir, "bin\\x.exe"),
            Some(PathBuf::from("/g/bin/x.exe"))
        );
        assert_eq!(join_relative(dir, "..\\x.exe"), None);
        assert_eq!(join_relative(dir, "C:\\x.exe"), None);
    }
}
