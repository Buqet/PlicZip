//! Формат контейнера PlicZip (`.plz`), версия 1.
//!
//! # Раскладка файла
//!
//! ```text
//! смещение  размер  содержимое
//! 0         4       магия "PLZ1"
//! 4         ...     блоки данных (сжатые, друг за другом)
//! X         N       индекс (CBOR)
//! EOF-44    8       длина индекса, u64 LE
//! EOF-36    32      BLAKE3 индекса
//! EOF-4     4       магия "PLZ1"
//! ```
//!
//! Индекс лежит в конце (как в 7z/zip): это позволяет писать архив потоком,
//! не зная заранее сжатых размеров. Футер фиксированной длины 44 байта даёт
//! чтение индекса за один seek и ловит обрезанные файлы: магия в начале и в
//! конце обязана совпасть, а хеш индекса защищает весь список файлов, поскольку
//! в нём лежат хеши содержимого.
//!
//! Всё числовое — little-endian. Пути внутри архива всегда относительные,
//! через `/`, нормализованные (см. [`crate::safety`]).

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;

use crate::crypto::{self, CryptoParams};
use crate::safety;
use crate::source::Source;

/// Сигнатура обычного (незашифрованного) контейнера.
pub const MAGIC: [u8; 4] = *b"PLZ1";

/// Сигнатура зашифрованного контейнера: сразу за ней лежат параметры KDF.
///
/// Отдельная сигнатура вместо флага — чтобы читатель узнавал про шифрование по
/// четырём байтам, не разбирая ничего больше. Иначе пришлось бы гадать, лежит
/// ли на смещении 4 данные блока или заголовок криптографии.
pub const MAGIC_ENCRYPTED: [u8; 4] = *b"PLZE";

/// Длина параметров шифрования после сигнатуры `PLZE`.
pub const CRYPTO_HEADER_LEN: u64 = 40;

/// Текущая версия формата. Индекс с большей версией отвергается целиком.
pub const FORMAT_VERSION: u8 = 2;

/// Минимальная версия, которую этот читатель ещё понимает.
pub const MIN_FORMAT_VERSION: u8 = 1;

/// Длина футера: `index_len(8) + index_hash(32) + magic(4)`.
pub const FOOTER_LEN: u64 = 44;

/// Минимальный валидный архив: магия + пустой индекс + футер.
pub const MIN_ARCHIVE_LEN: u64 = 4 + FOOTER_LEN;

/// Первое смещение, доступное для данных блока (незашифрованный архив).
pub const BLOCK_DATA_START: u64 = 4;

/// Первое смещение, доступное для данных блока (зашифрованный архив).
pub const BLOCK_DATA_START_ENCRYPTED: u64 = 4 + CRYPTO_HEADER_LEN;

/// Предохранитель от «индекса» на гигабайты — защита от вредоносного файла.
pub const MAX_INDEX_LEN: u64 = 256 * 1024 * 1024;

/// Тип элемента архива.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Обычный файл.
    File,
    /// Каталог (содержимое блока отсутствует).
    Dir,
    /// Символическая ссылка.
    Symlink,
    /// Тип, которого нет в этой версии читателя.
    #[serde(other)]
    Other,
}

/// Метод сжатия блока.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Без сжатия.
    Store,
    /// Raw deflate (без zlib-заголовка).
    Deflate,
    /// Brotli.
    Brotli,
    /// Метод, неизвестный этой версии читателя.
    #[serde(other)]
    Other,
}

impl Method {
    /// Человекочитаемое имя метода.
    pub fn name(self) -> &'static str {
        match self {
            Method::Store => "store",
            Method::Deflate => "deflate",
            Method::Brotli => "brotli",
            Method::Other => "unknown",
        }
    }
}

