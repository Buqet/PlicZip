//! Запись для восстановления (Recovery Record, «RR»).
//!
//! Секция дописывается после футера архива и защищает все байты перед ней
//! (`[0, protected_len)`): заголовок, данные, индекс и футер. Она работает
//! поверх уже зашифрованных байт, поэтому пароль для неё не нужен.
//!
//! ```text
//! [ архив: protected_len байт                        ]
//! [ копия трейлера, 128 байт                         ]
//! [ таблица хешей: (data + parity) × 32 байта        ]
//! [ parity-куски: parity × shard_size байт           ]
//! [ трейлер, 128 байт, последние 4 байта — «PLZR»    ]
//! ```
//!
//! Архив режется на куски по `shard_size` байт (последний дополняется нулями).
//! Кусок `j` попадает в группу `j % groups`, поэтому сплошное повреждение
//! размазывается по разным группам. Для группы из `k` кусков данных кодом
//! Рида — Соломона считается `max(1, ⌈k·percent/100⌉)` parity-кусков: группа
//! переживает потерю стольких любых своих кусков.
//!
//! Хеши кусков (BLAKE3) собраны в дерево Меркла, корень которого лежит
//! в трейлере: по корню проверяется таблица, по таблице — каждый кусок.
//! Читатель прячет секцию: остальной код видит архив ровно таким, каким
//! он был до добавления записи.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use reed_solomon_simd::ReedSolomonEncoder;

use crate::source::{FileSource, Source};
use crate::util::{Progress, ProgressSink};

/// Сигнатура записи для восстановления (в начале и в конце трейлера).
pub const MAGIC: [u8; 4] = *b"PLZR";
/// Версия формата записи.
pub const VERSION: u8 = 1;
/// Длина трейлера и его копии.
pub const TRAILER_LEN: u64 = 128;
/// Длина одного хеша в таблице.
pub const HASH_LEN: u64 = 32;
/// Наименьший размер куска.
pub const MIN_SHARD_SIZE: u64 = 4096;
/// Наибольший размер куска.
pub const MAX_SHARD_SIZE: u64 = 64 * 1024;
/// Наибольшее число кусков данных в одной группе.
pub const MAX_GROUP_DATA: u64 = 512;
/// Наименьший объём записи в процентах.
pub const MIN_PERCENT: u32 = 1;
/// Наибольший объём записи в процентах.
pub const MAX_PERCENT: u32 = 100;

const LEAF_CONTEXT: &str = "pliczip rr leaf v1";
const NODE_CONTEXT: &str = "pliczip rr node v1";
const TRAILER_CONTEXT: &str = "pliczip rr trailer v1";
/// Сколько первых байт трейлера покрывает его хеш.
const TRAILER_HASHED: usize = 92;
/// Окно обратного поиска копии трейлера.
const SCAN_WINDOW: u64 = 1 << 20;

/// Раскладка записи: всё выводится из длины архива и процента.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Сколько байт в начале файла защищено.
    pub protected_len: u64,
    /// Размер куска.
    pub shard_size: u64,
    /// Объём записи в процентах от данных.
    pub percent: u32,
    /// Сколько кусков данных.
    pub data_shards: u64,
    /// Сколько групп.
    pub groups: u64,
    /// Сколько parity-кусков всего.
    pub parity_shards: u64,
}

impl Plan {
    /// Считает раскладку для архива длиной `protected_len`.
    pub fn new(protected_len: u64, percent: u32) -> Result<Plan> {
        ensure!(protected_len > 0, "нечего защищать: архив пуст");
        ensure!(
            (MIN_PERCENT..=MAX_PERCENT).contains(&percent),
            "объём записи для восстановления должен быть от {}% до {}%",
            MIN_PERCENT,
            MAX_PERCENT
        );
        let shard_size = protected_len
            .div_ceil(4096)
            .next_multiple_of(4096)
            .clamp(MIN_SHARD_SIZE, MAX_SHARD_SIZE);
        let data_shards = protected_len.div_ceil(shard_size);
        let groups = data_shards.div_ceil(MAX_GROUP_DATA);
        let mut plan = Plan {
            protected_len,
            shard_size,
            percent,
            data_shards,
            groups,
            parity_shards: 0,
        };
        ensure!(
            groups <= u64::from(u32::MAX),
            "архив слишком велик для записи для восстановления"
        );
        plan.parity_shards = plan.parity_before(groups);
        plan.total_len()
            .context("архив слишком велик для записи для восстановления")?;
        Ok(plan)
    }

