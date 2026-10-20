//! Сборка архива: обход файловой системы, планирование блоков, запись.
//!
//! Память ограничена: блоки до [`INMEM_LIMIT`] собираются в память и сжимаются
//! параллельно, всё что больше — сжимается потоком прямо в файл. Поэтому архив
//! на десятки гигабайт собирается без риска упасть по памяти.
//!
//! Запись идёт через [`Sink`]: он умеет и один файл, и набор томов, и всегда
//! знает логическую позицию — на этом держится вся раскладка блоков.

use anyhow::{bail, ensure, Context, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zeroize::Zeroizing;

use crate::compress::{self, HashingReader};
use crate::crypto::{self, CryptoParams};
use crate::format::{
    self, Kind, Method, PzBlock, PzEntry, PzIndex, BLOCK_DATA_START, BLOCK_DATA_START_ENCRYPTED,
    FORMAT_VERSION, MAGIC, MAGIC_ENCRYPTED,
};
use crate::glob::Matcher;
use crate::safety;
use crate::util::{human_bytes, plural, Progress};

/// Блоки до этого размера собираются в память и сжимаются параллельно.
pub const INMEM_LIMIT: u64 = 32 * 1024 * 1024;

/// Максимальный суммарный объём одной пачки параллельных блоков.
const BATCH_BYTES: u64 = 256 * 1024 * 1024;

/// Размер буфера чтения при потоковой обработке.
const CHUNK: usize = 256 * 1024;

/// Размер solid-блока по умолчанию.
pub const DEFAULT_BLOCK_SIZE: u64 = 8 * 1024 * 1024;

/// Параметры создания архива.
pub struct CreateOptions {
    /// Степень сжатия 0..11.
    pub level: i32,
    /// Объединять мелкие файлы в общий solid-блок.
    pub solid: bool,
    /// Целевой размер solid-блока.
    pub block_size: u64,
    /// Комментарий внутрь индекса.
    pub comment: String,
    /// Следовать по символическим ссылкам вместо их сохранения.
    pub follow_links: bool,
    /// Показывать прогресс в stderr.
    pub show_progress: bool,
    /// Пароль: если задан, архив шифруется.
    pub password: Option<Zeroizing<String>>,
    /// Размер тома; `None` — писать одним файлом.
    pub volume_size: Option<u64>,
    /// Маски «включить только это».
    pub include: Vec<String>,
    /// Маски «исключить».
    pub exclude: Vec<String>,
    /// Получатель обновлений прогресса (GUI); `None` — только stderr.
    pub progress: Option<std::sync::Arc<dyn crate::util::ProgressSink>>,
}

impl Default for CreateOptions {
    fn default() -> Self {
        CreateOptions {
            level: compress::DEFAULT_LEVEL,
            solid: false,
            block_size: DEFAULT_BLOCK_SIZE,
            comment: String::new(),
            follow_links: false,
            show_progress: true,
            password: None,
            volume_size: None,
            include: Vec::new(),
            exclude: Vec::new(),
            progress: None,
        }
    }
}

impl std::fmt::Debug for CreateOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateOptions")
            .field("level", &self.level)
            .field("solid", &self.solid)
            .field("block_size", &self.block_size)
            .field("comment", &self.comment)
            .field("follow_links", &self.follow_links)
            .field("show_progress", &self.show_progress)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .field("volume_size", &self.volume_size)
            .field("include", &self.include)
            .field("exclude", &self.exclude)
            .finish_non_exhaustive()
    }
}

/// Итоги сборки архива.
#[derive(Debug, Clone)]
pub struct CreateStats {
    /// Сколько файлов записано.
    pub files: usize,
    /// Сколько каталогов записано.
    pub dirs: usize,
    /// Сколько ссылок записано.
    pub links: usize,
    /// Сколько блоков получилось.
    pub blocks: usize,
    /// Суммарный размер содержимого до сжатия.
    pub raw_bytes: u64,
    /// Суммарный размер сжатых данных.
    pub data_bytes: u64,
    /// Суммарный размер всех созданных файлов.
    pub archive_bytes: u64,
    /// Сколько томов создано.
    pub volumes: usize,
    /// Зашифрован ли архив.
    pub encrypted: bool,
}

