//! What a downloaded file is, judged by its bytes rather than its name.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// How far into a Windows program to look for an installer's signature: the setup loader and
/// its resources come first, the (large) payload after them.
const SCAN_LIMIT: u64 = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Zip,
    SevenZip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    Rar,
    /// An Inno Setup installer (every GOG installer for Windows).
    InnoSetup,
    /// A Nullsoft (NSIS) installer.
    Nsis,
    /// A Windows Installer package.
    Msi,
    /// Another Windows program whose name says it installs something.
    Installer,
    /// Another Windows program: most likely the game itself.
    Exe,
    /// A Linux program (an AppImage too).
    LinuxProgram,
    /// A shell script (GOG's Linux installers).
    Script,
    /// A macOS disk image or installer package.
    MacPackage,
    /// Anything else (a lone .gz, a document…).
    Other,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Zip => "zip",
            FileKind::SevenZip => "seven_zip",
            FileKind::Tar => "tar",
            FileKind::TarGz => "tar_gz",
            FileKind::TarBz2 => "tar_bz2",
            FileKind::TarXz => "tar_xz",
            FileKind::Rar => "rar",
            FileKind::InnoSetup => "inno_setup",
            FileKind::Nsis => "nsis",
            FileKind::Msi => "msi",
            FileKind::Installer => "installer",
            FileKind::Exe => "exe",
            FileKind::LinuxProgram => "linux_program",
            FileKind::Script => "script",
            FileKind::MacPackage => "mac_package",
            FileKind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        const ALL: [FileKind; 16] = [
            FileKind::Zip,
            FileKind::SevenZip,
            FileKind::Tar,
            FileKind::TarGz,
            FileKind::TarBz2,
            FileKind::TarXz,
            FileKind::Rar,
            FileKind::InnoSetup,
            FileKind::Nsis,
            FileKind::Msi,
            FileKind::Installer,
            FileKind::Exe,
            FileKind::LinuxProgram,
            FileKind::Script,
            FileKind::MacPackage,
            FileKind::Other,
        ];
        ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Unpacked by GameLib itself.
    pub fn is_archive(self) -> bool {
        matches!(
            self,
            FileKind::Zip
                | FileKind::SevenZip
                | FileKind::Tar
                | FileKind::TarGz
                | FileKind::TarBz2
                | FileKind::TarXz
        )
    }

    /// Someone else's installer, which GameLib can only run.
    pub fn is_installer(self) -> bool {
        matches!(
            self,
            FileKind::InnoSetup | FileKind::Nsis | FileKind::Msi | FileKind::Installer
        )
    }
}

/// Signatures found inside a Windows program.
const NSIS: &[u8] = b"NullsoftInst";
const INNO: [&[u8]; 3] = [b"Inno Setup Setup Data", b"rDlPtS", b"InnoSetupLdrWindow"];

