//! Read-only support for external archive formats.
//!
//! Archive creation remains exclusively in `.plz`. This module only detects,
//! lists, verifies and extracts ZIP, 7z, RAR and TAR files.

use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::safety::{join_safe, validate_rel_path};

const SEVEN_Z_SIGNATURE: &[u8] = b"7z\xbc\xaf\x27\x1c";
const RAR_SIGNATURE: &[u8] = b"Rar!\x1a\x07";
const ZIP_SIGNATURES: &[&[u8]] = &[b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"];

/// Supported external archive kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// ZIP archive.
    Zip,
    /// 7z archive.
    SevenZip,
    /// RAR 1.3 through RAR 5 archive.
    Rar,
    /// Uncompressed POSIX tar archive.
    Tar,
}

impl Kind {
    /// Human-readable format name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::SevenZip => "7z",
            Self::Rar => "RAR",
            Self::Tar => "TAR",
        }
    }
}

/// An external archive member.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Safe, normalized relative path.
    pub path: String,
    /// Unpacked size in bytes.
    pub size: u64,
    /// Whether the member is a directory.
    pub is_dir: bool,
}

/// Extraction summary for an external archive.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Number of extracted files.
    pub files: usize,
    /// Number of created directories.
    pub dirs: usize,
    /// Number of unpacked bytes.
    pub bytes: u64,
    /// Number of existing entries skipped by request.
    pub skipped: usize,
}

/// Detect an external archive by its signature.
///
/// `None` means that the file is not a supported external format. `.plz` is
/// deliberately not returned here and continues through the native backend.
pub fn detect(path: &Path) -> Result<Option<Kind>> {
    let mut file = File::open(path).with_context(|| format!("не удалось открыть {:?}", path))?;
    let mut header = [0u8; 512];
    let count = file.read(&mut header)?;
    let header = &header[..count];
    if header.len() >= SEVEN_Z_SIGNATURE.len() && header.starts_with(SEVEN_Z_SIGNATURE) {
        return Ok(Some(Kind::SevenZip));
    }
    if header.len() >= RAR_SIGNATURE.len() && header.starts_with(RAR_SIGNATURE) {
        return Ok(Some(Kind::Rar));
    }
    if ZIP_SIGNATURES
        .iter()
        .any(|signature| header.len() >= signature.len() && header.starts_with(signature))
    {
        return Ok(Some(Kind::Zip));
    }
    if header.len() >= 262 && &header[257..262] == b"ustar" {
        return Ok(Some(Kind::Tar));
    }
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tar"))
    {
        return Ok(Some(Kind::Tar));
    }
    Ok(None)
}

/// List entries in a supported external archive.
pub fn list(path: &Path, password: Option<&[u8]>) -> Result<(Kind, Vec<Entry>)> {
    let kind = detect(path)?.context("формат архива не поддерживается")?;
    let entries = match kind {
        Kind::Zip => list_zip(path, password)?,
        Kind::SevenZip => list_7z(path, password)?,
        Kind::Rar => list_rar(path, password)?,
        Kind::Tar => list_tar(path)?,
    };
    Ok((kind, entries))
}

/// Extract an external archive using the same safe path policy as `.plz`.
pub fn extract(
    path: &Path,
    output: &Path,
    password: Option<&[u8]>,
    skip_existing: bool,
) -> Result<(Kind, Stats)> {
    let kind = detect(path)?.context("формат архива не поддерживается")?;
    let stats = match kind {
        Kind::Zip => extract_zip(path, output, password, skip_existing)?,
        Kind::SevenZip => extract_7z(path, output, password, skip_existing)?,
        Kind::Rar => extract_rar(path, output, password, skip_existing)?,
        Kind::Tar => extract_tar(path, output, skip_existing)?,
    };
    Ok((kind, stats))
}

/// Verify every payload in an external archive without writing files.
pub fn test(path: &Path, password: Option<&[u8]>) -> Result<(Kind, usize)> {
    let kind = detect(path)?.context("формат архива не поддерживается")?;
    let count = match kind {
        Kind::Zip => test_zip(path, password)?,
        Kind::SevenZip => test_7z(path, password)?,
        Kind::Rar => test_rar(path, password)?,
        Kind::Tar => test_tar(path)?,
    };
    Ok((kind, count))
}

fn normalized_path(raw: &str) -> Result<String> {
    let normalized = raw.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty() {
        bail!("пустой путь элемента внешнего архива");
    }
    validate_rel_path(normalized)?;
    Ok(normalized.to_owned())
}

fn is_root_entry(raw: &str) -> bool {
    let normalized = raw.replace('\\', "/");
    normalized.trim_matches('/').is_empty()
}

