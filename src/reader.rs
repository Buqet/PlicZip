//! Чтение архива: список, проверка целостности, распаковка.
//!
//! Распаковка идёт потоком. Блок читается прямо из источника (файл или набор
//! томов) через позиционное чтение, при необходимости расшифровывается кусками,
//! распаковывается в `Read`, и байты сразу раздаются файлам — поэтому архив с
//! файлом на 20 ГБ распакуется, не заняв 20 ГБ памяти.
//!
//! Порядок распаковки: сначала все каталоги, потом файлы, потом символические
//! ссылки. Ссылки создаются последними намеренно: иначе вредоносный архив мог
//! бы положить ссылку `docs -> /etc` и записать файлы уже за пределы каталога.

use anyhow::{ensure, Context, Result};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::compress;
use crate::crypto;
use crate::format::{self, Kind, Layout, PzEntry, PzIndex};
use crate::safety;
use crate::source::{self, Source};
use crate::util::{human_bytes, plural, Progress};

/// Размер буфера при распаковке.
const BUF: usize = 256 * 1024;

/// Параметры распаковки.
pub struct ExtractOptions {
    /// Куда распаковывать.
    pub output: PathBuf,
    /// Пропускать уже существующие файлы вместо перезаписи.
    pub skip_existing: bool,
    /// Проверять контрольные суммы при распаковке.
    pub verify: bool,
    /// Восстанавливать символические ссылки.
    pub symlinks: bool,
    /// Восстанавливать время изменения.
    pub restore_mtime: bool,
    /// Максимально допустимая степень сжатия блока (0 — без ограничения).
    pub max_ratio: u64,
    /// Показывать прогресс.
    pub show_progress: bool,
    /// Получатель обновлений прогресса (GUI); `None` — только stderr.
    pub progress: Option<std::sync::Arc<dyn crate::util::ProgressSink>>,
}

impl std::fmt::Debug for ExtractOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractOptions")
            .field("output", &self.output)
            .field("skip_existing", &self.skip_existing)
            .field("verify", &self.verify)
            .field("symlinks", &self.symlinks)
            .field("restore_mtime", &self.restore_mtime)
            .field("max_ratio", &self.max_ratio)
            .field("show_progress", &self.show_progress)
            .finish_non_exhaustive()
    }
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            output: PathBuf::from("."),
            skip_existing: false,
            verify: true,
            symlinks: true,
            restore_mtime: true,
            max_ratio: 100_000,
            show_progress: true,
            progress: None,
        }
    }
}

/// Итоги распаковки.
#[derive(Debug, Clone, Default)]
pub struct ExtractStats {
    /// Сколько файлов записано.
    pub files: usize,
    /// Сколько каталогов создано.
    pub dirs: usize,
    /// Сколько символических ссылок восстановлено.
    pub links: usize,
    /// Сколько элементов пропущено.
    pub skipped: usize,
    /// Сколько байт записано.
    pub bytes: u64,
}

/// Итоги проверки целостности.
#[derive(Debug, Clone, Default)]
pub struct TestReport {
    /// Сколько блоков проверено.
    pub blocks: usize,
    /// Сколько файлов проверено.
    pub files: usize,
    /// Сколько байт распаковано при проверке.
    pub bytes: u64,
}

/// Кусок источника, читаемый как поток.
struct SliceReader<'a> {
    src: &'a dyn Source,
    pos: u64,
    end: u64,
}

impl Read for SliceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.end.saturating_sub(self.pos);
        if remaining == 0 || buf.is_empty() {
            return Ok(0);
        }
        let want = buf.len().min(remaining as usize);
        let read = self.src.read_at(&mut buf[..want], self.pos)?;
        self.pos += read as u64;
        Ok(read)
    }
}

/// Открытый архив.
pub struct Archive {
    path: PathBuf,
    sources: Vec<PathBuf>,
    src: Box<dyn Source>,
    /// Разобранный индекс.
    pub index: PzIndex,
    /// Смещение индекса в файле.
    pub index_start: u64,
    /// Раскладка контейнера (шифрование, начало данных).
    pub layout: Layout,
    key: Option<Zeroizing<[u8; 32]>>,
}