    /// Сколько кусков данных в группе `g`.
    pub fn group_data(&self, g: u64) -> u64 {
        self.data_shards / self.groups + u64::from(g < self.data_shards % self.groups)
    }

    /// Сколько parity-кусков в группе `g`.
    pub fn group_parity(&self, g: u64) -> u64 {
        self.parity_for(self.group_data(g))
    }

    fn parity_for(&self, k: u64) -> u64 {
        (k * u64::from(self.percent)).div_ceil(100).max(1)
    }

    /// Сколько parity-кусков лежит до группы `g` (её первый глобальный номер).
    pub fn parity_before(&self, g: u64) -> u64 {
        let big = self.data_shards % self.groups;
        let small_k = self.data_shards / self.groups;
        let big_m = self.parity_for(small_k + 1);
        let small_m = self.parity_for(small_k);
        if g <= big {
            g * big_m
        } else {
            big * big_m + (g - big) * small_m
        }
    }

    /// Группа, которой принадлежит parity-кусок с глобальным номером `t`.
    pub fn parity_group(&self, t: u64) -> u64 {
        let big = self.data_shards % self.groups;
        let small_k = self.data_shards / self.groups;
        let big_m = self.parity_for(small_k + 1);
        let small_m = self.parity_for(small_k);
        if t < big * big_m {
            t / big_m
        } else {
            big + (t - big * big_m) / small_m
        }
    }

    /// Номер куска данных: `i`-й кусок группы `g`.
    pub fn data_index(&self, g: u64, i: u64) -> u64 {
        i * self.groups + g
    }

    /// Сколько листьев в дереве Меркла (кусков данных и parity).
    pub fn leaves(&self) -> u64 {
        self.data_shards + self.parity_shards
    }

    /// Смещение таблицы хешей.
    pub fn table_offset(&self) -> u64 {
        self.protected_len + TRAILER_LEN
    }

    /// Смещение первого parity-куска.
    pub fn parity_offset(&self) -> u64 {
        self.table_offset() + self.leaves() * HASH_LEN
    }

    /// Смещение конечного трейлера.
    pub fn trailer_offset(&self) -> u64 {
        self.parity_offset() + self.parity_shards * self.shard_size
    }

    /// Полная длина файла с записью; `None` при переполнении.
    pub fn total_len(&self) -> Option<u64> {
        let table = self.leaves().checked_mul(HASH_LEN)?;
        let parity = self.parity_shards.checked_mul(self.shard_size)?;
        self.protected_len
            .checked_add(TRAILER_LEN * 2)?
            .checked_add(table)?
            .checked_add(parity)
    }

    /// Сколько байт добавляет запись.
    pub fn section_len(&self) -> u64 {
        self.total_len().unwrap_or(u64::MAX) - self.protected_len
    }
}

/// Трейлер записи: раскладка и корень дерева Меркла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    /// Раскладка.
    pub plan: Plan,
    /// Корень дерева Меркла над хешами всех кусков.
    pub merkle_root: [u8; 32],
}

impl Trailer {
    /// Сериализует трейлер в 128 байт.
    pub fn to_bytes(&self) -> [u8; TRAILER_LEN as usize] {
        let plan = &self.plan;
        let mut out = [0u8; TRAILER_LEN as usize];
        out[0..4].copy_from_slice(&MAGIC);
        out[4] = VERSION;
        out[8..16].copy_from_slice(&plan.protected_len.to_le_bytes());
        out[16..20].copy_from_slice(&(plan.shard_size as u32).to_le_bytes());
        out[20..24].copy_from_slice(&plan.percent.to_le_bytes());
        out[24..32].copy_from_slice(&plan.data_shards.to_le_bytes());
        out[32..40].copy_from_slice(&plan.parity_shards.to_le_bytes());
        out[40..44].copy_from_slice(&(plan.groups as u32).to_le_bytes());
        out[48..80].copy_from_slice(&self.merkle_root);
        let hash = trailer_hash(&out[..TRAILER_HASHED]);
        out[92..124].copy_from_slice(&hash);
        out[124..128].copy_from_slice(&MAGIC);
        out
    }