/// Один элемент, найденный на диске.
#[derive(Debug, Clone)]
struct Candidate {
    rel: String,
    abs: PathBuf,
    kind: Kind,
    size: u64,
    mode: u32,
    mtime: i64,
    link: String,
}

/// Результат сжатия блока, который поместился в память.
struct MemBlock {
    packed: Vec<u8>,
    method: Method,
    size: u64,
    block_hash: [u8; 32],
    file_hashes: Vec<[u8; 32]>,
}

/// Приёмник байтов архива: один файл или последовательность томов.
///
/// Пишет во временные имена, а [`Sink::finish`] переименовывает их в целевые:
/// так недописанный архив не выглядит как готовый.
struct Sink {
    base: PathBuf,
    limit: Option<u64>,
    volume: u32,
    file: Option<BufWriter<File>>,
    planned: Vec<(PathBuf, PathBuf)>,
    pos: u64,
    in_volume: u64,
}

impl Sink {
    fn create(base: &Path, limit: Option<u64>) -> Result<Self> {
        if let Some(limit) = limit {
            ensure!(
                limit >= 64 * 1024,
                "размер тома слишком мал: {} байт",
                limit
            );
        }
        sweep_stale_temps(base);
        let mut sink = Sink {
            base: base.to_path_buf(),
            limit,
            volume: 0,
            file: None,
            planned: Vec::new(),
            pos: 0,
            in_volume: 0,
        };
        sink.open_next()?;
        Ok(sink)
    }

    fn open_next(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            let file = file
                .into_inner()
                .map_err(|err| anyhow::anyhow!("не удалось дописать том: {}", err))?;
            file.sync_all().context("не удалось сбросить том на диск")?;
        }

        self.volume += 1;
        let suffix = if self.limit.is_some() {
            format!(".{:03}", self.volume)
        } else {
            String::new()
        };
        let final_path = PathBuf::from(format!("{}{}", self.base.display(), suffix));
        let temp_path = PathBuf::from(format!(
            "{}{}.tmp{}",
            self.base.display(),
            suffix,
            std::process::id()
        ));

        let file = File::create(safety::native_path(&temp_path))
            .with_context(|| format!("не удалось создать {:?}", temp_path))?;
        self.planned.push((temp_path, final_path));
        self.file = Some(BufWriter::with_capacity(CHUNK, file));
        self.in_volume = 0;
        Ok(())
    }

    /// Логическая позиция записи (сумма по всем томам).
    fn pos(&self) -> u64 {
        self.pos
    }

    /// Сколько томов уже открыто.
    fn volumes(&self) -> usize {
        self.planned.len()
    }

    /// Дописывает всё и переименовывает временные файлы в целевые.
    fn finish(mut self) -> Result<Vec<PathBuf>> {
        if let Some(file) = self.file.take() {
            let file = file
                .into_inner()
                .map_err(|err| anyhow::anyhow!("не удалось дописать архив: {}", err))?;
            file.sync_all()
                .context("не удалось сбросить архив на диск")?;
        }

        let mut finished = Vec::with_capacity(self.planned.len());
        for (temp, final_path) in std::mem::take(&mut self.planned) {
            if final_path.exists() {
                fs::remove_file(safety::native_path(&final_path))
                    .with_context(|| format!("не удалось перезаписать {:?}", final_path))?;
            }
            fs::rename(safety::native_path(&temp), safety::native_path(&final_path))
                .with_context(|| format!("не удалось переименовать {:?}", temp))?;
            finished.push(final_path);
        }
        Ok(finished)
    }

    /// Убирает временные файлы после ошибки.
    fn abort(mut self) {
        self.file = None;
        for (temp, _) in std::mem::take(&mut self.planned) {
            let _ = fs::remove_file(safety::native_path(&temp));
        }
    }

    fn io_error(err: anyhow::Error) -> io::Error {
        io::Error::new(io::ErrorKind::Other, err.to_string())
    }
}