/// Один блок данных: непрерывный кусок файла-архива, который сжимается целиком.
///
/// В режиме `--solid` один блок содержит несколько файлов подряд; в обычном
/// режиме — ровно один файл. Раскладка блока на файлы описана в
/// [`PzEntry::block_offset`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PzBlock {
    /// Абсолютное смещение сжатых байт в файле-архиве.
    pub offset: u64,
    /// Сколько байт блок занимает на диске.
    pub stored_size: u64,
    /// Длина сжатых данных до шифрования; 0 — блок не зашифрован.
    #[serde(default)]
    pub payload_size: u64,
    /// Сколько байт блок разворачивается в памяти (сумма размеров файлов).
    pub size: u64,
    /// Метод сжатия.
    pub method: Method,
    /// Степень сжатия, с которой блок был создан (справочно).
    #[serde(default)]
    pub level: i8,
    /// BLAKE3 от несжатого содержимого блока.
    pub hash: [u8; 32],
}

/// Описание одного элемента архива.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PzEntry {
    /// Путь внутри архива: относительный, через `/`, нормализованный.
    pub path: String,
    /// Тип элемента.
    pub kind: Kind,
    /// Размер распакованных данных (0 для каталогов и ссылок).
    #[serde(default)]
    pub size: u64,
    /// BLAKE3 содержимого файла; для каталогов и ссылок — нули.
    pub hash: [u8; 32],
    /// Права доступа (биты unix; на Windows восстанавливается частично).
    #[serde(default)]
    pub mode: u32,
    /// Время изменения, секунды с UNIX-эпохи (0 — неизвестно).
    #[serde(default)]
    pub mtime: i64,
    /// Индекс блока с данными; `None` у каталогов, ссылок и пустых файлов.
    #[serde(default)]
    pub block: Option<u32>,
    /// Смещение данных файла внутри распакованного блока.
    #[serde(default)]
    pub block_offset: u64,
    /// Цель символической ссылки (только для [`Kind::Symlink`]).
    #[serde(default)]
    pub link: String,
}

impl PzEntry {
    /// Возвращает true, если у элемента есть данные в блоке.
    pub fn has_data(&self) -> bool {
        self.kind == Kind::File && self.size > 0 && self.block.is_some()
    }
}

/// Индекс архива: всё, что нужно для навигации, без самих данных.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PzIndex {
    /// Версия формата.
    #[serde(default = "default_version")]
    pub version: u8,
    /// Чем создан архив (справочно, не влияет на чтение).
    #[serde(default)]
    pub tool: String,
    /// Время создания, секунды с UNIX-эпохи.
    #[serde(default)]
    pub created_at: i64,
    /// Произвольный комментарий.
    #[serde(default)]
    pub comment: String,
    /// Был ли архив собран в solid-режиме.
    #[serde(default)]
    pub solid: bool,
    /// Блоки данных.
    #[serde(default)]
    pub blocks: Vec<PzBlock>,
    /// Элементы архива в порядке записи.
    #[serde(default)]
    pub entries: Vec<PzEntry>,
}

fn default_version() -> u8 {
    FORMAT_VERSION
}

impl Default for PzIndex {
    fn default() -> Self {
        PzIndex {
            version: FORMAT_VERSION,
            tool: String::new(),
            created_at: 0,
            comment: String::new(),
            solid: false,
            blocks: Vec::new(),
            entries: Vec::new(),
        }
    }
}

impl PzIndex {
    /// Суммарный размер содержимого до сжатия.
    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }

    /// Суммарный размер сжатых данных на диске.
    pub fn stored_size(&self) -> u64 {
        self.blocks.iter().map(|b| b.stored_size).sum()
    }

    /// Количество файлов, каталогов и ссылок.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut files = 0;
        let mut dirs = 0;
        let mut links = 0;
        for e in &self.entries {
            match e.kind {
                Kind::File => files += 1,
                Kind::Dir => dirs += 1,
                Kind::Symlink => links += 1,
                Kind::Other => {}
            }
        }
        (files, dirs, links)
    }
}

/// Разобранный футер и индекс.
#[derive(Debug)]
pub struct ParsedIndex {
    /// Индекс архива.
    pub index: PzIndex,
    /// Смещение индекса в файле.
    pub index_start: u64,
    /// Длина сериализованного индекса.
    pub index_len: u64,
    /// Хеш индекса из футера.
    pub index_hash: [u8; 32],
    /// Раскладка контейнера (шифрование, начало данных).
    pub layout: Layout,
}

