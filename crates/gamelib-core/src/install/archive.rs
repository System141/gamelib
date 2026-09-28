//! Unpacking archives safely: every entry must land inside the target folder (no `..`, drive
//! letters or alternate data streams), links may only point inside it, and an entry can never
//! grow past the size its header announced.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::inspect::FileKind;
use crate::downloads::names::safe_name;
use crate::{Error, Result};

const BUFFER: usize = 1 << 20;

/// What was unpacked.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Unpacked {
    pub files: u64,
    pub bytes: u64,
}

/// The unpacked size, when the archive lists it up front (zip, 7z).
pub fn unpacked_size(kind: FileKind, archive: &Path) -> Result<Option<u64>> {
    match kind {
        FileKind::Zip => {
            let zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
            Ok(zip
                .decompressed_size()
                .map(|s| s.min(u128::from(u64::MAX)) as u64))
        }
        FileKind::SevenZip => {
            let reader =
                sevenz_rust2::ArchiveReader::new(open(archive)?, sevenz_rust2::Password::empty())
                    .map_err(seven_error)?;
            Ok(Some(reader.archive().files.iter().map(|f| f.size).sum()))
        }
        _ => Ok(None),
    }
}

/// Unpacks `archive` into `dest` (created if needed). `progress` gets (done, total) in bytes:
/// unpacked bytes for zip and 7z, archive bytes read for tars.
pub fn unpack(
    kind: FileKind,
    archive: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Unpacked> {
    fs::create_dir_all(dest).map_err(|e| io_error(dest, e))?;
    match kind {
        FileKind::Zip => unzip(archive, dest, cancel, progress),
        FileKind::SevenZip => un7z(archive, dest, cancel, progress),
        FileKind::Tar | FileKind::TarGz | FileKind::TarBz2 | FileKind::TarXz => {
            let total = archive.metadata().map(|m| m.len()).unwrap_or(0);
            let counted = Counted::new(open(archive)?);
            let read = counted.count.clone();
            let mut report = |_: u64| progress(read.get(), total);
            match kind {
                FileKind::Tar => untar(counted, dest, cancel, &mut report),
                FileKind::TarGz => untar(
                    flate2::read::GzDecoder::new(counted),
                    dest,
                    cancel,
                    &mut report,
                ),
                FileKind::TarBz2 => untar(
                    bzip2::read::BzDecoder::new(counted),
                    dest,
                    cancel,
                    &mut report,
                ),
                _ => untar(
                    lzma_rust2::XzReader::new(counted, true),
                    dest,
                    cancel,
                    &mut report,
                ),
            }
        }
        _ => Err(Error::Invalid("unsupported_archive")),
    }
}

/// Where an archive entry goes under `dest`, or `None` when it would escape it. Separators of
/// both kinds split the name; each part is made safe for Windows.
pub fn entry_path(dest: &Path, name: &str) -> Option<PathBuf> {
    let mut out = dest.to_path_buf();
    let mut depth = 0;
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            // A drive letter, an alternate data stream or a NUL byte.
            p if p.contains(':') || p.contains('\0') => return None,
            p => {
                out.push(safe_name(p, "_"));
                depth += 1;
            }
        }
    }
    (depth > 0).then_some(out)
}

/// Whether a link at `link` pointing to `target` stays inside `dest`.
#[cfg_attr(not(unix), allow(dead_code))]
fn link_inside(dest: &Path, link: &Path, target: &str) -> bool {
    let target = Path::new(target);
    // `/etc/passwd` has a root but no drive, so Windows does not call it absolute.
    if target.is_absolute()
        || target.has_root()
        || target
            .components()
            .any(|c| matches!(c, Component::Prefix(_)))
    {
        return false;
    }
    let mut resolved = link.parent().unwrap_or(dest).to_path_buf();
    for c in target.components() {
        match c {
            Component::ParentDir => {
                if !resolved.pop() {
                    return false;
                }
            }
            Component::Normal(part) => resolved.push(part),
            _ => {}
        }
    }
    resolved.starts_with(dest)
}