impl Archive {
    /// Открывает архив. Если он зашифрован, пароль спрашивается интерактивно.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, None)
    }

    /// Открывает архив, беря пароль из аргумента или спрашивая его.
    pub fn open_with(path: &Path, password: Option<&str>) -> Result<Self> {
        let (src, sources) = source::open(path)?;
        let layout = format::inspect(src.as_ref())?;

        let key = if layout.encrypted {
            let password = crypto::password(password, "Пароль архива")?;
            let params = layout
                .crypto
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("в архиве нет параметров шифрования"))?;
            Some(crypto::derive_key(
                password.as_bytes(),
                &params.salt,
                &params.kdf,
            )?)
        } else {
            None
        };

        let parsed = format::read_index(src.as_ref(), &layout, key.as_ref().map(|value| &**value))?;

        Ok(Archive {
            path: path.to_path_buf(),
            sources,
            src,
            index: parsed.index,
            index_start: parsed.index_start,
            layout: parsed.layout,
            key,
        })
    }

    /// Путь, с которого начали открытие.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Зашифрован ли архив.
    pub fn is_encrypted(&self) -> bool {
        self.layout.encrypted
    }

    /// Все файлы, из которых состоит архив (для многотомного — все тома).
    pub fn files(&self) -> &[PathBuf] {
        &self.sources
    }

    /// Элементы архива в порядке записи.
    pub fn entries(&self) -> &[PzEntry] {
        &self.index.entries
    }

    /// Открывает поток сжатых (но уже расшифрованных) байт блока.
    fn block_stream(&self, bi: usize) -> Result<Box<dyn Read + '_>> {
        let block = &self.index.blocks[bi];
        let slice = SliceReader {
            src: self.src.as_ref(),
            pos: block.offset,
            end: block.offset + block.stored_size,
        };

        let payload: Box<dyn Read + '_> = match (&self.key, &self.layout.crypto) {
            (Some(key), Some(params)) => Box::new(crypto::DecryptingReader::new(
                slice,
                key,
                &params.nonce_base,
                bi as u32,
                block.payload_size,
            )),
            _ => Box::new(slice),
        };

        Ok(payload)
    }

    /// Раскладывает элементы по блокам, сохраняя порядок индекса.
    fn group_by_block(&self) -> Vec<Vec<usize>> {
        let mut per_block: Vec<Vec<usize>> = vec![Vec::new(); self.index.blocks.len()];
        for (i, entry) in self.index.entries.iter().enumerate() {
            if entry.kind == Kind::File && entry.size > 0 {
                if let Some(block) = entry.block {
                    per_block[block as usize].push(i);
                }
            }
        }
        per_block
    }

    /// Распаковывает архив целиком.
    pub fn extract(&mut self, opts: &ExtractOptions) -> Result<ExtractStats> {
        fs::create_dir_all(&opts.output)
            .with_context(|| format!("не удалось создать каталог {:?}", opts.output))?;
        let dest = opts
            .output
            .canonicalize()
            .with_context(|| format!("не удалось разрешить путь {:?}", opts.output))?;

        let mut stats = ExtractStats::default();
        let per_block = self.group_by_block();
        let total: u64 = self.index.total_size();
        let mut progress = Progress::new("распаковка", total, opts.show_progress && total > 0);
        if let Some(sink) = &opts.progress {
            progress = progress.with_sink(std::sync::Arc::clone(sink));
        }

        // Проход 1: каталоги.
        for entry in &self.index.entries {
            if entry.kind != Kind::Dir {
                continue;
            }
            let target = safety::join_safe(&dest, &entry.path)?;
            match fs::create_dir_all(&target) {
                Ok(()) => stats.dirs += 1,
                Err(err) => {
                    eprintln!("  ! каталог {}: {}", entry.path, err);
                    stats.skipped += 1;
                }
            }
        }

        // Проход 2: файлы, блок за блоком.
        for (bi, block) in self.index.blocks.iter().enumerate() {
            let indexes = &per_block[bi];
            if indexes.is_empty() {
                continue;
            }
            format::check_expansion(block.stored_size, block.size, opts.max_ratio)
                .with_context(|| format!("блок {}", bi))?;

            let payload = self.block_stream(bi)?;
            let mut stream = compress::decompress_stream(block.method, payload)?;
            let mut block_hasher = blake3::Hasher::new();
            let mut pos = 0u64;

            for &ei in indexes {
                let entry = &self.index.entries[ei];
                ensure!(
                    entry.block_offset == pos,
                    "блок {}: неожиданное смещение для {}",
                    bi,
                    entry.path
                );

                let target = target_path(&dest, entry)?;
                let mut file_hasher = blake3::Hasher::new();

                if opts.skip_existing && target.exists() {
                    let written = copy_hashed(
                        &mut *stream,
                        &mut io::sink(),
                        entry.size,
                        &mut file_hasher,
                        &mut block_hasher,
                    )?;
                    ensure!(written == entry.size, "блок {}: не хватает данных", bi);
                    stats.skipped += 1;
                    pos += entry.size;
                    progress.add(entry.size);
                    continue;
                }

                let mut file = File::create(safety::native_path(&target))
                    .with_context(|| format!("не удалось создать {:?}", target))?;
                let written = copy_hashed(
                    &mut *stream,
                    &mut file,
                    entry.size,
                    &mut file_hasher,
                    &mut block_hasher,
                )?;
                ensure!(
                    written == entry.size,
                    "файл {}: получено {} байт из {}",
                    entry.path,
                    written,
                    entry.size
                );
                file.flush()?;
                drop(file);

                if opts.verify {
                    let actual = file_hasher.finalize();
                    ensure!(
                        actual.as_bytes() == &entry.hash,
                        "контрольная сумма не совпала: {} (архив повреждён)",
                        entry.path
                    );
                }

                apply_metadata(&target, entry, opts.restore_mtime);
                stats.files += 1;
                stats.bytes += entry.size;
                pos += entry.size;
                progress.add(entry.size);
            }

            // Лишние байты в блоке означают, что индекс не соответствует данным.
            let extra = copy_hashed(
                &mut *stream,
                &mut io::sink(),
                BUF as u64,
                &mut blake3::Hasher::new(),
                &mut block_hasher,
            )?;
            ensure!(
                extra == 0,
                "блок {}: данных больше, чем заявлено в индексе",
                bi
            );

            if opts.verify {
                let actual = block_hasher.finalize();
                ensure!(
                    actual.as_bytes() == &block.hash,
                    "контрольная сумма блока {} не совпала (архив повреждён)",
                    bi
                );
            }
        }

        // Проход 3: пустые файлы (у них нет блока данных, поэтому проход по
        // блокам их не увидит).
        let empty_hash = *blake3::hash(b"").as_bytes();
        for entry in &self.index.entries {
            if entry.kind != Kind::File || entry.size > 0 {
                continue;
            }
            let target = target_path(&dest, entry)?;
            if opts.skip_existing && target.exists() {
                stats.skipped += 1;
                continue;
            }
            if opts.verify {
                ensure!(
                    entry.hash == empty_hash,
                    "контрольная сумма пустого файла {} неверна (архив повреждён)",
                    entry.path
                );
            }
            File::create(safety::native_path(&target)).with_context(|| format!("не удалось создать {:?}", target))?;
            apply_metadata(&target, entry, opts.restore_mtime);
            stats.files += 1;
        }

        // Проход 4: символические ссылки.
        for entry in &self.index.entries {
            if entry.kind != Kind::Symlink {
                continue;
            }
            if !opts.symlinks {
                stats.skipped += 1;
                continue;
            }
            if let Err(err) = safety::validate_link_target(&entry.path, &entry.link) {
                eprintln!("  ! ссылка {} пропущена: {}", entry.path, err);
                stats.skipped += 1;
                continue;
            }
            let target = safety::join_safe(&dest, &entry.path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(safety::native_path(parent))?;
            }
            let _ = fs::remove_file(safety::native_path(&target));
            match create_symlink(&entry.link, &target) {
                Ok(()) => stats.links += 1,
                Err(err) => {
                    eprintln!(
                        "  ! ссылка {} -> {} не создана: {}",
                        entry.path, entry.link, err
                    );
                    stats.skipped += 1;
                }
            }
        }

        // Проход 5: время и права каталогов (файлы внутри уже их сдвинули).
        for entry in &self.index.entries {
            if entry.kind != Kind::Dir {
                continue;
            }
            if let Ok(target) = safety::join_safe(&dest, &entry.path) {
                apply_metadata(&target, entry, opts.restore_mtime);
            }
        }

        progress.finish_with(&format!(
            "готово: {} ({}), {}, {}{}",
            plural(stats.files, "файл", "файла", "файлов"),
            human_bytes(stats.bytes),
            plural(stats.dirs, "каталог", "каталога", "каталогов"),
            plural(stats.links, "ссылка", "ссылки", "ссылок"),
            if stats.skipped > 0 {
                format!(", пропущено {}", stats.skipped)
            } else {
                String::new()
            }
        ));

        Ok(stats)
    }

    /// Проверяет целостность всех блоков, ничего не записывая на диск.
    pub fn test(
        &mut self,
        show_progress: bool,
        max_ratio: u64,
        sink: Option<std::sync::Arc<dyn crate::util::ProgressSink>>,
    ) -> Result<TestReport> {
        let per_block = self.group_by_block();
        let total: u64 = self.index.total_size();
        let mut progress = Progress::new("проверка", total, show_progress && total > 0);
        if let Some(sink) = sink {
            progress = progress.with_sink(sink);
        }
        let mut report = TestReport::default();

        for (bi, block) in self.index.blocks.iter().enumerate() {
            format::check_expansion(block.stored_size, block.size, max_ratio)
                .with_context(|| format!("блок {}", bi))?;

            let payload = self.block_stream(bi)?;
            let mut stream = compress::decompress_stream(block.method, payload)?;
            let mut block_hasher = blake3::Hasher::new();
            let mut pos = 0u64;

            for &ei in &per_block[bi] {
                let entry = &self.index.entries[ei];
                ensure!(
                    entry.block_offset == pos,
                    "блок {}: неожиданное смещение для {}",
                    bi,
                    entry.path
                );

                let mut file_hasher = blake3::Hasher::new();
                let written = copy_hashed(
                    &mut *stream,
                    &mut io::sink(),
                    entry.size,
                    &mut file_hasher,
                    &mut block_hasher,
                )?;
                ensure!(
                    written == entry.size,
                    "файл {}: не хватает данных ({} из {})",
                    entry.path,
                    written,
                    entry.size
                );
                ensure!(
                    file_hasher.finalize().as_bytes() == &entry.hash,
                    "не сошлась контрольная сумма файла {}",
                    entry.path
                );

                report.files += 1;
                report.bytes += entry.size;
                pos += entry.size;
                progress.add(entry.size);
            }

            let extra = copy_hashed(
                &mut *stream,
                &mut io::sink(),
                BUF as u64,
                &mut blake3::Hasher::new(),
                &mut block_hasher,
            )?;
            ensure!(extra == 0, "блок {}: данных больше, чем заявлено", bi);
            ensure!(
                block_hasher.finalize().as_bytes() == &block.hash,
                "не сошлась контрольная сумма блока {}",
                bi
            );

            report.blocks += 1;
        }

        // Пустые файлы и служебные записи проверяем отдельно.
        let empty_hash = *blake3::hash(b"").as_bytes();
        for entry in &self.index.entries {
            if entry.kind == Kind::File && entry.size == 0 {
                ensure!(
                    entry.hash == empty_hash,
                    "не сошлась контрольная сумма пустого файла {}",
                    entry.path
                );
            }
        }

        progress.finish_with(&format!(
            "всё в порядке: {}, {}, {} данных",
            plural(report.blocks, "блок", "блока", "блоков"),
            plural(report.files, "файл", "файла", "файлов"),
            human_bytes(report.bytes)
        ));

        Ok(report)
    }
}

