//! Windows only: running programs through the shell (so installers can ask for administrator
//! rights), marking downloads as coming from the internet with an antivirus check, and reading
//! the games GOG Galaxy (or GOG's installers) registered.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    AttachmentServices, IAttachmentExecute, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
    SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{GUID, HRESULT, HSTRING, PCWSTR};

use crate::{Error, Result};

/// GameLib's client id for Windows' Attachment Services.
const CLIENT_GUID: GUID = GUID::from_u128(0x6b1c3f2e_9a4d_4f7b_8e21_5c0d7a93e4b1);
/// The antivirus found (and removed) something.
const VIRUS_INFECTED: HRESULT = HRESULT::from_win32(225);
const VIRUS_DELETED: HRESULT = HRESULT::from_win32(226);
/// Policy (zone settings) forbids the file.
const SECURITY_PROBLEM: HRESULT = HRESULT(0x800C_000E_u32 as i32);

/// Closes a process handle when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Runs `file` like a double-click would: a program whose manifest asks for administrator
/// rights shows the UAC prompt, and refusing it is `Cancelled`. With `wait`, returns the exit
/// code once it ends.
pub fn shell_execute(file: &Path, params: &str, dir: &Path, wait: bool) -> Result<Option<u32>> {
    let file_w = HSTRING::from(file);
    let params_w = HSTRING::from(params);
    let dir_w = HSTRING::from(dir);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file_w.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        lpDirectory: PCWSTR(dir_w.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
            Error::Cancelled
        } else {
            Error::Other(format!("{}: {e}", file.display()))
        });
    }
    if info.hProcess.is_invalid() {
        // Handed to an already running program; nothing to wait for.
        return Ok(None);
    }
    let process = Handle(info.hProcess);
    if !wait {
        return Ok(None);
    }
    let mut code = 0u32;
    unsafe {
        WaitForSingleObject(process.0, INFINITE);
        GetExitCodeProcess(process.0, &mut code)
            .map_err(|e| Error::Other(format!("{}: {e}", file.display())))?;
    }
    Ok(Some(code))
}

/// Marks a downloaded file as coming from `source` on the internet (the Mark of the Web) and lets
/// the antivirus scan it, as browsers do when they save a download. Fails with `blocked` when the
/// antivirus or a policy refuses the file; other failures only skip the mark.
pub fn mark_downloaded(path: &Path, source: &str) -> Result<()> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let saved = unsafe { save_attachment(path, source) };
    if initialized {
        unsafe { CoUninitialize() };
    }
    match saved {
        Ok(()) if path.exists() => Ok(()),
        // Scanned and removed.
        Ok(()) => Err(Error::Invalid("blocked")),
        Err(e) if [VIRUS_INFECTED, VIRUS_DELETED, SECURITY_PROBLEM].contains(&e.code()) => {
            Err(Error::Invalid("blocked"))
        }
        Err(e) => {
            eprintln!("could not mark {} as downloaded: {e}", path.display());
            Ok(())
        }
    }
}

unsafe fn save_attachment(path: &Path, source: &str) -> windows::core::Result<()> {
    unsafe {
        let attachment: IAttachmentExecute =
            CoCreateInstance(&AttachmentServices, None, CLSCTX_INPROC_SERVER)?;
        attachment.SetClientGuid(&CLIENT_GUID)?;
        attachment.SetLocalPath(&HSTRING::from(path))?;
        attachment.SetSource(&HSTRING::from(source))?;
        attachment.Save()
    }
}

/// A game in GOG's registry entries (installed by GOG Galaxy or a GOG installer).
#[derive(Debug, Clone)]
pub struct RegisteredGame {
    pub product_id: String,
    pub title: String,
    pub dir: PathBuf,
    pub exe: Option<PathBuf>,
    pub args: String,
    pub workdir: Option<PathBuf>,
    pub uninstaller: Option<PathBuf>,
}

/// `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<product id>`, for folders that still exist.
pub fn gog_registered_games() -> Vec<RegisteredGame> {
    let Ok(root) = windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\WOW6432Node\GOG.com\Games")
    else {
        return Vec::new();
    };
    let Ok(keys) = root.keys() else {
        return Vec::new();
    };
    keys.filter_map(|id| {
        let key = root.open(&id).ok()?;
        let get = |name: &str| key.get_string(name).ok().filter(|v| !v.trim().is_empty());
        let dir = PathBuf::from(get("path")?);
        if !dir.is_dir() {
            return None;
        }
        Some(RegisteredGame {
            product_id: get("productID").unwrap_or(id),
            title: get("gameName").unwrap_or_default(),
            exe: get("exe").map(PathBuf::from),
            args: get("launchParam").unwrap_or_default(),
            workdir: get("workingDir").map(PathBuf::from),
            uninstaller: get("uninstallCommand").map(|c| PathBuf::from(c.trim_matches('"'))),
            dir,
        })
    })
    .collect()
}

/// Where Steam is installed, from its registry entries.
pub fn steam_path() -> Option<PathBuf> {
    let user = windows_registry::CURRENT_USER
        .open(r"Software\Valve\Steam")
        .and_then(|k| k.get_string("SteamPath"));
    let machine = || {
        windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\WOW6432Node\Valve\Steam")
            .and_then(|k| k.get_string("InstallPath"))
    };
    user.or_else(|_| machine())
        .ok()
        .map(|p| PathBuf::from(p.trim()))
        .filter(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_waits_for_exit_codes() {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let cmd = system.join("System32").join("cmd.exe");
        let code = shell_execute(&cmd, "/c exit 3", &std::env::temp_dir(), true).unwrap();
        assert_eq!(code, Some(3));
        assert!(
            shell_execute(
                Path::new(r"C:\does\not\exist.exe"),
                "",
                &std::env::temp_dir(),
                true
            )
            .is_err()
        );
    }

    #[test]
    fn marks_downloads() {
        let path = std::env::temp_dir().join(format!("gamelib-motw-{}.txt", std::process::id()));
        std::fs::write(&path, b"harmless").unwrap();
        mark_downloaded(&path, "https://www.gog.com/").unwrap();
        // The mark lives in an alternate data stream; some file systems cannot hold one.
        if let Ok(zone) = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())) {
            assert!(zone.contains("ZoneId=3"), "{zone}");
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn reads_the_registry_without_failing() {
        // Usually empty on a build machine; must never panic.
        let _ = gog_registered_games();
        let _ = steam_path();
    }
}