fn unzip(
    archive: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Unpacked> {
    let mut zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
    let total = zip
        .decompressed_size()
        .map_or(0, |s| s.min(u128::from(u64::MAX)) as u64);
    let mut done = Unpacked::default();
    for i in 0..zip.len() {
        check(cancel)?;
        let mut entry = zip.by_index(i).map_err(zip_error)?;
        if entry.encrypted() {
            return Err(Error::Invalid("unsupported_archive"));
        }
        let Some(path) = entry_path(dest, entry.name()) else {
            return Err(Error::Invalid("archive_unsafe"));
        };
        if entry.is_dir() {
            fs::create_dir_all(&path).map_err(|e| io_error(&path, e))?;
            continue;
        }
        if entry.is_symlink() {
            let mut target = String::new();
            entry
                .read_to_string(&mut target)
                .map_err(|e| io_error(&path, e))?;
            make_link(dest, &path, &target)?;
            continue;
        }
        let size = entry.size();
        let before = done.bytes;
        let written = write_file(&mut entry, &path, size, cancel, &mut |n| {
            progress(before + n, total)
        })?;
        done.bytes += written;
        done.files += 1;
        set_mode(&path, entry.unix_mode());
    }
    progress(done.bytes, total.max(done.bytes));
    Ok(done)
}

fn un7z(
    archive: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Unpacked> {
    let mut reader =
        sevenz_rust2::ArchiveReader::new(open(archive)?, sevenz_rust2::Password::empty())
            .map_err(seven_error)?;
    let total: u64 = reader.archive().files.iter().map(|f| f.size).sum();
    let mut done = Unpacked::default();
    let mut failure: Option<Error> = None;
    let result = reader.for_each_entries(|entry, data| {
        match seven_entry(entry, data, dest, total, &mut done, cancel, progress) {
            Ok(()) => Ok(true),
            Err(e) => {
                // Stop here; the error is reported after the reader returns.
                failure = Some(e);
                Ok(false)
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    result.map_err(seven_error)?;
    progress(done.bytes, total.max(done.bytes));
    Ok(done)
}

fn seven_entry(
    entry: &sevenz_rust2::ArchiveEntry,
    data: &mut dyn Read,
    dest: &Path,
    total: u64,
    done: &mut Unpacked,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    check(cancel)?;
    let path = entry_path(dest, entry.name()).ok_or(Error::Invalid("archive_unsafe"))?;
    if entry.is_directory() {
        return fs::create_dir_all(&path).map_err(|e| io_error(&path, e));
    }
    let before = done.bytes;
    let written = write_file(data, &path, entry.size(), cancel, &mut |n| {
        progress(before + n, total)
    })?;
    done.bytes += written;
    done.files += 1;
    Ok(())
}

fn untar(
    reader: impl Read,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<Unpacked> {
    let mut tar = tar::Archive::new(reader);
    let mut done = Unpacked::default();
    for entry in tar.entries().map_err(tar_error)? {
        check(cancel)?;
        let mut entry = entry.map_err(tar_error)?;
        let name = entry
            .path()
            .map_err(tar_error)?
            .to_string_lossy()
            .into_owned();
        let Some(path) = entry_path(dest, &name) else {
            return Err(Error::Invalid("archive_unsafe"));
        };
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&path).map_err(|e| io_error(&path, e))?;
        } else if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(tar_error)?
                .map(|t| t.to_string_lossy().into_owned())
                .unwrap_or_default();
            make_link(dest, &path, &target)?;
        } else if kind.is_file() || kind == tar::EntryType::Continuous {
            let size = entry.header().size().map_err(tar_error)?;
            let mode = entry.header().mode().ok();
            done.bytes += write_file(&mut entry, &path, size, cancel, &mut |_| progress(0))?;
            done.files += 1;
            set_mode(&path, mode);
        }
        // Hard links, devices and FIFOs are skipped: games do not need them.
        progress(0);
    }
    Ok(done)
}

/// Copies one entry to `path`, refusing to write more than the `size` its header declared.
fn write_file(
    data: &mut dyn Read,
    path: &Path,
    size: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    let mut out = File::create(path).map_err(|e| io_error(path, e))?;
    let mut buf = vec![0u8; BUFFER.min(size.max(1) as usize)];
    let mut written = 0u64;
    loop {
        check(cancel)?;
        let n = data.read(&mut buf).map_err(|e| read_error(path, e))?;
        if n == 0 {
            break;
        }
        written += n as u64;
        if written > size {
            return Err(Error::Invalid("archive_corrupt"));
        }
        out.write_all(&buf[..n]).map_err(|e| io_error(path, e))?;
        progress(written);
    }
    Ok(written)
}

#[cfg(unix)]
fn make_link(dest: &Path, link: &Path, target: &str) -> Result<()> {
    if target.is_empty() || !link_inside(dest, link, target) {
        // Skipped rather than failing the whole install.
        return Ok(());
    }
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    let _ = fs::remove_file(link);
    std::os::unix::fs::symlink(target, link).map_err(|e| io_error(link, e))
}

#[cfg(not(unix))]
fn make_link(_dest: &Path, _link: &Path, _target: &str) -> Result<()> {
    // Links in archives are made for Linux and macOS builds; Windows needs admin rights for them.
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        // Keep the execute bits (Linux games need them); the owner can always read and write.
        let mode = (mode & 0o777) | 0o600;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) {}

/// Counts the bytes read from the archive file, for progress through compressed tars.
struct Counted<R> {
    inner: R,
    count: CountCell,
}

#[derive(Clone, Default)]
struct CountCell(std::rc::Rc<std::cell::Cell<u64>>);

impl CountCell {
    fn get(&self) -> u64 {
        self.0.get()
    }
}

impl<R> Counted<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            count: CountCell::default(),
        }
    }
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.0.set(self.count.0.get() + n as u64);
        Ok(n)
    }
}