/// Раскладка контейнера: определяется по сигнатуре, до разбора индекса.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Зашифрован ли архив.
    pub encrypted: bool,
    /// Параметры шифрования (есть только у зашифрованных архивов).
    pub crypto: Option<CryptoParams>,
    /// Смещение, с которого начинаются данные блоков.
    pub data_start: u64,
    /// Сигнатура, которая обязана стоять в конце файла.
    pub magic: [u8; 4],
}

/// Читает сигнатуру и параметры шифрования, ничего не расшифровывая.
///
/// Вызывается до запроса пароля: конверт открытый, иначе негде взять соль.
pub fn inspect(src: &dyn Source) -> Result<Layout> {
    let len = src.len();
    ensure!(
        len >= MIN_ARCHIVE_LEN,
        "файл слишком мал для архива PlicZip ({} байт)",
        len
    );

    let mut head = [0u8; 4];
    src.read_exact_at(&mut head, 0)
        .context("не удалось прочитать сигнатуру архива")?;

    match head {
        MAGIC => Ok(Layout {
            encrypted: false,
            crypto: None,
            data_start: BLOCK_DATA_START,
            magic: MAGIC,
        }),
        MAGIC_ENCRYPTED => {
            ensure!(
                len >= BLOCK_DATA_START_ENCRYPTED + FOOTER_LEN,
                "зашифрованный архив слишком мал ({} байт)",
                len
            );
            let mut raw = [0u8; CRYPTO_HEADER_LEN as usize];
            src.read_exact_at(&mut raw, 4)
                .context("не удалось прочитать параметры шифрования")?;
            let params = CryptoParams::from_bytes(&raw);
            params.kdf.validate()?;
            Ok(Layout {
                encrypted: true,
                crypto: Some(params),
                data_start: BLOCK_DATA_START_ENCRYPTED,
                magic: MAGIC_ENCRYPTED,
            })
        }
        _ => bail!("это не архив PlicZip: в начале файла нет сигнатуры PLZ1 или PLZE"),
    }
}

/// Читает и проверяет индекс архива, начиная с футера.
///
/// Проверяется: сигнатура в конце, границы индекса, его BLAKE3-хеш (по
/// шифротексту, если архив зашифрован), расшифровка и структурная целостность
/// (см. [`validate_index`]).
pub fn read_index(
    src: &dyn Source,
    layout: &Layout,
    key: Option<&[u8; 32]>,
) -> Result<ParsedIndex> {
    let len = src.len();

    let mut tail = [0u8; 4];
    src.read_exact_at(&mut tail, len - 4)?;
    ensure!(
        tail == layout.magic,
        "архив повреждён или обрезан: в конце файла нет нужной сигнатуры"
    );

    let footer_start = len - FOOTER_LEN;
    ensure!(
        footer_start >= layout.data_start,
        "в архиве нет места под индекс"
    );

    let mut footer = [0u8; FOOTER_LEN as usize];
    src.read_exact_at(&mut footer, footer_start)?;

    let index_len = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    let index_hash: [u8; 32] = footer[8..40].try_into().unwrap();

    ensure!(
        index_len <= MAX_INDEX_LEN,
        "индекс неправдоподобно большой: {} байт",
        index_len
    );
    ensure!(
        index_len <= footer_start - layout.data_start,
        "индекс выходит за границы файла"
    );

    let index_start = footer_start - index_len;
    let mut raw = vec![0u8; index_len as usize];
    src.read_exact_at(&mut raw, index_start)?;

    let actual = blake3::hash(&raw);
    ensure!(
        actual.as_bytes() == &index_hash,
        "хеш индекса не совпадает: архив повреждён"
    );

    let plain = if layout.encrypted {
        let key = key.ok_or_else(|| anyhow::anyhow!("архив зашифрован: нужен пароль"))?;
        let params = layout
            .crypto
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("в зашифрованном архиве нет параметров шифрования"))?;
        crypto::decrypt(
            key,
            &crypto::nonce_for_chunk(&params.nonce_base, crypto::INDEX_BLOCK, crypto::INDEX_CHUNK),
            crypto::INDEX_AAD,
            &raw,
        )?
    } else {
        raw
    };

    let index: PzIndex =
        ciborium::from_reader(plain.as_slice()).context("не удалось разобрать индекс архива")?;
    validate_index(&index, index_start, layout.data_start, layout.encrypted)?;

    Ok(ParsedIndex {
        index,
        index_start,
        index_len,
        index_hash,
        layout: layout.clone(),
    })
}