    /// Разбирает и проверяет трейлер.
    pub fn from_bytes(raw: &[u8; TRAILER_LEN as usize]) -> Result<Trailer> {
        ensure!(
            raw[0..4] == MAGIC && raw[124..128] == MAGIC,
            "нет сигнатуры записи для восстановления"
        );
        ensure!(
            raw[92..124] == trailer_hash(&raw[..TRAILER_HASHED]),
            "контрольная сумма трейлера записи для восстановления не сходится"
        );
        ensure!(
            raw[4] == VERSION,
            "версия записи для восстановления {} не поддерживается",
            raw[4]
        );
        let u32_at = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
        let u64_at = |at: usize| u64::from_le_bytes(raw[at..at + 8].try_into().unwrap());

        let plan = Plan::new(u64_at(8), u32_at(20))?;
        ensure!(
            plan.shard_size == u64::from(u32_at(16))
                && plan.data_shards == u64_at(24)
                && plan.parity_shards == u64_at(32)
                && plan.groups == u64::from(u32_at(40)),
            "параметры записи для восстановления противоречат друг другу"
        );
        let mut merkle_root = [0u8; 32];
        merkle_root.copy_from_slice(&raw[48..80]);
        Ok(Trailer { plan, merkle_root })
    }
}

fn trailer_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(TRAILER_CONTEXT);
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

/// Хеш листа: BLAKE3 куска с отдельным контекстом.
pub fn leaf_hash(shard: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(LEAF_CONTEXT);
    hasher.update(shard);
    *hasher.finalize().as_bytes()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(NODE_CONTEXT);
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Корень дерева Меркла. Непарный узел поднимается на уровень выше как есть.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => node_hash(left, right),
                [single] => *single,
                _ => unreachable!(),
            })
            .collect();
    }
    level[0]
}

/// Источник, который видит только защищённую часть файла.
struct Protected {
    inner: Box<dyn Source>,
    len: u64,
}

impl Source for Protected {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let max = (self.len - offset).min(buf.len() as u64) as usize;
        self.inner.read_at(&mut buf[..max], offset)
    }
}

/// Ищет запись для восстановления в конце источника.
///
/// Если конечный трейлер повреждён, ищет его копию обратным просмотром.
pub fn locate(src: &dyn Source) -> Result<Option<Trailer>> {
    let len = src.len();
    if len < TRAILER_LEN * 2 {
        return Ok(None);
    }
    let mut tail = [0u8; 4];
    src.read_exact_at(&mut tail, len - 4)?;
    if tail != MAGIC {
        return Ok(None);
    }

    let mut raw = [0u8; TRAILER_LEN as usize];
    src.read_exact_at(&mut raw, len - TRAILER_LEN)?;
    if let Ok(trailer) = Trailer::from_bytes(&raw) {
        if trailer.plan.total_len() == Some(len) {
            return Ok(Some(trailer));
        }
    }
    if let Some(trailer) = find_copy(src)? {
        return Ok(Some(trailer));
    }
    bail!(
        "запись для восстановления повреждена: трейлер в конце файла не прошёл проверку, \
         а его копия не найдена"
    )
}

/// Обратный поиск копии трейлера: она лежит ровно на `protected_len`.
fn find_copy(src: &dyn Source) -> Result<Option<Trailer>> {
    let len = src.len();
    let mut hi = len - TRAILER_LEN;
    let mut window = Vec::new();
    loop {
        let lo = hi.saturating_sub(SCAN_WINDOW);
        window.resize((hi - lo) as usize, 0);
        src.read_exact_at(&mut window, lo)?;
        let hits: Vec<usize> = window
            .windows(4)
            .enumerate()
            .filter(|(_, bytes)| *bytes == MAGIC)
            .map(|(at, _)| at)
            .collect();
        for at in hits.into_iter().rev() {
            let pos = lo + at as u64;
            if pos == 0 || pos + TRAILER_LEN * 2 > len {
                continue;
            }
            let mut raw = [0u8; TRAILER_LEN as usize];
            src.read_exact_at(&mut raw, pos)?;
            if let Ok(trailer) = Trailer::from_bytes(&raw) {
                if trailer.plan.protected_len == pos {
                    return Ok(Some(trailer));
                }
            }
        }
        if lo == 0 {
            return Ok(None);
        }
        hi = lo + 3;
    }
}