fn open(path: &Path) -> Result<BufReader<File>> {
    Ok(BufReader::new(
        File::open(path).map_err(|e| io_error(path, e))?,
    ))
}

fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn io_error(path: &Path, e: io::Error) -> Error {
    if is_disk_full(&e) {
        Error::Invalid("disk_space")
    } else {
        Error::Other(format!("{}: {e}", path.display()))
    }
}

/// A failed read from the archive means it is damaged.
fn read_error(path: &Path, e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
            Error::Invalid("archive_corrupt")
        }
        _ => io_error(path, e),
    }
}

fn is_disk_full(e: &io::Error) -> bool {
    // ENOSPC on Unix, ERROR_DISK_FULL (112) and ERROR_HANDLE_DISK_FULL (39) on Windows.
    matches!(e.kind(), io::ErrorKind::StorageFull)
        || (cfg!(unix) && e.raw_os_error() == Some(28))
        || (cfg!(windows) && matches!(e.raw_os_error(), Some(112 | 39)))
}

fn zip_error(e: zip::result::ZipError) -> Error {
    use zip::result::ZipError;
    match e {
        ZipError::Io(e) => read_error(Path::new("zip"), e),
        ZipError::UnsupportedArchive(_)
        | ZipError::CompressionMethodNotSupported(_)
        | ZipError::InvalidPassword => Error::Invalid("unsupported_archive"),
        ZipError::InvalidArchive(_) | ZipError::FileNotFound => Error::Invalid("archive_corrupt"),
        other => Error::Other(format!("zip: {other}")),
    }
}

fn seven_error(e: sevenz_rust2::Error) -> Error {
    use sevenz_rust2::Error as E;
    match e {
        E::Io(e, _) => read_error(Path::new("7z"), e),
        E::PasswordRequired
        | E::UnsupportedCompressionMethod(_)
        | E::ExternalUnsupported
        | E::Unsupported(_)
        | E::UnsupportedVersion { .. }
        | E::MaxMemLimited { .. } => Error::Invalid("unsupported_archive"),
        other => {
            let text = other.to_string();
            if text.to_lowercase().contains("checksum") || text.to_lowercase().contains("crc") {
                Error::Invalid("archive_corrupt")
            } else {
                Error::Other(format!("7z: {text}"))
            }
        }
    }
}

