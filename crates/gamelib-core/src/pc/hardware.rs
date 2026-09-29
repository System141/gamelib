//! What this computer has, for comparing with a game's system requirements: memory, graphics,
//! Windows version and the disk games go to. Everything is best effort; what cannot be read
//! stays `None` and its checks read "unknown".

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThisPc {
    /// "Windows 11 24H2 (26100)", "Ubuntu 24.04.1 LTS".
    pub os: String,
    /// The Windows version as a number (10, 11) for comparisons; `None` on other systems.
    pub windows: Option<f32>,
    pub bits64: bool,
    pub cpu: Option<String>,
    /// Logical processors.
    pub cores: Option<u32>,
    /// Installed memory in bytes.
    pub memory: Option<u64>,
    pub gpu: Option<String>,
    /// Dedicated video memory of that graphics card, in bytes.
    pub video_memory: Option<u64>,
    /// The highest DirectX (Direct3D feature level) the graphics card supports: 12, 11, …
    pub directx: Option<u32>,
    /// Free space on the drive of the library folder, where games are installed.
    pub disk_free: Option<u64>,
    /// Whether that drive is an SSD.
    pub disk_ssd: Option<bool>,
    pub disk_path: String,
}

/// Reads this computer. `library_dir` decides which drive's free space counts; it need not
/// exist yet.
pub fn detect(library_dir: &Path) -> ThisPc {
    let mut pc = platform::detect();
    pc.bits64 = cfg!(target_pointer_width = "64");
    pc.cores = std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u32);
    let existing = library_dir.ancestors().find(|p| p.exists());
    pc.disk_free = existing.and_then(|p| fs4::available_space(p).ok());
    pc.disk_ssd = existing.and_then(platform::is_ssd);
    pc.disk_path = library_dir.display().to_string();
    pc
}

#[cfg(windows)]
mod platform {
    pub use super::super::windows::{detect, is_ssd};
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::Path;

    use super::ThisPc;

    pub fn detect() -> ThisPc {
        let os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "Linux".to_owned());
        let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        });
        let memory = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("MemTotal:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
                .map(|kb| kb * 1024)
        });
        let (gpu, video_memory) = graphics();
        ThisPc {
            os,
            cpu,
            memory,
            gpu,
            video_memory,
            ..ThisPc::default()
        }
    }

    /// The graphics card with the most video memory, named from the PCI id list when the
    /// system has one. Only AMD's driver reports video memory there.
    fn graphics() -> (Option<String>, Option<u64>) {
        let Ok(cards) = std::fs::read_dir("/sys/class/drm") else {
            return (None, None);
        };
        let mut best: Option<(String, Option<u64>)> = None;
        for card in cards.flatten() {
            let name = card.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let device = card.path().join("device");
            let read = |f: &str| std::fs::read_to_string(device.join(f)).ok();
            let (Some(vendor), Some(id)) = (read("vendor"), read("device")) else {
                continue;
            };
            let vram = read("mem_info_vram_total").and_then(|v| v.trim().parse::<u64>().ok());
            let label = pci_name(vendor.trim(), id.trim())
                .unwrap_or_else(|| format!("PCI {}:{}", vendor.trim(), id.trim()));
            if best.as_ref().is_none_or(|(_, b)| vram > *b) {
                best = Some((label, vram));
            }
        }
        best.map_or((None, None), |(name, vram)| (Some(name), vram))
    }

    fn pci_name(vendor: &str, device: &str) -> Option<String> {
        let vendor = vendor.trim_start_matches("0x").to_lowercase();
        let device = device.trim_start_matches("0x").to_lowercase();
        let ids = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"]
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())?;
        let mut vendor_name = None;
        for line in ids.lines() {
            if let Some(rest) = line
                .strip_prefix(&vendor)
                .filter(|_| !line.starts_with('\t'))
            {
                vendor_name = Some(rest.trim().to_owned());
                continue;
            }
            if vendor_name.is_some() {
                if !line.starts_with('\t') && !line.starts_with('#') && !line.is_empty() {
                    break;
                }
                if let Some(rest) = line
                    .strip_prefix('\t')
                    .and_then(|l| l.strip_prefix(&device))
                {
                    return Some(format!("{} {}", vendor_name?, rest.trim()));
                }
            }
        }
        vendor_name
    }

    /// Whether the drive holding `path` is an SSD (not rotational).
    pub fn is_ssd(path: &Path) -> Option<bool> {
        use std::os::unix::fs::MetadataExt;
        let dev = path.metadata().ok()?.dev();
        let (major, minor) = (libc_major(dev), libc_minor(dev));
        let base = std::path::PathBuf::from(format!("/sys/dev/block/{major}:{minor}"));
        let rotational = std::fs::read_to_string(base.join("queue/rotational"))
            .or_else(|_| std::fs::read_to_string(base.join("../queue/rotational")))
            .ok()?;
        Some(rotational.trim() == "0")
    }

    // glibc's encoding of device numbers.
    fn libc_major(dev: u64) -> u64 {
        ((dev >> 32) & 0xffff_f000) | ((dev >> 8) & 0x0000_0fff)
    }

    fn libc_minor(dev: u64) -> u64 {
        ((dev >> 12) & 0xffff_ff00) | (dev & 0x0000_00ff)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;
    use std::process::Command;

    use super::ThisPc;

    fn sysctl(name: &str) -> Option<String> {
        let out = Command::new("sysctl").args(["-n", name]).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    pub fn detect() -> ThisPc {
        let version = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
        ThisPc {
            os: format!("macOS {}", version.unwrap_or_default())
                .trim()
                .to_owned(),
            cpu: sysctl("machdep.cpu.brand_string"),
            memory: sysctl("hw.memsize").and_then(|m| m.parse().ok()),
            ..ThisPc::default()
        }
    }

    pub fn is_ssd(_path: &Path) -> Option<bool> {
        None
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use std::path::Path;

    use super::ThisPc;

    pub fn detect() -> ThisPc {
        ThisPc {
            os: std::env::consts::OS.to_owned(),
            ..ThisPc::default()
        }
    }

    pub fn is_ssd(_path: &Path) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_computer_without_failing() {
        let dir = std::env::temp_dir()
            .join("gamelib-not-created-yet")
            .join("Games");
        let pc = detect(&dir);
        assert!(!pc.os.is_empty());
        assert!(pc.cores.is_some_and(|c| c >= 1));
        assert!(
            pc.disk_free.is_some(),
            "free space of the nearest existing folder"
        );
        assert_eq!(pc.disk_path, dir.display().to_string());
        if cfg!(any(windows, target_os = "linux")) {
            assert!(pc.memory.is_some_and(|m| m > 256 << 20), "{pc:?}");
        }
        if cfg!(windows) {
            assert!(pc.windows.is_some_and(|w| w >= 10.0), "{pc:?}");
            assert!(pc.cpu.is_some(), "{pc:?}");
        }
    }
}