/// Копирует до `limit` байт, обновляя хеши файла и блока.
///
/// Два хешера сразу нужны потому, что один блок может содержать много файлов:
/// хеш блока считается по всему блоку, хеш файла — по его куску.
fn copy_hashed(
    src: &mut dyn Read,
    dst: &mut dyn Write,
    limit: u64,
    file_hasher: &mut blake3::Hasher,
    block_hasher: &mut blake3::Hasher,
) -> Result<u64> {
    let mut buf = vec![0u8; BUF];
    let mut done = 0u64;
    while done < limit {
        let want = ((limit - done).min(BUF as u64)) as usize;
        let read = src.read(&mut buf[..want]).context("ошибка чтения блока")?;
        if read == 0 {
            break;
        }
        dst.write_all(&buf[..read]).context("ошибка записи файла")?;
        file_hasher.update(&buf[..read]);
        block_hasher.update(&buf[..read]);
        done += read as u64;
    }
    Ok(done)
}

/// Готовит путь для записи элемента.
///
/// Проверяет, что путь остаётся внутри каталога распаковки даже после
/// разыменования символических ссылок в родительских каталогах.
fn target_path(dest: &Path, entry: &PzEntry) -> Result<PathBuf> {
    let target = safety::join_safe(dest, &entry.path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(safety::native_path(parent))
            .with_context(|| format!("не удалось создать каталог для {}", entry.path))?;
        let canon = safety::native_path(parent)
            .canonicalize()
            .with_context(|| format!("не удалось разрешить каталог для {}", entry.path))?;
        ensure!(
            canon.starts_with(dest),
            "путь {} выводит за пределы каталога распаковки",
            entry.path
        );
    }
    Ok(target)
}