fn tar_error(e: io::Error) -> Error {
    read_error(Path::new("tar"), e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gamelib-archive-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entry_paths_stay_inside() {
        let dest = Path::new("/games/x");
        assert_eq!(
            entry_path(dest, "bin/game.exe"),
            Some(PathBuf::from("/games/x/bin/game.exe"))
        );
        assert_eq!(
            entry_path(dest, "bin\\data\\a.pak"),
            Some(PathBuf::from("/games/x/bin/data/a.pak"))
        );
        assert_eq!(
            entry_path(dest, "/etc/passwd"),
            Some(PathBuf::from("/games/x/etc/passwd"))
        );
        assert_eq!(
            entry_path(dest, "./a/./b"),
            Some(PathBuf::from("/games/x/a/b"))
        );
        assert_eq!(entry_path(dest, "../evil"), None);
        assert_eq!(entry_path(dest, "a/../../evil"), None);
        assert_eq!(entry_path(dest, "C:\\Windows\\evil.dll"), None);
        assert_eq!(entry_path(dest, "game.exe:Zone.Identifier"), None);
        assert_eq!(
            entry_path(dest, "CON.txt"),
            Some(PathBuf::from("/games/x/_CON.txt"))
        );
        assert_eq!(entry_path(dest, ""), None);
        assert_eq!(entry_path(dest, "./"), None);
    }

    #[test]
    fn links_must_stay_inside() {
        let dest = Path::new("/games/x");
        assert!(link_inside(
            dest,
            Path::new("/games/x/lib/libfoo.so"),
            "libfoo.so.1"
        ));
        assert!(link_inside(dest, Path::new("/games/x/lib/a"), "../bin/b"));
        assert!(!link_inside(
            dest,
            Path::new("/games/x/lib/a"),
            "../../../etc/passwd"
        ));
        assert!(!link_inside(dest, Path::new("/games/x/a"), "/etc/passwd"));
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut out);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o755);
            for (name, data) in entries {
                if name.ends_with('/') {
                    w.add_directory(*name, opts).unwrap();
                } else {
                    w.start_file(*name, opts).unwrap();
                    w.write_all(data).unwrap();
                }
            }
            w.finish().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn unzips_with_progress() {
        let dir = temp_dir("zip");
        let archive = dir.join("game.zip");
        fs::write(
            &archive,
            zip_bytes(&[
                ("Game/", b""),
                ("Game/game.exe", b"MZ program"),
                ("Game/data/level1.pak", &[7u8; 5000]),
            ]),
        )
        .unwrap();
        assert_eq!(unpacked_size(FileKind::Zip, &archive).unwrap(), Some(5010));
        let dest = dir.join("out");
        let mut seen = Vec::new();
        let done = unpack(
            FileKind::Zip,
            &archive,
            &dest,
            &AtomicBool::new(false),
            &mut |d, t| seen.push((d, t)),
        )
        .unwrap();
        assert_eq!(
            done,
            Unpacked {
                files: 2,
                bytes: 5010
            }
        );
        assert_eq!(fs::read(dest.join("Game/game.exe")).unwrap(), b"MZ program");
        assert_eq!(
            fs::read(dest.join("Game/data/level1.pak")).unwrap().len(),
            5000
        );
        assert_eq!(seen.last(), Some(&(5010, 5010)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dest.join("Game/game.exe"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "execute bits kept");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zip_slip_is_refused() {
        let dir = temp_dir("slip");
        let archive = dir.join("evil.zip");
        fs::write(
            &archive,
            zip_bytes(&[("ok.txt", b"fine"), ("../escaped.txt", b"bad")]),
        )
        .unwrap();
        let dest = dir.join("out");
        let result = unpack(
            FileKind::Zip,
            &archive,
            &dest,
            &AtomicBool::new(false),
            &mut |_, _| {},
        );
        assert!(
            matches!(result, Err(Error::Invalid("archive_unsafe"))),
            "{result:?}"
        );
        assert!(!dir.join("escaped.txt").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn untars_gzip() {
        let dir = temp_dir("tgz");
        let archive = dir.join("game.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(11);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "game/start.sh", &b"#!/bin/sh\n\n"[..])
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let dest = dir.join("out");
        let mut last = (0, 0);
        let done = unpack(
            FileKind::TarGz,
            &archive,
            &dest,
            &AtomicBool::new(false),
            &mut |d, t| last = (d, t),
        )
        .unwrap();
        assert_eq!(done.files, 1);
        assert_eq!(
            fs::read(dest.join("game/start.sh")).unwrap(),
            b"#!/bin/sh\n\n"
        );
        assert!(last.1 > 0 && last.0 <= last.1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelling_stops_unpacking() {
        let dir = temp_dir("cancel");
        let archive = dir.join("game.zip");
        fs::write(&archive, zip_bytes(&[("a.bin", &[1u8; 100])])).unwrap();
        let result = unpack(
            FileKind::Zip,
            &archive,
            &dir.join("out"),
            &AtomicBool::new(true),
            &mut |_, _| {},
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rar_is_not_unpacked() {
        let result = unpack(
            FileKind::Rar,
            Path::new("x.rar"),
            &temp_dir("rar"),
            &AtomicBool::new(false),
            &mut |_, _| {},
        );
        assert!(matches!(result, Err(Error::Invalid("unsupported_archive"))));
    }
}