/// Структурная валидация индекса.
///
/// Здесь проверяется не только «нет мусора», но и инвариант раскладки: элементы
/// одного блока должны покрывать его целиком, без дыр и наложений. Именно это
/// позволяет распаковывать блок потоком, не доверяя смещениям из файла.
pub fn validate_index(
    index: &PzIndex,
    index_start: u64,
    data_start: u64,
    encrypted: bool,
) -> Result<()> {
    ensure!(
        (MIN_FORMAT_VERSION..=FORMAT_VERSION).contains(&index.version),
        "неподдерживаемая версия формата {} (этот читатель понимает {}..={})",
        index.version,
        MIN_FORMAT_VERSION,
        FORMAT_VERSION
    );

    for (i, block) in index.blocks.iter().enumerate() {
        ensure!(
            block.method != Method::Other,
            "блок {}: неизвестный метод сжатия",
            i
        );
        ensure!(block.size > 0, "блок {}: нулевой распакованный размер", i);
        ensure!(
            block.offset >= data_start,
            "блок {}: смещение {} указывает внутрь заголовка",
            i,
            block.offset
        );
        let end = block
            .offset
            .checked_add(block.stored_size)
            .ok_or_else(|| anyhow::anyhow!("блок {}: переполнение смещения", i))?;
        ensure!(
            end <= index_start,
            "блок {}: данные выходят за границу блока данных",
            i
        );
        if block.method == Method::Store && block.payload_size == 0 {
            ensure!(
                block.stored_size == block.size,
                "блок {}: метод store, но размеры {} и {} различаются",
                i,
                block.stored_size,
                block.size
            );
        }
        if encrypted {
            ensure!(
                block.payload_size > 0,
                "блок {}: в зашифрованном архиве нет длины открытых данных",
                i
            );
            ensure!(
                block.stored_size == crypto::sealed_len(block.payload_size),
                "блок {}: размер шифротекста {} не равен расчётному {} для {} байт",
                i,
                block.stored_size,
                crypto::sealed_len(block.payload_size),
                block.payload_size
            );
        } else {
            ensure!(
                block.payload_size == 0,
                "блок {}: длина открытых данных указана в незашифрованном архиве",
                i
            );
        }
    }

    let mut seen: HashSet<String> = HashSet::with_capacity(index.entries.len());
    for entry in &index.entries {
        ensure!(
            entry.kind != Kind::Other,
            "элемент {}: неизвестный тип",
            entry.path
        );
        safety::validate_rel_path(&entry.path)
            .with_context(|| format!("недопустимый путь в архиве: {:?}", entry.path))?;

        // Регистронезависимая проверка: архив должен распаковываться одинаково
        // на Windows, macOS и Linux, иначе `A.txt` и `a.txt` молча затрут друг друга.
        let key = entry.path.to_lowercase();
        ensure!(
            seen.insert(key),
            "повторяющийся путь в архиве: {}",
            entry.path
        );

        match entry.kind {
            Kind::File => {
                if entry.size == 0 {
                    ensure!(
                        entry.block.is_none(),
                        "файл {}: пустой файл не должен занимать блок",
                        entry.path
                    );
                } else {
                    let bi = entry.block.ok_or_else(|| {
                        anyhow::anyhow!("файл {}: нет ссылки на блок данных", entry.path)
                    })? as usize;
                    ensure!(
                        bi < index.blocks.len(),
                        "файл {}: ссылка на несуществующий блок {}",
                        entry.path,
                        bi
                    );
                    ensure!(
                        entry.block_offset.checked_add(entry.size).is_some(),
                        "файл {}: переполнение смещения",
                        entry.path
                    );
                    ensure!(
                        entry.block_offset + entry.size <= index.blocks[bi].size,
                        "файл {}: данные выходят за границу блока {}",
                        entry.path,
                        bi
                    );
                }
            }
            Kind::Dir | Kind::Symlink => {
                ensure!(
                    entry.block.is_none() && entry.size == 0,
                    "элемент {}: каталог или ссылка не могут содержать данные",
                    entry.path
                );
            }
            Kind::Other => {}
        }
    }

    // Проверяем, что элементы покрывают каждый блок ровно и по порядку.
    let mut cursor = vec![0u64; index.blocks.len()];
    for entry in &index.entries {
        if entry.kind != Kind::File || entry.size == 0 {
            continue;
        }
        let bi = entry.block.unwrap() as usize;
        ensure!(
            entry.block_offset == cursor[bi],
            "файл {}: разрыв или наложение в блоке {} (ожидалось смещение {}, указано {})",
            entry.path,
            bi,
            cursor[bi],
            entry.block_offset
        );
        cursor[bi] = entry.block_offset + entry.size;
    }
    for (i, block) in index.blocks.iter().enumerate() {
        ensure!(
            cursor[i] == block.size,
            "блок {}: покрыт не полностью ({} из {})",
            i,
            cursor[i],
            block.size
        );
    }

    Ok(())
}