/// Восстанавливает время изменения и права доступа.
fn apply_metadata(target: &Path, entry: &PzEntry, restore_mtime: bool) {
    if restore_mtime && entry.mtime > 0 {
        let ft = filetime::FileTime::from_unix_time(entry.mtime, 0);
        let _ = filetime::set_file_mtime(safety::native_path(target), ft);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if entry.mode != 0 {
            let _ = fs::set_permissions(target, fs::Permissions::from_mode(entry.mode));
        }
    }
    #[cfg(not(unix))]
    {
        // На Windows из unix-прав осмысленно восстанавливается только «только чтение».
        if entry.kind == Kind::File && entry.mode != 0 && entry.mode & 0o200 == 0 {
            let native = safety::native_path(target);
            if let Ok(meta) = fs::metadata(&native) {
                let mut perms = meta.permissions();
                perms.set_readonly(true);
                let _ = fs::set_permissions(native, perms);
            }
        }
    }
}

fn create_symlink(target: &str, link_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link_path)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        // На Windows тип ссылки надо угадывать: сначала файл, потом каталог.
        match std::os::windows::fs::symlink_file(target, link_path) {
            Ok(()) => Ok(()),
            Err(first) => std::os::windows::fs::symlink_dir(target, link_path)
                .map_err(|_| anyhow::anyhow!("{}", first)),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link_path);
        anyhow::bail!("символические ссылки не поддерживаются на этой платформе")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{self, CreateOptions};
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("r-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_sample(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::write(dir.join("src/a.txt"), "привет ".repeat(500)).unwrap();
        fs::write(dir.join("src/b.txt"), "").unwrap();
        fs::write(dir.join("src/nested/c.bin"), vec![0xABu8; 200_000]).unwrap();
        dir.join("src")
    }

    fn roundtrip(solid: bool, level: i32) {
        let dir = temp_dir(if solid { "solid" } else { "plain" });
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        let opts = CreateOptions {
            level,
            solid,
            show_progress: false,
            ..Default::default()
        };
        writer::create(&archive, &[src.clone()], &opts).unwrap();

        let out = dir.join("out");
        let mut ar = Archive::open(&archive).unwrap();
        let stats = ar
            .extract(&ExtractOptions {
                output: out.clone(),
                show_progress: false,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(stats.files, 3, "файлов распаковано");
        assert_eq!(
            fs::read(out.join("src/a.txt")).unwrap(),
            fs::read(src.join("a.txt")).unwrap()
        );
        assert_eq!(
            fs::read(out.join("src/nested/c.bin")).unwrap(),
            fs::read(src.join("nested/c.bin")).unwrap()
        );
        assert_eq!(fs::read(out.join("src/b.txt")).unwrap().len(), 0);

        // Повторное извлечение поверх — не должно падать.
        let mut ar = Archive::open(&archive).unwrap();
        ar.extract(&ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .unwrap();

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_plain() {
        for level in [0, 1, 5, 9] {
            roundtrip(false, level);
        }
    }

    #[test]
    fn roundtrip_solid() {
        roundtrip(true, 6);
    }

    #[test]
    fn encrypted_roundtrip() {
        let dir = temp_dir("encrypted");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src.clone()],
            &CreateOptions {
                show_progress: false,
                password: Some(Zeroizing::new("секрет".to_string())),
                ..Default::default()
            },
        )
        .unwrap();

        // Без пароля не открывается.
        assert!(Archive::open_with(&archive, None).is_err());
        // С неверным паролем — тоже.
        assert!(Archive::open_with(&archive, Some("другой")).is_err());

        let mut ar = Archive::open_with(&archive, Some("секрет")).unwrap();
        assert!(ar.is_encrypted());
        let out = dir.join("out");
        ar.extract(&ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            fs::read(out.join("src/a.txt")).unwrap(),
            fs::read(src.join("a.txt")).unwrap()
        );
        assert_eq!(
            fs::read(out.join("src/nested/c.bin")).unwrap(),
            fs::read(src.join("nested/c.bin")).unwrap()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn encrypted_large_file_streams() {
        // Больше лимита памяти на блок: проверяет потоковое шифрование.
        let dir = temp_dir("encrypted-big");
        fs::create_dir_all(dir.join("data")).unwrap();
        let big = vec![0x5Au8; (writer::INMEM_LIMIT + 1024 * 1024) as usize];
        fs::write(dir.join("data/big.bin"), &big).unwrap();

        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[dir.join("data")],
            &CreateOptions {
                show_progress: false,
                password: Some(Zeroizing::new("пароль".to_string())),
                level: 0,
                ..Default::default()
            },
        )
        .unwrap();

        let mut ar = Archive::open_with(&archive, Some("пароль")).unwrap();
        let out = dir.join("out");
        ar.extract(&ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(fs::read(out.join("data/big.bin")).unwrap(), big);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_volume_roundtrip() {
        let dir = temp_dir("volumes");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src.clone()],
            &CreateOptions {
                show_progress: false,
                level: 0,
                volume_size: Some(100_000),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            dir.join("a.plz.001").exists(),
            "первый том должен существовать"
        );
        assert!(dir.join("a.plz.002").exists(), "второй том должен быть");
        assert!(!archive.exists(), "одного файла быть не должно");

        let mut ar = Archive::open(&archive).unwrap();
        assert!(ar.files().len() > 1);
        let out = dir.join("out");
        ar.extract(&ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            fs::read(out.join("src/nested/c.bin")).unwrap(),
            fs::read(src.join("nested/c.bin")).unwrap()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_volume_is_detected() {
        let dir = temp_dir("volume-gap");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src],
            &CreateOptions {
                show_progress: false,
                level: 0,
                volume_size: Some(100_000),
                ..Default::default()
            },
        )
        .unwrap();
        fs::remove_file(dir.join("a.plz.002")).unwrap();
        assert!(
            Archive::open(&archive).is_err(),
            "недостача тома должна ломать чтение"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detects_corruption() {
        let dir = temp_dir("corrupt");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src],
            &CreateOptions {
                show_progress: false,
                solid: true,
                ..Default::default()
            },
        )
        .unwrap();

        let mut bytes = fs::read(&archive).unwrap();
        bytes[8] ^= 0xFF;
        fs::write(&archive, &bytes).unwrap();

        let mut ar = Archive::open(&archive).unwrap();
        assert!(ar.test(false, 0, None).is_err(), "порча данных должна ловиться");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_truncation() {
        let dir = temp_dir("trunc");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src],
            &CreateOptions {
                show_progress: false,
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = fs::read(&archive).unwrap();
        fs::write(&archive, &bytes[..bytes.len() - 10]).unwrap();
        assert!(Archive::open(&archive).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_existing_keeps_files() {
        let dir = temp_dir("skip");
        let src = build_sample(&dir);
        let archive = dir.join("a.plz");
        writer::create(
            &archive,
            &[src],
            &CreateOptions {
                show_progress: false,
                ..Default::default()
            },
        )
        .unwrap();

        let out = dir.join("out");
        fs::create_dir_all(out.join("src")).unwrap();
        fs::write(out.join("src/a.txt"), b"original").unwrap();

        let mut ar = Archive::open(&archive).unwrap();
        let stats = ar
            .extract(&ExtractOptions {
                output: out.clone(),
                skip_existing: true,
                show_progress: false,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(fs::read(out.join("src/a.txt")).unwrap(), b"original");

        let _ = fs::remove_dir_all(&dir);
    }
}
