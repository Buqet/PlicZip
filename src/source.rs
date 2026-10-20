//! Источник байтов архива: один файл или набор томов.
//!
//! Читатель обращается к архиву только через [`Source`] и только позиционно
//! (`read_at`), поэтому многотомный архив — это просто ещё одна реализация:
//! логическое смещение переводится в «том + смещение внутри тома».
//!
//! Тома — обычная нарезка потока байт: `файл.plz.001`, `.002`, … Читателю не
//! нужно знать размер тома, он склеивает их по порядку и складывает длины.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Произвольный доступ к байтам архива.
pub trait Source {
    /// Полная длина архива в байтах.
    fn len(&self) -> u64;

    /// Всегда ли архив пуст (нужно для clippy и общего интерфейса).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Читает до `buf.len()` байт по смещению.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;

    /// Читает ровно `buf.len()` байт или сообщает об обрезке.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let mut done = 0usize;
        while done < buf.len() {
            let read = self.read_at(&mut buf[done..], offset + done as u64)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "архив обрезан",
                ));
            }
            done += read;
        }
        Ok(())
    }
}

/// Один файл.
pub struct FileSource {
    file: File,
    len: u64,
}

impl FileSource {
    /// Открывает файл и запоминает его длину.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(crate::safety::native_path(path)).with_context(|| format!("не удалось открыть {:?}", path))?;
        let len = file
            .metadata()
            .with_context(|| format!("не удалось прочитать размер {:?}", path))?
            .len();
        Ok(FileSource { file, len })
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let room = (self.len - offset).min(buf.len() as u64) as usize;
        read_at(&self.file, &mut buf[..room], offset)
    }
}

/// Несколько томов, читаемых как один непрерывный поток.
pub struct VolumeSource {
    files: Vec<File>,
    /// Смещение начала каждого тома в логическом потоке.
    starts: Vec<u64>,
    total: u64,
    names: Vec<PathBuf>,
}

impl VolumeSource {
    /// Собирает тома по префиксу: `base.001`, `base.002`, …
    pub fn open(base: &Path) -> Result<Self> {
        let mut files = Vec::new();
        let mut names = Vec::new();
        let mut starts = Vec::new();
        let mut total = 0u64;

        for number in 1..=9999u32 {
            let path = PathBuf::from(format!("{}.{:03}", base.display(), number));
            if !path.exists() {
                break;
            }
            let file =
                File::open(crate::safety::native_path(&path)).with_context(|| format!("не удалось открыть {:?}", path))?;
            let len = file.metadata()?.len();
            starts.push(total);
            total += len;
            files.push(file);
            names.push(path);
        }

        if files.is_empty() {
            bail!("тома {:?}.001 не найдены", base);
        }

        Ok(VolumeSource {
            files,
            starts,
            total,
            names,
        })
    }

    /// Имена томов в порядке чтения.
    pub fn names(&self) -> &[PathBuf] {
        &self.names
    }

    fn locate(&self, offset: u64) -> Option<usize> {
        match self.starts.binary_search(&offset) {
            Ok(index) => Some(index),
            Err(0) => None,
            Err(index) => Some(index - 1),
        }
    }
}

impl Source for VolumeSource {
    fn len(&self) -> u64 {
        self.total
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.total {
            return Ok(0);
        }
        let index = match self.locate(offset) {
            Some(index) => index,
            None => return Ok(0),
        };
        // Сколько байт этого тома ещё осталось после offset.
        let in_volume = offset - self.starts[index];
        let volume_len = self.files[index].metadata()?.len();
        let room = (volume_len - in_volume).min(buf.len() as u64) as usize;
        read_at(&self.files[index], &mut buf[..room], in_volume)
    }
}

/// Позиционное чтение без изменения курсора файла.
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(buf, offset)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, buf, offset);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "позиционное чтение не поддерживается на этой платформе",
        ))
    }
}

/// Открывает архив: либо набор томов, либо один файл.
///
/// Возвращает источник и список открытых файлов (для вывода и проверок).
pub fn open(path: &Path) -> Result<(Box<dyn Source>, Vec<PathBuf>)> {
    // Пользователь мог указать прямо том: `backup.plz.001`.
    let base = strip_volume_suffix(path);

    let first_volume = PathBuf::from(format!("{}.001", base.display()));
    if first_volume.exists() {
        let volumes = VolumeSource::open(&base)?;
        let names = volumes.names().to_vec();
        return Ok((Box::new(volumes), names));
    }

    if base != path {
        bail!(
            "том {:?} не найден: нужны все части от .001 подряд",
            first_volume
        );
    }

    let source = FileSource::open(path)?;
    Ok((Box::new(source), vec![path.to_path_buf()]))
}

/// Отрезает номер тома `.001`, если он есть в конце пути.
fn strip_volume_suffix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(dot) = text.rfind('.') {
        let digits = &text[dot + 1..];
        if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) {
            return PathBuf::from(&text[..dot]);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn workdir(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("src-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn single_file_reads_by_offset() {
        let dir = workdir("single");
        let path = dir.join("a.bin");
        fs::write(&path, b"0123456789").unwrap();
        let source = FileSource::open(&path).unwrap();
        assert_eq!(source.len(), 10);
        let mut buf = [0u8; 4];
        assert_eq!(source.read_at(&mut buf, 2).unwrap(), 4);
        assert_eq!(&buf, b"2345");
        assert_eq!(source.read_at(&mut buf, 20).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volumes_are_concatenated() {
        let dir = workdir("volumes");
        let base = dir.join("a.plz");
        fs::write(PathBuf::from(format!("{}.001", base.display())), b"01234").unwrap();
        fs::write(PathBuf::from(format!("{}.002", base.display())), b"567").unwrap();
        fs::write(PathBuf::from(format!("{}.003", base.display())), b"89").unwrap();

        let source = VolumeSource::open(&base).unwrap();
        assert_eq!(source.len(), 10);
        let mut buf = [0u8; 10];
        source.read_exact_at(&mut buf, 0).unwrap();
        assert_eq!(&buf, b"0123456789");

        // Чтение через границу томов.
        let mut small = [0u8; 4];
        assert_eq!(source.read_at(&mut small, 3).unwrap(), 2, "граница тома");
        assert_eq!(&small[..2], b"34");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_volume_suffix() {
        assert_eq!(
            strip_volume_suffix(Path::new("C:/a/backup.plz.001")),
            PathBuf::from("C:/a/backup.plz")
        );
        assert_eq!(
            strip_volume_suffix(Path::new("C:/a/backup.plz")),
            PathBuf::from("C:/a/backup.plz")
        );
        assert_eq!(
            strip_volume_suffix(Path::new("C:/a/2024")),
            PathBuf::from("C:/a/2024")
        );
    }
}