/// Прячет запись для восстановления: возвращает источник длиной
/// `protected_len` и трейлер, если запись есть.
pub fn protect(src: Box<dyn Source>) -> Result<(Box<dyn Source>, Option<Trailer>)> {
    match locate(src.as_ref())? {
        Some(trailer) => {
            let len = trailer.plan.protected_len;
            Ok((Box::new(Protected { inner: src, len }), Some(trailer)))
        }
        None => Ok((src, None)),
    }
}

/// Читает кусок данных `j`, дополняя последний нулями.
fn read_shard(src: &dyn Source, plan: &Plan, j: u64, buf: &mut [u8]) -> io::Result<usize> {
    let start = j * plan.shard_size;
    let n = (plan.protected_len - start).min(plan.shard_size) as usize;
    src.read_exact_at(&mut buf[..n], start)?;
    buf[n..].fill(0);
    Ok(n)
}

fn new_progress(
    label: &'static str,
    total: u64,
    show: bool,
    sink: Option<&Arc<dyn ProgressSink>>,
) -> Progress {
    let progress = Progress::new(label, total, show && total > 0);
    match sink {
        Some(sink) => progress.with_sink(Arc::clone(sink)),
        None => progress,
    }
}

/// Дописывает запись для восстановления к однофайловому архиву.
///
/// При ошибке файл обрезается до исходной длины.
pub fn add_to_file(
    path: &Path,
    percent: u32,
    show_progress: bool,
    sink: Option<&Arc<dyn ProgressSink>>,
) -> Result<Trailer> {
    let source = FileSource::open(path)?;
    crate::format::inspect(&source)?;
    match locate(&source) {
        Ok(None) => {}
        _ => bail!("в архиве уже есть запись для восстановления"),
    }
    let plan = Plan::new(source.len(), percent)?;

    let mut file = OpenOptions::new()
        .write(true)
        .open(crate::safety::native_path(path))
        .with_context(|| format!("не удалось открыть {:?} на запись", path))?;
    let mut progress = new_progress("восстановление", plan.protected_len, show_progress, sink);
    let result = write_section(&source, &mut file, &plan, &mut progress).and_then(|trailer| {
        file.sync_all()
            .context("не удалось сбросить запись для восстановления на диск")?;
        Ok(trailer)
    });
    progress.finish();
    if result.is_err() {
        let _ = file.set_len(plan.protected_len);
    }
    result
}

fn write_section(
    src: &dyn Source,
    file: &mut File,
    plan: &Plan,
    progress: &mut Progress,
) -> Result<Trailer> {
    let shard = plan.shard_size as usize;
    let mut leaves = vec![[0u8; 32]; plan.leaves() as usize];
    let mut buf = vec![0u8; shard];
    let mut encoder: Option<ReedSolomonEncoder> = None;
    let mut parity_index = plan.data_shards as usize;

    file.seek(SeekFrom::Start(plan.parity_offset()))?;
    let mut out = BufWriter::with_capacity(1 << 20, &mut *file);
    for g in 0..plan.groups {
        let k = plan.group_data(g) as usize;
        let m = plan.group_parity(g) as usize;
        let encoder = match encoder.as_mut() {
            Some(encoder) => {
                encoder.reset(k, m, shard)?;
                encoder
            }
            None => encoder.insert(ReedSolomonEncoder::new(k, m, shard)?),
        };
        for i in 0..k as u64 {
            let j = plan.data_index(g, i);
            let read = read_shard(src, plan, j, &mut buf)
                .with_context(|| format!("не удалось прочитать кусок {}", j))?;
            leaves[j as usize] = leaf_hash(&buf);
            encoder.add_original_shard(&buf)?;
            progress.add(read as u64);
        }
        let result = encoder.encode()?;
        for parity in result.recovery_iter() {
            leaves[parity_index] = leaf_hash(parity);
            out.write_all(parity)?;
            parity_index += 1;
        }
    }
    out.flush()?;
    drop(out);
    ensure!(
        parity_index as u64 == plan.leaves(),
        "внутренняя ошибка: parity-кусков получилось {} вместо {}",
        parity_index as u64 - plan.data_shards,
        plan.parity_shards
    );

    let trailer = Trailer {
        plan: plan.clone(),
        merkle_root: merkle_root(&leaves),
    };
    let bytes = trailer.to_bytes();
    file.seek(SeekFrom::Start(plan.trailer_offset()))?;
    file.write_all(&bytes)?;

    file.seek(SeekFrom::Start(plan.protected_len))?;
    let mut out = BufWriter::with_capacity(1 << 20, &mut *file);
    out.write_all(&bytes)?;
    for leaf in &leaves {
        out.write_all(leaf)?;
    }
    out.flush()?;
    Ok(trailer)
}