/// Подметает недописанные временные файлы прошлых запусков рядом с архивом.
///
/// Имя временного файла: `{base}.tmp{pid}` или `{base}.001.tmp{pid}`.
/// После kill -9 процесса такие файлы остаются навсегда, поэтому при
/// создании архива убираем осиротевшие записи. Параллельная запись того же
/// архива и так не поддерживается — обе сессии сойдутся на одном rename.
fn sweep_stale_temps(base: &Path) {
    let Some(parent) = base.parent() else { return };
    let Some(name) = base.file_name().and_then(|n| n.to_str()) else { return };
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(text) = file_name.to_str() else { continue };
        // `{name}.tmp…` или `{name}.NNN.tmp…`, хвост — только pid (цифры)
        let Some(after_name) = text.strip_prefix(name) else { continue };
        let Some(tmp_pos) = after_name.find(".tmp") else { continue };
        let pid_part = &after_name[tmp_pos + 4..];
        if !pid_part.is_empty() && pid_part.bytes().all(|b| b.is_ascii_digit()) {
            let _ = std::fs::remove_file(safety::native_path(&entry.path()));
        }
    }
}

impl Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let limit = self.limit;
        if let Some(limit) = limit {
            if self.in_volume >= limit {
                self.open_next().map_err(Sink::io_error)?;
            }
            let room = (limit - self.in_volume) as usize;
            let take = room.min(data.len());
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "том не открыт"))?;
            let written = file.write(&data[..take])?;
            self.pos += written as u64;
            self.in_volume += written as u64;
            Ok(written)
        } else {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "файл не открыт"))?;
            let written = file.write(data)?;
            self.pos += written as u64;
            Ok(written)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// Права доступа: на unix берём как есть, на Windows — восстанавливаемый минимум.