fn password_7z(password: Option<&[u8]>) -> sevenz_rust::Password {
    match password {
        Some(bytes) if !bytes.is_empty() => {
            let text = String::from_utf8_lossy(bytes);
            sevenz_rust::Password::from(text.as_ref())
        }
        _ => sevenz_rust::Password::empty(),
    }
}

fn zip_options(password: Option<&[u8]>) -> zip::read::ZipReadOptions<'_> {
    zip::read::ZipReadOptions::new().password(password)
}

fn list_zip(path: &Path, password: Option<&[u8]>) -> Result<Vec<Entry>> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть ZIP-архив {:?}", path))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось прочитать ZIP-архив {:?}", path))?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index_with_options(index, zip_options(password))
            .map_err(|err| anyhow::anyhow!(err))
            .with_context(|| format!("не удалось прочитать элемент ZIP #{index}"))?;
        let raw_name = String::from_utf8_lossy(entry.name_raw()).into_owned();
        if is_root_entry(&raw_name) {
            continue;
        }
        entries.push(Entry {
            path: normalized_path(&raw_name)?,
            size: entry.size(),
            is_dir: entry.is_dir(),
        });
    }
    Ok(entries)
}

fn extract_zip(
    path: &Path,
    output: &Path,
    password: Option<&[u8]>,
    skip_existing: bool,
) -> Result<Stats> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть ZIP-архив {:?}", path))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось прочитать ZIP-архив {:?}", path))?;
    let mut stats = Stats::default();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index_with_options(index, zip_options(password))
            .map_err(|err| anyhow::anyhow!(err))
            .with_context(|| format!("не удалось открыть элемент ZIP #{index}"))?;
        let raw_name = String::from_utf8_lossy(entry.name_raw()).into_owned();
        if is_root_entry(&raw_name) {
            continue;
        }
        let rel = normalized_path(&raw_name)?;
        let target = join_safe(output, &rel)?;
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            stats.dirs += 1;
            continue;
        }
        if skip_existing && target.exists() {
            stats.skipped += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output_file = File::create(&target)?;
        let copied = io::copy(&mut entry, &mut output_file)?;
        output_file.sync_all()?;
        stats.files += 1;
        stats.bytes = stats.bytes.saturating_add(copied);
    }
    Ok(stats)
}

fn test_zip(path: &Path, password: Option<&[u8]>) -> Result<usize> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть ZIP-архив {:?}", path))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось прочитать ZIP-архив {:?}", path))?;
    let mut count = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index_with_options(index, zip_options(password))
            .map_err(|err| anyhow::anyhow!(err))
            .with_context(|| format!("не удалось проверить элемент ZIP #{index}"))?;
        if is_root_entry(&String::from_utf8_lossy(entry.name_raw())) || entry.is_dir() {
            continue;
        }
        io::copy(&mut entry, &mut io::sink())?;
        count += 1;
    }
    Ok(count)
}

fn list_7z(path: &Path, password: Option<&[u8]>) -> Result<Vec<Entry>> {
    let reader = sevenz_rust::SevenZReader::open(path, password_7z(password))
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось прочитать 7z-архив {:?}", path))?;
    reader
        .archive()
        .files
        .iter()
        .filter(|entry| !entry.is_anti_item && !is_root_entry(entry.name()))
        .map(|entry| {
            Ok(Entry {
                path: normalized_path(entry.name())?,
                size: entry.size,
                is_dir: entry.is_directory(),
            })
        })
        .collect()
}

fn extract_7z(
    path: &Path,
    output: &Path,
    password: Option<&[u8]>,
    skip_existing: bool,
) -> Result<Stats> {
    let mut reader = sevenz_rust::SevenZReader::open(path, password_7z(password))
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось открыть 7z-архив {:?}", path))?;
    let mut stats = Stats::default();
    reader
        .for_each_entries(|entry, data| {
            if is_root_entry(entry.name()) {
                return Ok(true);
            }
            let rel = normalized_path(entry.name())
                .map_err(|err| sevenz_rust::Error::other(err.to_string()))?;
            let target = join_safe(output, &rel)
                .map_err(|err| sevenz_rust::Error::other(err.to_string()))?;
            if entry.is_directory() {
                fs::create_dir_all(&target).map_err(sevenz_rust::Error::io)?;
                stats.dirs += 1;
                return Ok(true);
            }
            if skip_existing && target.exists() {
                stats.skipped += 1;
                return Ok(true);
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(sevenz_rust::Error::io)?;
            }
            let mut file = File::create(&target).map_err(sevenz_rust::Error::io)?;
            let copied = io::copy(data, &mut file).map_err(sevenz_rust::Error::io)?;
            file.sync_all().map_err(sevenz_rust::Error::io)?;
            stats.files += 1;
            stats.bytes = stats.bytes.saturating_add(copied);
            Ok(true)
        })
        .map_err(|err| anyhow::anyhow!(err))
        .context("ошибка распаковки 7z")?;
    Ok(stats)
}

