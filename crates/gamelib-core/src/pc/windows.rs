//! Windows only: installed memory, the graphics card (DXGI) and the DirectX level it supports,
//! the processor and Windows version (registry), and whether a drive is an SSD.

use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HMODULE};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2,
    D3D_FEATURE_LEVEL_9_3, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_12_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory1,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, GetVolumePathNameW,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery,
    STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty,
};
use windows::Win32::System::SystemInformation::{
    GetPhysicallyInstalledSystemMemory, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows::core::HSTRING;

use super::hardware::ThisPc;

pub fn detect() -> ThisPc {
    let (os, windows) = windows_version();
    let (gpu, video_memory, directx) = graphics().unwrap_or_default();
    ThisPc {
        os,
        windows,
        cpu: processor(),
        memory: memory(),
        gpu,
        video_memory,
        directx,
        ..ThisPc::default()
    }
}

/// "Windows 11 24H2 (26100)" and 11. Windows 11 still calls itself "Windows 10" in the
/// registry's product name; the build number tells them apart.
fn windows_version() -> (String, Option<f32>) {
    let Ok(key) =
        windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    else {
        return ("Windows".into(), None);
    };
    let build: Option<u32> = key
        .get_string("CurrentBuild")
        .ok()
        .and_then(|b| b.trim().parse().ok());
    let release = key
        .get_string("DisplayVersion")
        .or_else(|_| key.get_string("ReleaseId"))
        .ok()
        .filter(|r| !r.trim().is_empty());
    let (name, version) = match build {
        Some(b) if b >= 22_000 => ("Windows 11".to_owned(), Some(11.0)),
        Some(b) if b >= 10_240 => ("Windows 10".to_owned(), Some(10.0)),
        Some(b) if b >= 9_600 => ("Windows 8.1".to_owned(), Some(8.1)),
        Some(b) if b >= 9_200 => ("Windows 8".to_owned(), Some(8.0)),
        Some(_) => ("Windows 7".to_owned(), Some(7.0)),
        None => (
            key.get_string("ProductName")
                .unwrap_or_else(|_| "Windows".into()),
            None,
        ),
    };
    let detail = match (release, build) {
        (Some(r), Some(b)) => format!(" {r} ({b})"),
        (None, Some(b)) => format!(" ({b})"),
        _ => String::new(),
    };
    (format!("{name}{detail}"), version)
}

fn processor() -> Option<String> {
    windows_registry::LOCAL_MACHINE
        .open(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .ok()?
        .get_string("ProcessorNameString")
        .ok()
        .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|n| !n.is_empty())
}

/// Installed memory (the size of the modules), or what Windows can use when that is unknown.
fn memory() -> Option<u64> {
    let mut kb = 0u64;
    if unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }.is_ok() && kb > 0 {
        return Some(kb * 1024);
    }
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some(status.ullTotalPhys)
}

/// The hardware adapter with the most video memory: its name, video memory and DirectX level.
fn graphics() -> Option<(Option<String>, Option<u64>, Option<u32>)> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<(IDXGIAdapter1, String, u64)> = None;
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let memory = desc.DedicatedVideoMemory as u64;
            if best.as_ref().is_none_or(|(_, _, m)| memory > *m) {
                let end = desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..end])
                    .trim()
                    .to_owned();
                best = Some((adapter, name, memory));
            }
        }
        let (adapter, name, memory) = best?;
        Some((
            Some(name).filter(|n| !n.is_empty()),
            Some(memory).filter(|&m| m > 0),
            directx_level(&adapter),
        ))
    }
}

/// The highest Direct3D feature level of `adapter`, as a DirectX version (12, 11, 10, 9).
fn directx_level(adapter: &IDXGIAdapter1) -> Option<u32> {
    const ALL: [D3D_FEATURE_LEVEL; 9] = [
        D3D_FEATURE_LEVEL_12_1,
        D3D_FEATURE_LEVEL_12_0,
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_9_3,
        D3D_FEATURE_LEVEL_9_2,
        D3D_FEATURE_LEVEL_9_1,
    ];
    // Runtimes older than Direct3D 11.3 reject the 12_x levels outright; ask again without.
    for levels in [&ALL[..], &ALL[2..]] {
        let mut level = D3D_FEATURE_LEVEL::default();
        // Without a device to return, this only reports the highest supported level.
        let found = unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(levels),
                D3D11_SDK_VERSION,
                None,
                Some(&mut level),
                None,
            )
        };
        if found.is_ok() {
            return Some(match level.0 {
                l if l >= D3D_FEATURE_LEVEL_12_0.0 => 12,
                l if l >= D3D_FEATURE_LEVEL_11_0.0 => 11,
                l if l >= D3D_FEATURE_LEVEL_10_0.0 => 10,
                _ => 9,
            });
        }
    }
    None
}

/// Whether the drive holding `path` is an SSD: drives without a seek penalty are.
pub fn is_ssd(path: &Path) -> Option<bool> {
    let mut root = [0u16; 261];
    unsafe { GetVolumePathNameW(&HSTRING::from(path), &mut root) }.ok()?;
    let end = root.iter().position(|&c| c == 0)?;
    let volume = String::from_utf16_lossy(&root[..end]);
    // "C:\" → "\\.\C:"; mounted folders and network shares have no such device.
    let letter = volume
        .strip_suffix('\\')
        .filter(|v| v.len() == 2 && v.ends_with(':'))?;
    let device = HSTRING::from(format!(r"\\.\{letter}"));
    unsafe {
        let handle = CreateFileW(
            &device,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceSeekPenaltyProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        let mut result = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
        let mut returned = 0u32;
        let asked = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut result as *mut _ as *mut _),
            size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut returned),
            None,
        );
        let _ = CloseHandle(handle);
        asked.ok()?;
        (returned as usize >= size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>())
            .then_some(!result.IncursSeekPenalty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_graphics_card_and_system_drive_without_failing() {
        // A build machine may only have Microsoft's software renderer: no card is fine.
        let _ = graphics();
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let _ = is_ssd(Path::new(&system));
        let (os, version) = windows_version();
        assert!(os.starts_with("Windows"), "{os}");
        assert!(version.is_some_and(|v| v >= 10.0), "{os}");
    }
}