/// Итог проверки записи для восстановления.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// Сколько кусков данных.
    pub data_shards: u64,
    /// Сколько parity-кусков.
    pub parity_shards: u64,
    /// Номера повреждённых кусков данных.
    pub bad_data: Vec<u64>,
    /// Номера повреждённых parity-кусков (от нуля).
    pub bad_parity: Vec<u64>,
    /// Цела ли таблица хешей (сходится с корнем Меркла).
    pub table_ok: bool,
    /// Можно ли доверять номерам повреждённых кусков.
    pub localized: bool,
    /// Цела ли копия трейлера.
    pub trailer_copy_ok: bool,
    /// Цел ли конечный трейлер.
    pub trailer_end_ok: bool,
    /// Группы, где повреждений больше, чем parity-кусков.
    pub unrecoverable_groups: Vec<u64>,
}

impl VerifyReport {
    /// Нет ли вообще повреждений.
    pub fn is_clean(&self) -> bool {
        self.bad_data.is_empty()
            && self.bad_parity.is_empty()
            && self.table_ok
            && self.trailer_copy_ok
            && self.trailer_end_ok
    }

    /// Хватает ли parity, чтобы восстановить все повреждённые куски.
    pub fn recoverable(&self) -> bool {
        self.localized && self.unrecoverable_groups.is_empty()
    }
}

/// Читает ровно `buf.len()` байт; при ошибке заполняет нулями и возвращает `false`.
fn read_lenient(src: &dyn Source, buf: &mut [u8], offset: u64) -> bool {
    match src.read_exact_at(buf, offset) {
        Ok(()) => true,
        Err(_) => {
            buf.fill(0);
            false
        }
    }
}