fn open_rar(path: &Path, password: Option<&[u8]>) -> Result<rars::Archive> {
    let options = rars::ArchiveReadOptions::with_optional_password(password);
    rars::ArchiveReader::read_path_with_options(path, options)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("не удалось прочитать RAR-архив {:?}", path))
}

fn list_rar(path: &Path, password: Option<&[u8]>) -> Result<Vec<Entry>> {
    let archive = open_rar(path, password)?;
    archive
        .members()
        .filter(|member| !member.meta.is_redirection)
        .map(|member| {
            let owned = member.to_owned();
            Ok(Entry {
                path: normalized_path(&owned.meta.name_lossy())?,
                size: owned.meta.unpacked_size,
                is_dir: owned.meta.is_directory,
            })
        })
        .collect()
}

fn extract_rar(
    path: &Path,
    output: &Path,
    password: Option<&[u8]>,
    skip_existing: bool,
) -> Result<Stats> {
    let archive = open_rar(path, password)?;
    let bytes = Arc::new(AtomicU64::new(0));
    let bytes_for_writer = Arc::clone(&bytes);
    let mut stats = Stats::default();
    archive
        .extract_to_with_options(
            rars::ArchiveReadOptions::with_optional_password(password),
            |meta| {
                let rel = normalized_path(&String::from_utf8_lossy(&meta.name))
                    .map_err(|err| io::Error::other(err.to_string()))?;
                let target =
                    join_safe(output, &rel).map_err(|err| io::Error::other(err.to_string()))?;
                if meta.is_directory {
                    fs::create_dir_all(&target)?;
                    stats.dirs += 1;
                    return Ok(Box::new(io::sink()) as Box<dyn Write>);
                }
                if skip_existing && target.exists() {
                    stats.skipped += 1;
                    return Ok(Box::new(io::sink()) as Box<dyn Write>);
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let file = OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&target)?;
                stats.files += 1;
                Ok(Box::new(CountingWriter {
                    file,
                    bytes: Arc::clone(&bytes_for_writer),
                }) as Box<dyn Write>)
            },
        )
        .map_err(|err| anyhow::anyhow!(err))
        .context("ошибка распаковки RAR")?;
    stats.bytes = bytes.load(Ordering::Relaxed);
    Ok(stats)
}

fn test_7z(path: &Path, password: Option<&[u8]>) -> Result<usize> {
    let mut reader = sevenz_rust::SevenZReader::open(path, password_7z(password))
        .map_err(|err| anyhow::anyhow!(err))?;
    let mut count = 0;
    reader
        .for_each_entries(|entry, data| {
            if is_root_entry(entry.name()) {
                return Ok(true);
            }
            if !entry.is_directory() {
                io::copy(data, &mut io::sink()).map_err(sevenz_rust::Error::io)?;
                count += 1;
            }
            Ok(true)
        })
        .map_err(|err| anyhow::anyhow!(err))?;
    Ok(count)
}

fn test_rar(path: &Path, password: Option<&[u8]>) -> Result<usize> {
    let archive = open_rar(path, password)?;
    let count = archive
        .members()
        .filter(|member| !member.meta.is_directory && !member.meta.is_redirection)
        .count();
    archive
        .test(password)
        .map_err(|err| anyhow::anyhow!(err))
        .context("проверка RAR не пройдена")?;
    Ok(count)
}

fn list_tar(path: &Path) -> Result<Vec<Entry>> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть TAR-архив {:?}", path))?;
    let mut archive = tar::Archive::new(file);
    let mut entries = Vec::new();
    for item in archive
        .entries()
        .context("не удалось прочитать TAR-архив")?
    {
        let entry = item.context("не удалось прочитать элемент TAR")?;
        let raw = entry.path().context("некорректный путь элемента TAR")?;
        let raw = raw.to_string_lossy();
        if is_root_entry(&raw) {
            continue;
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            continue;
        }
        entries.push(Entry {
            path: normalized_path(&raw)?,
            size: entry.size(),
            is_dir: kind.is_dir(),
        });
    }
    Ok(entries)
}

fn extract_tar(path: &Path, output: &Path, skip_existing: bool) -> Result<Stats> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть TAR-архив {:?}", path))?;
    let mut archive = tar::Archive::new(file);
    let mut stats = Stats::default();
    for item in archive
        .entries()
        .context("не удалось прочитать TAR-архив")?
    {
        let mut entry = item.context("не удалось прочитать элемент TAR")?;
        let raw = entry.path().context("некорректный путь элемента TAR")?;
        let raw = raw.to_string_lossy();
        if is_root_entry(&raw) {
            continue;
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            continue;
        }
        let rel = normalized_path(&raw)?;
        let target = join_safe(output, &rel)?;
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
            stats.dirs += 1;
            continue;
        }
        if skip_existing && target.exists() {
            stats.skipped += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&target)?;
        let copied = io::copy(&mut entry, &mut out)?;
        out.sync_all()?;
        stats.files += 1;
        stats.bytes = stats.bytes.saturating_add(copied);
    }
    Ok(stats)
}