pub fn inspect(path: &Path) -> Result<FileKind> {
    let mut file = File::open(path).map_err(|e| io(path, e))?;
    let mut head = [0u8; 512];
    let n = read_up_to(&mut file, &mut head).map_err(|e| io(path, e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let head = &head[..n];
    if head.starts_with(b"MZ") {
        let markers = scan(path, SCAN_LIMIT).map_err(|e| io(path, e))?;
        return Ok(windows_program(markers, &name));
    }
    Ok(match classify(head, &name) {
        Some(kind) => kind,
        None if head.starts_with(&[0x1F, 0x8B]) => compressed_tar(
            flate2::read::GzDecoder::new(File::open(path).map_err(|e| io(path, e))?),
            FileKind::TarGz,
        ),
        None if head.starts_with(b"BZh") => compressed_tar(
            bzip2::read::BzDecoder::new(File::open(path).map_err(|e| io(path, e))?),
            FileKind::TarBz2,
        ),
        None if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) => compressed_tar(
            lzma_rust2::XzReader::new(File::open(path).map_err(|e| io(path, e))?, true),
            FileKind::TarXz,
        ),
        None => FileKind::Other,
    })
}

/// Recognizes a file from its first bytes (not Windows programs or compressed tars).
fn classify(head: &[u8], name: &str) -> Option<FileKind> {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    Some(
        if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") || at(0, b"PK\x07\x08") {
            FileKind::Zip
        } else if at(0, &[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
            FileKind::SevenZip
        } else if at(0, b"Rar!\x1A\x07") {
            FileKind::Rar
        } else if at(257, b"ustar") {
            FileKind::Tar
        } else if at(0, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            // Also Office documents; only a .msi is an installer.
            if name.ends_with(".msi") {
                FileKind::Msi
            } else {
                FileKind::Other
            }
        } else if at(0, b"xar!") || name.ends_with(".dmg") || name.ends_with(".pkg") {
            FileKind::MacPackage
        } else if at(0, b"#!") {
            FileKind::Script
        } else if at(0, b"\x7FELF") {
            FileKind::LinuxProgram
        } else {
            return None;
        },
    )
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Markers {
    nsis: bool,
    inno: bool,
}

fn windows_program(markers: Markers, name: &str) -> FileKind {
    if markers.inno {
        FileKind::InnoSetup
    } else if markers.nsis {
        FileKind::Nsis
    } else if ["setup", "install"].iter().any(|w| name.contains(w)) {
        FileKind::Installer
    } else {
        FileKind::Exe
    }
}

/// Looks for installer signatures in the first `limit` bytes.
fn scan(path: &Path, limit: u64) -> std::io::Result<Markers> {
    let longest = INNO
        .iter()
        .chain([&NSIS])
        .map(|m| m.len())
        .max()
        .unwrap_or(0);
    let mut file = File::open(path)?.take(limit);
    let mut buf = vec![0u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    let mut markers = Markers::default();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(markers);
        }
        let mut window = std::mem::take(&mut carry);
        window.extend_from_slice(&buf[..n]);
        markers.nsis |= contains(&window, NSIS);
        markers.inno |= INNO.iter().any(|m| contains(&window, m));
        if markers.inno {
            return Ok(markers);
        }
        let keep = window.len().min(longest - 1);
        carry = window[window.len() - keep..].to_vec();
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A tar inside a compressed stream, or just a compressed file.
fn compressed_tar(mut reader: impl Read, kind: FileKind) -> FileKind {
    let mut head = [0u8; 512];
    match read_up_to(&mut reader, &mut head) {
        Ok(n) if n == 512 && &head[257..262] == b"ustar" => kind,
        _ => FileKind::Other,
    }
}

fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Other(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gamelib-inspect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn recognizes_archives_and_packages() {
        assert_eq!(classify(b"PK\x03\x04rest", "a.zip"), Some(FileKind::Zip));
        assert_eq!(
            classify(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4], "a.7z"),
            Some(FileKind::SevenZip)
        );
        assert_eq!(
            classify(b"Rar!\x1A\x07\x01\x00", "a.rar"),
            Some(FileKind::Rar)
        );
        let mut tar = vec![0u8; 512];
        tar[257..262].copy_from_slice(b"ustar");
        assert_eq!(classify(&tar, "a.tar"), Some(FileKind::Tar));
        let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        assert_eq!(classify(&ole, "setup.msi"), Some(FileKind::Msi));
        assert_eq!(classify(&ole, "manual.doc"), Some(FileKind::Other));
        assert_eq!(
            classify(b"#!/bin/sh\n", "gog_game_1.0.sh"),
            Some(FileKind::Script)
        );
        assert_eq!(
            classify(b"\x7FELF\x02\x01", "game.x86_64"),
            Some(FileKind::LinuxProgram)
        );
        assert_eq!(
            classify(b"xar!\x00\x1c", "game.pkg"),
            Some(FileKind::MacPackage)
        );
        assert_eq!(classify(b"hello", "readme.txt"), None);
    }

    #[test]
    fn tells_installers_from_games() {
        let mut exe = b"MZ\x90\x00".to_vec();
        exe.resize(4096, 0);
        let mut inno = exe.clone();
        inno.extend_from_slice(b"....Inno Setup Setup Data (6.2.0)....");
        let mut nsis = exe.clone();
        nsis.extend_from_slice(&[0xEF, 0xBE, 0xAD, 0xDE]);
        nsis.extend_from_slice(b"NullsoftInst");

        assert_eq!(
            inspect(&temp("setup_game_1.0.exe", &inno)).unwrap(),
            FileKind::InnoSetup
        );
        assert_eq!(
            inspect(&temp("game-installer.exe", &nsis)).unwrap(),
            FileKind::Nsis
        );
        assert_eq!(inspect(&temp("Game.exe", &exe)).unwrap(), FileKind::Exe);
        assert_eq!(
            inspect(&temp("GameSetup.exe", &exe)).unwrap(),
            FileKind::Installer
        );
    }

    #[test]
    fn signatures_split_across_reads_are_found() {
        // The marker straddles the 1 MB read boundary.
        let mut exe = b"MZ".to_vec();
        exe.resize((1 << 20) - 4, 0);
        exe.extend_from_slice(b"NullsoftInst");
        exe.resize((1 << 20) + 64, 0);
        assert_eq!(inspect(&temp("big.exe", &exe)).unwrap(), FileKind::Nsis);
    }

    #[test]
    fn compressed_tars() {
        let mut tar = vec![0u8; 1024];
        tar[257..262].copy_from_slice(b"ustar");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(inspect(&temp("game.tar.gz", &gz)).unwrap(), FileKind::TarGz);
        let mut lone = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut lone, b"just text").unwrap();
        assert_eq!(
            inspect(&temp("notes.gz", &lone.finish().unwrap())).unwrap(),
            FileKind::Other
        );
    }

    #[test]
    fn kinds_round_trip() {
        for kind in [
            FileKind::Zip,
            FileKind::TarXz,
            FileKind::InnoSetup,
            FileKind::MacPackage,
            FileKind::Other,
        ] {
            assert_eq!(FileKind::parse(kind.as_str()), Some(kind));
        }
        assert!(FileKind::Zip.is_archive() && !FileKind::Rar.is_archive());
        assert!(FileKind::Msi.is_installer() && !FileKind::Exe.is_installer());
    }
}