fn mode_of(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.mode() & 0o7777) as u32
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() {
            0o444
        } else if meta.is_dir() {
            0o755
        } else {
            0o644
        }
    }
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mtime()
    }
    #[cfg(not(unix))]
    {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Собирает список элементов из входных путей.
///
/// `dir` попадает в архив вместе со своим именем: `pliczip a x.plz photos`
/// даёт внутри `photos/...`, что совпадает с поведением 7-Zip.
fn collect(inputs: &[PathBuf], follow: bool, matcher: &Matcher) -> Result<Vec<Candidate>> {
    let mut out: Vec<Candidate> = Vec::new();

    for input in inputs {
        // Отдельная ветка на «путь не найден»: это самая частая ошибка при
        // наборе команды, и сырой текст от ОС тут только мешает.
        let meta = match fs::symlink_metadata(input) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                bail!("путь не найден: {:?}", input);
            }
            Err(err) => {
                return Err(err).with_context(|| format!("не удалось прочитать {:?}", input));
            }
        };

        if meta.file_type().is_symlink() && !follow {
            let name = input
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("не удалось получить имя для {:?}", input))?;
            let rel = safety::path_to_archive_string(Path::new(name))?;
            if !matcher.allows_file(&rel) {
                continue;
            }
            let target = fs::read_link(input)
                .with_context(|| format!("не удалось прочитать ссылку {:?}", input))?;
            out.push(Candidate {
                rel,
                abs: input.clone(),
                kind: Kind::Symlink,
                size: 0,
                mode: mode_of(&meta),
                mtime: mtime_of(&meta),
                link: target.to_string_lossy().into_owned(),
            });
            continue;
        }

        let base = input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        if meta.is_file() {
            let rel_path = input.strip_prefix(&base).unwrap_or(input);
            let rel = safety::path_to_archive_string(rel_path)?;
            if !matcher.allows_file(&rel) {
                continue;
            }
            out.push(Candidate {
                rel,
                abs: input.clone(),
                kind: Kind::File,
                size: meta.len(),
                mode: mode_of(&meta),
                mtime: mtime_of(&meta),
                link: String::new(),
            });
        } else if meta.is_dir() {
            let walker = WalkDir::new(input)
                .follow_links(follow)
                .sort_by_file_name()
                .into_iter()
                .filter_entry(|entry| {
                    // Исключённый каталог не обходим вообще: иначе на большом
                    // дереве впустую тратится время на отбрасывание файлов.
                    entry
                        .path()
                        .strip_prefix(&base)
                        .ok()
                        .and_then(|rel| safety::path_to_archive_string(rel).ok())
                        .map(|rel| matcher.allows_dir(&rel))
                        .unwrap_or(true)
                });

            for entry in walker {
                let entry = entry.with_context(|| format!("ошибка обхода каталога {:?}", input))?;
                let path = entry.path();
                let rel_path = path
                    .strip_prefix(&base)
                    .with_context(|| format!("путь {:?} не входит в {:?}", path, base))?;
                let rel = safety::path_to_archive_string(rel_path)?;
                let fs_meta = fs::symlink_metadata(path)
                    .with_context(|| format!("не удалось прочитать {:?}", path))?;

                let is_dir = fs_meta.is_dir();
                if !is_dir && !matcher.allows_file(&rel) {
                    continue;
                }

                if fs_meta.file_type().is_symlink() && !follow {
                    let target = fs::read_link(path)
                        .with_context(|| format!("не удалось прочитать ссылку {:?}", path))?;
                    out.push(Candidate {
                        rel,
                        abs: path.to_path_buf(),
                        kind: Kind::Symlink,
                        size: 0,
                        mode: mode_of(&fs_meta),
                        mtime: mtime_of(&fs_meta),
                        link: target.to_string_lossy().into_owned(),
                    });
                } else if is_dir {
                    out.push(Candidate {
                        rel,
                        abs: path.to_path_buf(),
                        kind: Kind::Dir,
                        size: 0,
                        mode: mode_of(&fs_meta),
                        mtime: mtime_of(&fs_meta),
                        link: String::new(),
                    });
                } else if fs_meta.is_file() {
                    out.push(Candidate {
                        rel,
                        abs: path.to_path_buf(),
                        kind: Kind::File,
                        size: fs_meta.len(),
                        mode: mode_of(&fs_meta),
                        mtime: mtime_of(&fs_meta),
                        link: String::new(),
                    });
                } else {
                    eprintln!("  пропущен специальный файл: {:?}", path);
                }
            }
        } else {
            bail!("неподдерживаемый тип файла: {:?}", input);
        }
    }

    out.sort_by(|a, b| a.rel.cmp(&b.rel));

    let mut seen: HashSet<String> = HashSet::with_capacity(out.len());
    for c in &out {
        if !seen.insert(c.rel.to_lowercase()) {
            bail!(
                "в архив попали два элемента с одинаковым путём: {} — переименуйте один из них",
                c.rel
            );
        }
    }

    Ok(out)
}

/// Сжимает блок, содержимое которого уже в памяти.
fn build_mem_block(
    cands: &[Candidate],
    idxs: &[usize],
    method: Method,
    level: i32,
) -> Result<MemBlock> {
    let total: u64 = idxs.iter().map(|&i| cands[i].size).sum();
    let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
    let mut file_hashes = Vec::with_capacity(idxs.len());

    for &i in idxs {
        let cand = &cands[i];
        let file =
            File::open(safety::native_path(&cand.abs)).with_context(|| format!("не удалось открыть {:?}", cand.abs))?;
        let mut reader = BufReader::with_capacity(CHUNK, file);
        let before = buf.len();
        let read = reader
            .by_ref()
            .take(cand.size.saturating_add(1))
            .read_to_end(&mut buf)
            .with_context(|| format!("ошибка чтения {:?}", cand.abs))? as u64;
        ensure!(
            read == cand.size,
            "файл {:?} изменился во время архивации (ожидалось {} байт, прочитано {})",
            cand.abs,
            cand.size,
            read
        );
        file_hashes.push(*blake3::hash(&buf[before..]).as_bytes());
    }

    let block_hash = *blake3::hash(&buf).as_bytes();
    let (packed, used) = compress::compress_block(method, level, &buf)?;

    Ok(MemBlock {
        packed,
        method: used,
        size: total,
        block_hash,
        file_hashes,
    })
}