fn test_tar(path: &Path) -> Result<usize> {
    let file =
        File::open(path).with_context(|| format!("не удалось открыть TAR-архив {:?}", path))?;
    let mut archive = tar::Archive::new(file);
    let mut count = 0;
    for item in archive
        .entries()
        .context("не удалось прочитать TAR-архив")?
    {
        let mut entry = item.context("не удалось проверить элемент TAR")?;
        if entry.header().entry_type().is_file() {
            io::copy(&mut entry, &mut io::sink())?;
            count += 1;
        }
    }
    Ok(count)
}

struct CountingWriter {
    file: File,
    bytes: Arc<AtomicU64>,
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buf)?;
        self.bytes.fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_signatures() {
        let dir = std::env::temp_dir().join(format!("pliczip-external-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let seven = dir.join("a.bin");
        File::create(&seven)
            .unwrap()
            .write_all(SEVEN_Z_SIGNATURE)
            .unwrap();
        assert_eq!(detect(&seven).unwrap(), Some(Kind::SevenZip));

        let rar = dir.join("b.bin");
        File::create(&rar)
            .unwrap()
            .write_all(b"Rar!\x1a\x07\x01\x00")
            .unwrap();
        assert_eq!(detect(&rar).unwrap(), Some(Kind::Rar));
        let zip = dir.join("c.zip");
        File::create(&zip)
            .unwrap()
            .write_all(b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00")
            .unwrap();
        assert_eq!(detect(&zip).unwrap(), Some(Kind::Zip));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(normalized_path("../outside.txt").is_err());
        assert!(normalized_path("C:/Windows/file").is_err());
        assert_eq!(
            normalized_path("folder\\file.txt").unwrap(),
            "folder/file.txt"
        );
    }

    #[test]
    fn seven_zip_roundtrip() {
        let root = std::env::temp_dir().join(format!("pliczip-7z-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/nested/data.bin"), [0u8, 1, 2, 3, 255]).unwrap();
        let archive = root.join("sample.7z");
        sevenz_rust::compress_to_path(root.join("src"), &archive).unwrap();

        let (kind, entries) = list(&archive, None).unwrap();
        assert_eq!(kind, Kind::SevenZip);
        assert!(entries.iter().any(|entry| entry.path == "nested/data.bin"));

        let output = root.join("out");
        let (_, stats) = extract(&archive, &output, None, false).unwrap();
        assert_eq!(stats.files, 1);
        assert_eq!(
            fs::read(output.join("nested/data.bin")).unwrap(),
            [0, 1, 2, 3, 255]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tar_roundtrip() {
        let root = std::env::temp_dir().join(format!("pliczip-tar-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/nested/data.txt"), b"tar data").unwrap();
        let archive = root.join("sample.tar");
        let file = File::create(&archive).unwrap();
        let mut builder = tar::Builder::new(file);
        builder.append_dir_all("", root.join("src")).unwrap();
        builder.finish().unwrap();
        assert_eq!(detect(&archive).unwrap(), Some(Kind::Tar));
        let (kind, entries) = list(&archive, None).unwrap();
        assert_eq!(kind, Kind::Tar);
        assert!(entries.iter().any(|entry| entry.path == "nested/data.txt"));
        assert_eq!(test(&archive, None).unwrap().1, 1);
        let output = root.join("out");
        let (_, stats) = extract(&archive, &output, None, false).unwrap();
        assert_eq!(stats.files, 1);
        assert_eq!(
            fs::read(output.join("nested/data.txt")).unwrap(),
            b"tar data"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zip_roundtrip() {
        let root = std::env::temp_dir().join(format!("pliczip-zip-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("sample.zip");
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        writer.add_directory("nested/", options).unwrap();
        writer.start_file("nested/data.bin", options).unwrap();
        writer.write_all(&[0, 1, 2, 3, 255]).unwrap();
        writer.finish().unwrap();

        let (kind, entries) = list(&archive, None).unwrap();
        assert_eq!(kind, Kind::Zip);
        assert!(entries.iter().any(|entry| entry.path == "nested/data.bin"));

        let output = root.join("out");
        let (_, stats) = extract(&archive, &output, None, false).unwrap();
        assert_eq!(stats.files, 1);
        assert_eq!(test(&archive, None).unwrap().1, 1);
        assert_eq!(
            fs::read(output.join("nested/data.bin")).unwrap(),
            [0, 1, 2, 3, 255]
        );
        let _ = fs::remove_dir_all(root);
    }
}