/// Проверяет запись для восстановления по сырому (необрезанному) источнику.
pub fn verify(
    src: &dyn Source,
    trailer: &Trailer,
    show_progress: bool,
    sink: Option<&Arc<dyn ProgressSink>>,
) -> Result<VerifyReport> {
    let plan = &trailer.plan;
    let shard = plan.shard_size as usize;
    let leaves = plan.leaves() as usize;

    let check_trailer = |offset: u64| {
        let mut raw = [0u8; TRAILER_LEN as usize];
        read_lenient(src, &mut raw, offset)
            && Trailer::from_bytes(&raw).is_ok_and(|found| &found == trailer)
    };
    let trailer_copy_ok = check_trailer(plan.protected_len);
    let trailer_end_ok =
        src.len() == plan.trailer_offset() + TRAILER_LEN && check_trailer(plan.trailer_offset());

    let mut table_raw = vec![0u8; leaves * HASH_LEN as usize];
    let table_read = read_lenient(src, &mut table_raw, plan.table_offset());
    let table: Vec<[u8; 32]> = table_raw
        .chunks_exact(HASH_LEN as usize)
        .map(|chunk| chunk.try_into().unwrap())
        .collect();

    let total = plan.protected_len + plan.parity_shards * plan.shard_size;
    let mut progress = new_progress("проверка записи", total, show_progress, sink);
    let mut actual = vec![[0u8; 32]; leaves];
    let mut readable = vec![true; leaves];
    let mut buf = vec![0u8; shard];
    for j in 0..plan.data_shards {
        let ok = read_shard(src, plan, j, &mut buf).is_ok();
        if !ok {
            buf.fill(0);
        }
        readable[j as usize] = ok;
        actual[j as usize] = leaf_hash(&buf);
        progress.add(
            plan.shard_size
                .min(plan.protected_len - j * plan.shard_size),
        );
    }
    for t in 0..plan.parity_shards {
        let ok = read_lenient(src, &mut buf, plan.parity_offset() + t * plan.shard_size);
        let leaf = (plan.data_shards + t) as usize;
        readable[leaf] = ok;
        actual[leaf] = leaf_hash(&buf);
        progress.add(plan.shard_size);
    }
    progress.finish();

    let table_root_ok = table_read && merkle_root(&table) == trailer.merkle_root;
    let actual_root_ok = !readable.contains(&false) && merkle_root(&actual) == trailer.merkle_root;
    let (bad, table_ok, localized): (Vec<usize>, bool, bool) = if actual_root_ok {
        (Vec::new(), table == actual, true)
    } else {
        let bad = (0..leaves)
            .filter(|&i| !readable[i] || actual[i] != table[i])
            .collect();
        (bad, table_root_ok, table_root_ok)
    };

    let data_shards = plan.data_shards as usize;
    let mut damaged = vec![0u64; plan.groups as usize];
    let mut bad_data = Vec::new();
    let mut bad_parity = Vec::new();
    for i in bad {
        if i < data_shards {
            bad_data.push(i as u64);
            damaged[(i as u64 % plan.groups) as usize] += 1;
        } else {
            let t = (i - data_shards) as u64;
            bad_parity.push(t);
            damaged[plan.parity_group(t) as usize] += 1;
        }
    }
    let unrecoverable_groups = (0..plan.groups)
        .filter(|&g| damaged[g as usize] > plan.group_parity(g))
        .collect();

    Ok(VerifyReport {
        data_shards: plan.data_shards,
        parity_shards: plan.parity_shards,
        bad_data,
        bad_parity,
        table_ok,
        localized,
        trailer_copy_ok,
        trailer_end_ok,
        unrecoverable_groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::Archive;
    use crate::writer::{create, CreateOptions};
    use reed_solomon_simd::ReedSolomonDecoder;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("rr-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Псевдослучайные (плохо сжимаемые) байты без внешних зависимостей.
    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    fn make_archive(dir: &Path, percent: Option<u32>, password: Option<&str>) -> PathBuf {
        let input = dir.join("in");
        fs::create_dir_all(&input).unwrap();
        fs::write(input.join("noise.bin"), noise(200_000, 7)).unwrap();
        fs::write(input.join("text.txt"), "строка ".repeat(5000)).unwrap();
        let archive = dir.join("a.plz");
        let opts = CreateOptions {
            show_progress: false,
            recovery: percent,
            password: password.map(|p| zeroize::Zeroizing::new(p.to_string())),
            ..Default::default()
        };
        create(&archive, &[input], &opts).unwrap();
        archive
    }

    fn open_trailer(path: &Path) -> (Box<dyn Source>, Trailer) {
        let (raw, _) = crate::source::open_raw(path).unwrap();
        let trailer = locate(raw.as_ref()).unwrap().expect("нет записи");
        (raw, trailer)
    }

    #[test]
    fn plan_for_small_archive() {
        let plan = Plan::new(10_000, 5).unwrap();
        assert_eq!(plan.shard_size, 4096);
        assert_eq!(plan.data_shards, 3);
        assert_eq!(plan.groups, 1);
        assert_eq!(plan.parity_shards, 1);
        assert_eq!(plan.total_len(), Some(10_000 + 256 + 4 * 32 + 4096));
    }

    #[test]
    fn plan_invariants_hold_for_many_sizes() {
        for &len in &[1u64, 4095, 4096, 4097, 1 << 20, 300 << 20, 5 << 30, 1 << 40] {
            for &percent in &[1u32, 5, 33, 100] {
                let plan = Plan::new(len, percent).unwrap();
                assert_eq!(plan.shard_size % 4096, 0);
                assert!((MIN_SHARD_SIZE..=MAX_SHARD_SIZE).contains(&plan.shard_size));
                assert!(plan.data_shards * plan.shard_size >= len);
                assert!((plan.data_shards - 1) * plan.shard_size < len);
                let mut data = 0;
                let mut parity = 0;
                for g in 0..plan.groups.min(2000) {
                    let k = plan.group_data(g);
                    assert!((1..=MAX_GROUP_DATA).contains(&k));
                    assert_eq!(plan.parity_before(g), parity);
                    for t in parity..parity + plan.group_parity(g) {
                        assert_eq!(plan.parity_group(t), g);
                    }
                    assert!(plan.group_parity(g) * 100 >= k * u64::from(percent));
                    data += k;
                    parity += plan.group_parity(g);
                }
                if plan.groups <= 2000 {
                    assert_eq!(data, plan.data_shards);
                    assert_eq!(parity, plan.parity_shards);
                }
            }
        }
    }

    #[test]
    fn plan_rejects_bad_percent() {
        assert!(Plan::new(1000, 0).is_err());
        assert!(Plan::new(1000, 101).is_err());
        assert!(Plan::new(0, 5).is_err());
    }

    #[test]
    fn trailer_roundtrip_and_tamper() {
        let trailer = Trailer {
            plan: Plan::new(123_456, 7).unwrap(),
            merkle_root: [9u8; 32],
        };
        let bytes = trailer.to_bytes();
        assert_eq!(Trailer::from_bytes(&bytes).unwrap(), trailer);
        for at in [0usize, 4, 9, 20, 50, 100, 127] {
            let mut bad = bytes;
            bad[at] ^= 1;
            assert!(Trailer::from_bytes(&bad).is_err(), "байт {} не защищён", at);
        }
    }

    #[test]
    fn merkle_shape() {
        let a = leaf_hash(b"a");
        let b = leaf_hash(b"b");
        let c = leaf_hash(b"c");
        assert_eq!(merkle_root(&[a]), a);
        assert_eq!(merkle_root(&[a, b]), node_hash(&a, &b));
        assert_eq!(merkle_root(&[a, b, c]), node_hash(&node_hash(&a, &b), &c));
        assert_ne!(merkle_root(&[a, b]), merkle_root(&[b, a]));
        assert_ne!(node_hash(&a, &b), leaf_hash(&[a, b].concat()));
    }

    #[test]
    fn archive_with_recovery_opens_extracts_and_verifies() {
        let dir = temp_dir("roundtrip");
        let archive = make_archive(&dir, Some(10), None);
        let (raw, trailer) = open_trailer(&archive);
        assert_eq!(trailer.plan.total_len(), Some(raw.len()));
        assert_eq!(trailer.plan.percent, 10);

        let report = verify(raw.as_ref(), &trailer, false, None).unwrap();
        assert!(report.is_clean(), "{:?}", report);

        let mut ar = Archive::open(&archive).unwrap();
        assert_eq!(ar.recovery.as_ref(), Some(&trailer));
        assert_eq!(ar.data_len(), trailer.plan.protected_len);
        ar.test(false, 0, None).unwrap();
        let stats = ar
            .extract(&crate::reader::ExtractOptions {
                output: dir.join("out"),
                show_progress: false,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(stats.files, 2);
        assert_eq!(
            fs::read(dir.join("out/in/noise.bin")).unwrap(),
            noise(200_000, 7)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn protected_part_equals_archive_without_recovery() {
        let dir = temp_dir("same");
        let archive = make_archive(&dir, None, None);
        let original = fs::read(&archive).unwrap();
        add_to_file(&archive, 5, false, None).unwrap();
        let with_rr = fs::read(&archive).unwrap();
        assert_eq!(&with_rr[..original.len()], &original[..]);
        assert!(add_to_file(&archive, 5, false, None).is_err());
        assert_eq!(fs::read(&archive).unwrap(), with_rr);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn encrypted_archive_with_recovery() {
        let dir = temp_dir("enc");
        let archive = make_archive(&dir, Some(3), Some("пароль"));
        let mut ar = Archive::open_with(&archive, Some("пароль")).unwrap();
        assert!(ar.is_encrypted());
        assert!(ar.recovery.is_some());
        ar.test(false, 0, None).unwrap();
        // Проверка записи не требует пароля.
        let report = ar.verify_recovery(false).unwrap().unwrap();
        assert!(report.is_clean());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_byte_is_located() {
        let dir = temp_dir("corrupt");
        let archive = make_archive(&dir, Some(10), None);
        let (_, trailer) = open_trailer(&archive);
        let offset = 100_000u64;
        let mut bytes = fs::read(&archive).unwrap();
        bytes[offset as usize] ^= 0x5a;
        fs::write(&archive, &bytes).unwrap();

        let (raw, _) = open_trailer(&archive);
        let report = verify(raw.as_ref(), &trailer, false, None).unwrap();
        assert_eq!(report.bad_data, vec![offset / trailer.plan.shard_size]);
        assert!(report.bad_parity.is_empty());
        assert!(report.table_ok);
        assert!(report.recoverable());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_end_trailer_falls_back_to_copy() {
        let dir = temp_dir("trailer");
        let archive = make_archive(&dir, Some(5), None);
        let (_, trailer) = open_trailer(&archive);
        let mut bytes = fs::read(&archive).unwrap();
        let len = bytes.len();
        bytes[len - 40] ^= 0xff;
        fs::write(&archive, &bytes).unwrap();

        let mut ar = Archive::open(&archive).unwrap();
        assert_eq!(ar.recovery.as_ref(), Some(&trailer));
        ar.test(false, 0, None).unwrap();
        let report = ar.verify_recovery(false).unwrap().unwrap();
        assert!(report.trailer_copy_ok);
        assert!(!report.trailer_end_ok);
        assert!(report.bad_data.is_empty() && report.bad_parity.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parity_restores_lost_shards() {
        let dir = temp_dir("decode");
        let archive = make_archive(&dir, Some(10), None);
        let (raw, trailer) = open_trailer(&archive);
        let plan = &trailer.plan;
        assert_eq!(plan.groups, 1);
        let k = plan.group_data(0) as usize;
        let m = plan.group_parity(0) as usize;
        let shard = plan.shard_size as usize;
        assert!(m >= 2);

        // Теряем первые m кусков данных и восстанавливаем их по parity.
        let mut decoder = ReedSolomonDecoder::new(k, m, shard).unwrap();
        let mut buf = vec![0u8; shard];
        for j in m..k {
            read_shard(raw.as_ref(), plan, j as u64, &mut buf).unwrap();
            decoder.add_original_shard(j, &buf).unwrap();
        }
        for t in 0..m {
            raw.read_exact_at(&mut buf, plan.parity_offset() + (t * shard) as u64)
                .unwrap();
            decoder.add_recovery_shard(t, &buf).unwrap();
        }
        let result = decoder.decode().unwrap();
        for j in 0..m {
            read_shard(raw.as_ref(), plan, j as u64, &mut buf).unwrap();
            assert_eq!(
                result.restored_original(j).unwrap(),
                &buf[..],
                "кусок {}",
                j
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn interleaving_spreads_burst_across_groups() {
        let plan = Plan::new(300 << 20, 5).unwrap();
        assert!(plan.groups > 1);
        let burst: Vec<u64> = (1000..1000 + plan.groups)
            .map(|j| j % plan.groups)
            .collect();
        let mut seen = burst.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len() as u64, plan.groups);
    }

    #[test]
    fn volumes_with_recovery_are_rejected() {
        let dir = temp_dir("volumes");
        fs::write(dir.join("f.bin"), noise(10_000, 3)).unwrap();
        let opts = CreateOptions {
            show_progress: false,
            recovery: Some(5),
            volume_size: Some(64 * 1024),
            ..Default::default()
        };
        let err = create(&dir.join("v.plz"), &[dir.join("f.bin")], &opts).unwrap_err();
        assert!(format!("{:#}", err).contains("многотомных"));
        let _ = fs::remove_dir_all(&dir);
    }
}