/// Сжимает один файл потоком в уже открытый приёмник.
///
/// Возвращает хеш и размер несжатых данных.
fn compress_file_into<W: Write>(
    out: &mut W,
    path: &Path,
    method: Method,
    level: i32,
) -> Result<([u8; 32], u64)> {
    let file = File::open(safety::native_path(path)).with_context(|| format!("не удалось открыть {:?}", path))?;
    let mut reader = HashingReader::new(BufReader::with_capacity(CHUNK, file));
    compress::compress_stream(method, level, &mut reader, out)
        .with_context(|| format!("ошибка сжатия {:?}", path))?;
    Ok(reader.finish())
}

/// Шифрует готовый буфер, привязывая его к номеру блока.
fn seal_block(key: &[u8; 32], base: &[u8; 12], block: u32, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(crypto::sealed_len(data.len() as u64) as usize);
    {
        let mut writer = crypto::EncryptingWriter::new(&mut out, key, base, block);
        writer
            .write_all(data)
            .map_err(|err| anyhow::anyhow!("ошибка шифрования: {}", err))?;
        writer
            .finish()
            .map_err(|err| anyhow::anyhow!("ошибка шифрования: {}", err))?;
    }
    Ok(out)
}

/// Создаёт архив по входным путям.
pub fn create(output: &Path, inputs: &[PathBuf], opts: &CreateOptions) -> Result<CreateStats> {
    ensure!(!inputs.is_empty(), "не указано, что архивировать");
    ensure!(
        (compress::MIN_LEVEL..=compress::MAX_LEVEL).contains(&opts.level),
        "степень сжатия должна быть от {} до {}",
        compress::MIN_LEVEL,
        compress::MAX_LEVEL
    );

    let matcher = Matcher::new(&opts.include, &opts.exclude);
    let candidates = collect(inputs, opts.follow_links, &matcher)?;
    ensure!(
        !candidates.is_empty(),
        "после применения фильтров нечего архивировать"
    );

    let method = compress::method_for_level(opts.level);
    let raw_total: u64 = candidates.iter().map(|c| c.size).sum();

    let file_idx: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == Kind::File && c.size > 0)
        .map(|(i, _)| i)
        .collect();

    // Планирование блоков.
    let mut blocks: Vec<Vec<usize>> = Vec::new();
    if opts.solid {
        let mut current: Vec<usize> = Vec::new();
        let mut current_size = 0u64;
        for &i in &file_idx {
            let size = candidates[i].size;
            let overflows = !current.is_empty() && current_size + size > opts.block_size;
            if overflows {
                blocks.push(std::mem::take(&mut current));
                current_size = 0;
            }
            current.push(i);
            current_size += size;
        }
        if !current.is_empty() {
            blocks.push(current);
        }
    } else {
        for &i in &file_idx {
            blocks.push(vec![i]);
        }
    }

    // Ключ и параметры шифрования (соль и база nonce — случайные на архив).
    let crypto_setup = match &opts.password {
        Some(password) => {
            let params = CryptoParams::generate()?;
            let key = crypto::derive_key(password.as_bytes(), &params.salt, &params.kdf)?;
            Some((params, key))
        }
        None => None,
    };
    let encrypted = crypto_setup.is_some();
    let magic = if encrypted { MAGIC_ENCRYPTED } else { MAGIC };
    let data_start = if encrypted {
        BLOCK_DATA_START_ENCRYPTED
    } else {
        BLOCK_DATA_START
    };

    let mut progress = Progress::new("сжатие", raw_total, opts.show_progress && raw_total > 0);
    if let Some(sink) = &opts.progress {
        progress = progress.with_sink(std::sync::Arc::clone(sink));
    }
    let mut sink = Sink::create(output, opts.volume_size)?;

    let result = (|| -> Result<(Vec<PzBlock>, CreateStats)> {
        sink.write_all(&magic)?;
        if let Some((params, _)) = &crypto_setup {
            sink.write_all(&params.to_bytes())?;
        }
        ensure!(
            sink.pos() == data_start,
            "внутренняя ошибка: заголовок занял {} байт вместо {}",
            sink.pos(),
            data_start
        );

        let mut block_metas: Vec<PzBlock> = Vec::with_capacity(blocks.len());
        let mut file_hashes: Vec<[u8; 32]> = vec![[0u8; 32]; candidates.len()];
        let mut placement: Vec<Option<(u32, u64)>> = vec![None; candidates.len()];
        let mut data_bytes = 0u64;

        let threads = rayon::current_num_threads().max(1);
        let mut i = 0usize;

        while i < blocks.len() {
            // Набираем пачку блоков, которые можно сжать параллельно в памяти.
            let mut end = i;
            let mut batch_bytes = 0u64;
            while end < blocks.len() {
                let size: u64 = blocks[end].iter().map(|&c| candidates[c].size).sum();
                if size > INMEM_LIMIT {
                    break;
                }
                if end > i && (batch_bytes + size > BATCH_BYTES || end - i >= threads) {
                    break;
                }
                batch_bytes += size;
                end += 1;
            }

            if end == i {
                // Блок не помещается в память: пишем его файлы по одному, потоком.
                // Solid-склейку здесь приходится разорвать, иначе весь блок пришлось
                // бы держать в памяти — а это ровно то, чего мы избегаем.
                for &ci in &blocks[i] {
                    let index = block_metas.len() as u32;
                    let offset = sink.pos();
                    let (block_hash, size, payload_size) = match &crypto_setup {
                        Some((params, key)) => {
                            let mut writer = crypto::EncryptingWriter::new(
                                &mut sink,
                                key,
                                &params.nonce_base,
                                index,
                            );
                            let (hash, size) = compress_file_into(
                                &mut writer,
                                &candidates[ci].abs,
                                method,
                                opts.level,
                            )?;
                            let (_, sealed, plain) = writer
                                .finish()
                                .map_err(|err| anyhow::anyhow!("ошибка шифрования: {}", err))?;
                            ensure!(
                                sealed == crypto::sealed_len(plain),
                                "внутренняя ошибка: длина шифротекста не совпала с расчётной"
                            );
                            (hash, size, plain)
                        }
                        None => {
                            let (hash, size) = compress_file_into(
                                &mut sink,
                                &candidates[ci].abs,
                                method,
                                opts.level,
                            )?;
                            (hash, size, 0)
                        }
                    };
                    let stored = sink.pos() - offset;
                    ensure!(
                        size > 0,
                        "файл {:?} оказался пустым, хотя заявлен непустым",
                        candidates[ci].abs
                    );
                    file_hashes[ci] = block_hash;
                    placement[ci] = Some((index, 0));
                    block_metas.push(PzBlock {
                        offset,
                        stored_size: stored,
                        payload_size,
                        size,
                        method,
                        level: opts.level as i8,
                        hash: block_hash,
                    });
                    data_bytes += stored;
                    progress.add(size);
                }
                i += 1;
                continue;
            }

            let batch = &blocks[i..end];
            let results: Vec<Result<MemBlock>> = batch
                .par_iter()
                .map(|idxs| build_mem_block(&candidates, idxs, method, opts.level))
                .collect();

            for (n, res) in results.into_iter().enumerate() {
                let mb = res?;
                let index = block_metas.len() as u32;
                let offset = sink.pos();

                let (payload, payload_plain) = match &crypto_setup {
                    Some((params, key)) => {
                        let sealed = seal_block(key, &params.nonce_base, index, &mb.packed)?;
                        (sealed, mb.packed.len() as u64)
                    }
                    None => (mb.packed, 0),
                };
                sink.write_all(&payload)?;
                let stored = payload.len() as u64;

                block_metas.push(PzBlock {
                    offset,
                    stored_size: stored,
                    payload_size: payload_plain,
                    size: mb.size,
                    method: mb.method,
                    level: opts.level as i8,
                    hash: mb.block_hash,
                });
                data_bytes += stored;

                let mut block_offset = 0u64;
                for (&ci, &hash) in batch[n].iter().zip(mb.file_hashes.iter()) {
                    file_hashes[ci] = hash;
                    placement[ci] = Some((index, block_offset));
                    block_offset += candidates[ci].size;
                }
                progress.add(mb.size);
            }
            i = end;
        }

        // Собираем элементы архива в том же порядке, что и кандидаты.
        let empty_hash = *blake3::hash(b"").as_bytes();
        let mut entries: Vec<PzEntry> = Vec::with_capacity(candidates.len());
        for (i, cand) in candidates.iter().enumerate() {
            let (block, block_offset) = match placement[i] {
                Some((b, o)) => (Some(b), o),
                None => (None, 0),
            };
            let hash = match cand.kind {
                Kind::File if cand.size == 0 => empty_hash,
                Kind::File => file_hashes[i],
                _ => [0u8; 32],
            };
            entries.push(PzEntry {
                path: cand.rel.clone(),
                kind: cand.kind,
                size: cand.size,
                hash,
                mode: cand.mode,
                mtime: cand.mtime,
                block,
                block_offset,
                link: cand.link.clone(),
            });
        }

        let index = PzIndex {
            version: FORMAT_VERSION,
            tool: format::tool_string(),
            created_at: format::now_unix(),
            comment: opts.comment.clone(),
            solid: opts.solid,
            blocks: block_metas,
            entries,
        };

        format::validate_index(&index, sink.pos(), data_start, encrypted)
            .context("внутренняя ошибка: собранный индекс не проходит проверку")?;

        let raw = format::serialize_index(&index)?;
        let payload = match &crypto_setup {
            Some((params, key)) => crypto::encrypt(
                key,
                &crypto::nonce_for_chunk(
                    &params.nonce_base,
                    crypto::INDEX_BLOCK,
                    crypto::INDEX_CHUNK,
                ),
                crypto::INDEX_AAD,
                &raw,
            )?,
            None => raw,
        };
        format::write_index_and_footer(&mut sink, &payload, magic)?;
        sink.flush()?;

        let files = candidates.iter().filter(|c| c.kind == Kind::File).count();
        let dirs = candidates.iter().filter(|c| c.kind == Kind::Dir).count();
        let links = candidates
            .iter()
            .filter(|c| c.kind == Kind::Symlink)
            .count();

        Ok((
            index.blocks,
            CreateStats {
                files,
                dirs,
                links,
                blocks: 0,
                raw_bytes: raw_total,
                data_bytes,
                archive_bytes: sink.pos(),
                volumes: sink.volumes(),
                encrypted,
            },
        ))
    })();

    let (block_metas, mut stats) = match result {
        Ok(value) => value,
        Err(err) => {
            sink.abort();
            return Err(err);
        }
    };

    let created = sink.finish()?;
    stats.blocks = block_metas.len();
    stats.volumes = created.len();
    stats.archive_bytes = created
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();

    progress.finish_with(&format!(
        "готово: {}, {}, архив {}{}",
        plural(stats.files, "файл", "файла", "файлов"),
        plural(stats.dirs, "каталог", "каталога", "каталогов"),
        human_bytes(stats.archive_bytes),
        if stats.volumes > 1 {
            format!(" в {}", plural(stats.volumes, "томе", "томах", "томах"))
        } else {
            String::new()
        }
    ));

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        // Каталог внутри проекта: системный %TEMP% может быть закрыт песочницей.
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("w-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn collect_keeps_tree_shape() {
        let dir = temp_dir("collect");
        fs::create_dir_all(dir.join("sub/deep")).unwrap();
        fs::write(dir.join("a.txt"), b"a").unwrap();
        fs::write(dir.join("sub/b.txt"), b"b").unwrap();
        fs::write(dir.join("sub/deep/c.txt"), b"c").unwrap();

        let cands = collect(&[dir.join("sub")], false, &Matcher::new(&[], &[])).unwrap();
        let mut paths: Vec<&str> = cands.iter().map(|c| c.rel.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["sub", "sub/b.txt", "sub/deep", "sub/deep/c.txt"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_rejects_duplicate_targets() {
        let dir = temp_dir("dupes");
        fs::write(dir.join("x.txt"), b"x").unwrap();
        let err = collect(
            &[dir.join("x.txt"), dir.join("x.txt")],
            false,
            &Matcher::new(&[], &[]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("одинаковым путём"), "{}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_respects_excludes() {
        let dir = temp_dir("filters");
        fs::create_dir_all(dir.join("data/skip")).unwrap();
        fs::write(dir.join("data/keep.txt"), b"keep").unwrap();
        fs::write(dir.join("data/drop.tmp"), b"drop").unwrap();
        fs::write(dir.join("data/skip/inside.txt"), b"inside").unwrap();

        let matcher = Matcher::new(&[], &["**/*.tmp".to_string(), "**/skip".to_string()]);
        let cands = collect(&[dir.join("data")], false, &matcher).unwrap();
        let paths: Vec<&str> = cands.iter().map(|c| c.rel.as_str()).collect();
        assert!(paths.contains(&"data/keep.txt"));
        assert!(!paths.iter().any(|p| p.ends_with(".tmp")), "{:?}", paths);
        assert!(!paths.iter().any(|p| p.contains("skip")), "{:?}", paths);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_and_reopen() {
        let dir = temp_dir("create");
        fs::create_dir_all(dir.join("data/inner")).unwrap();
        fs::write(dir.join("data/inner/file.txt"), b"hello pliczip").unwrap();
        fs::write(dir.join("data/top.bin"), vec![7u8; 10_000]).unwrap();

        let archive = dir.join("out.plz");
        let opts = CreateOptions {
            show_progress: false,
            ..Default::default()
        };
        let stats = create(&archive, &[dir.join("data")], &opts).unwrap();
        assert_eq!(stats.files, 2);
        assert!(!stats.encrypted);
        assert_eq!(stats.volumes, 1);
        assert!(archive.exists());

        let (source, _) = crate::source::open(&archive).unwrap();
        let layout = format::inspect(source.as_ref()).unwrap();
        let parsed = format::read_index(source.as_ref(), &layout, None).unwrap();
        assert_eq!(parsed.index.entries.len(), stats.files + stats.dirs);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_encrypted_and_volumes() {
        let dir = temp_dir("encvol");
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(dir.join("data/a.txt"), vec![b'x'; 200_000]).unwrap();

        let archive = dir.join("out.plz");
        let opts = CreateOptions {
            show_progress: false,
            password: Some(Zeroizing::new("пароль".to_string())),
            level: 0,
            volume_size: Some(100_000),
            ..Default::default()
        };
        let stats = create(&archive, &[dir.join("data")], &opts).unwrap();
        assert!(stats.encrypted);
        assert!(
            stats.volumes > 1,
            "ожидались тома, получилось {}",
            stats.volumes
        );

        let (source, names) = crate::source::open(&archive).unwrap();
        assert_eq!(names.len(), stats.volumes);
        let layout = format::inspect(source.as_ref()).unwrap();
        assert!(layout.encrypted);

        // Без ключа индекс не читается.
        assert!(format::read_index(source.as_ref(), &layout, None).is_err());

        // С ключом — читается.
        let params = layout.crypto.as_ref().unwrap();
        let key = crypto::derive_key("пароль".as_bytes(), &params.salt, &params.kdf).unwrap();
        let parsed = format::read_index(source.as_ref(), &layout, Some(&key)).unwrap();
        assert_eq!(parsed.index.entries.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_removes_orphaned_temps_only() {
        let dir = temp_dir("sweep");
        let archive = dir.join("out.plz");
        // Осиротевший временный файл после kill -9.
        fs::write(dir.join("out.plz.tmp424242"), b"stale").unwrap();
        fs::write(dir.join("out.plz.001.tmp424242"), b"stale").unwrap();
        // Совсем другая запись — не наша.
        fs::write(dir.join("other.plz.tmp1"), b"keep").unwrap();
        // Наше имя, но не временный файл.
        fs::write(dir.join("out.plz.notes"), b"keep").unwrap();

        sweep_stale_temps(&archive);

        assert!(!dir.join("out.plz.tmp424242").exists());
        assert!(!dir.join("out.plz.001.tmp424242").exists());
        assert!(dir.join("other.plz.tmp1").exists());
        assert!(dir.join("out.plz.notes").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