/// Сериализует индекс в CBOR.
pub fn serialize_index(index: &PzIndex) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    ciborium::into_writer(index, &mut raw).context("не удалось сериализовать индекс")?;
    Ok(raw)
}

/// Пишет готовые байты индекса и футер.
///
/// Хеш считается по тому, что реально попало в файл: для зашифрованного
/// архива это шифротекст, поэтому подмена байтов индекса ловится до расшифровки.
pub fn write_index_and_footer<W: Write>(out: &mut W, raw: &[u8], magic: [u8; 4]) -> Result<()> {
    out.write_all(raw)?;
    let hash = blake3::hash(raw);
    out.write_all(&(raw.len() as u64).to_le_bytes())?;
    out.write_all(hash.as_bytes())?;
    out.write_all(&magic)?;
    Ok(())
}

/// Текущее время в секундах с UNIX-эпохи.
pub fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Строка вида `pliczip 0.1.0` для поля `tool`.
pub fn tool_string() -> String {
    format!("pliczip {}", env!("CARGO_PKG_VERSION"))
}

/// Расширение архива по умолчанию.
pub const DEFAULT_EXTENSION: &str = "plz";

/// Проверяет, что путь похож на архив PlicZip (только по расширению).
pub fn looks_like_archive(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(DEFAULT_EXTENSION))
        .unwrap_or(false)
}

/// Убеждается, что блок не пытается развернуться в большее число байт, чем заявлено.
///
/// Это эвристика против «бомб»: сжатый файл может разворачиваться в гигабайты,
/// записывая только нули. Порог не применяется к маленьким блокам — файл из
/// 200 КБ нулей честно сжимается в десятки байт, и ratio там огромный.
pub fn check_expansion(stored: u64, size: u64, max_ratio: u64) -> Result<()> {
    /// Ниже этого размера степень сжатия ни о чём не говорит.
    const SMALL_BLOCK: u64 = 1024 * 1024;

    if stored == 0 {
        bail!("блок нулевой длины не может содержать данные");
    }
    if max_ratio > 0 && size >= SMALL_BLOCK && size / stored.max(1) > max_ratio {
        bail!(
            "подозрительная степень сжатия: {} байт на диске разворачиваются в {} (лимит {}:1)",
            stored,
            size,
            max_ratio
        );
    }
    Ok(())
}
