//! Слой сжатия: выбор метода по степени и потоковые обёртки.
//!
//! Используются только чистые Rust-реализации (без C-зависимостей), чтобы
//! архив собирался на любой машине с rustc — включая пути с кириллицей, где
//! MSVC `cl.exe` падает на записи отладочной информации.

use anyhow::{ensure, Context, Result};
use std::io::{self, Read, Write};

use crate::format::Method;

/// Степень сжатия по умолчанию (brotli q5).
pub const DEFAULT_LEVEL: i32 = 5;

/// Минимальная степень (0 — без сжатия).
pub const MIN_LEVEL: i32 = 0;

/// Максимальная степень (brotli q11).
pub const MAX_LEVEL: i32 = 11;

/// Окно brotli по умолчанию для крупных блоков (2^22 = 4 МиБ).
const BROTLI_LGWIN: i32 = 22;

/// Подбирает окно brotli под размер входных данных.
///
/// Держать окно 4 МиБ для файла в 8 КБ — значит выделять память впустую на
/// каждом блоке: на архиве из тысяч мелких файлов это разница в разы.
fn brotli_window(size_hint: usize) -> i32 {
    if size_hint == 0 {
        // Размер неизвестен (потоковая запись) — считаем файл крупным.
        return BROTLI_LGWIN;
    }
    let bits = (usize::BITS - size_hint.leading_zeros()) as i32;
    (bits + 1).clamp(10, BROTLI_LGWIN)
}

/// Определяет метод сжатия по степени.
///
/// * `-mx0` — без сжатия;
/// * `-mx1..4` — deflate (быстро, совместимо по духу с zip);
/// * `-mx5..11` — brotli (лучше сжимает текст, степень = качество).
pub fn method_for_level(level: i32) -> Method {
    if level <= 0 {
        Method::Store
    } else if level <= 4 {
        Method::Deflate
    } else {
        Method::Brotli
    }
}

/// Переводит общую степень в уровень deflate (1..9).
pub fn deflate_level(level: i32) -> u32 {
    if level <= 0 {
        return 0;
    }
    (1 + (level - 1) * 2).clamp(1, 9) as u32
}

/// Переводит общую степень в качество brotli (0..11).
pub fn brotli_quality(level: i32) -> i32 {
    level.clamp(0, 11)
}

fn brotli_params(level: i32, size_hint: usize) -> brotli::enc::BrotliEncoderParams {
    brotli::enc::BrotliEncoderParams {
        quality: brotli_quality(level),
        lgwin: brotli_window(size_hint),
        size_hint,
        ..Default::default()
    }
}

/// Сжимает буфер целиком (для блоков, которые помещаются в память).
pub fn compress(method: Method, level: i32, data: &[u8]) -> Result<Vec<u8>> {
    match method {
        Method::Store => Ok(data.to_vec()),
        Method::Deflate => {
            let mut enc = flate2::write::DeflateEncoder::new(
                Vec::new(),
                flate2::Compression::new(deflate_level(level)),
            );
            enc.write_all(data).context("ошибка deflate-сжатия")?;
            enc.finish().context("ошибка deflate-сжатия")
        }
        Method::Brotli => {
            let mut out = Vec::new();
            let params = brotli_params(level, data.len());
            brotli::BrotliCompress(&mut io::Cursor::new(data), &mut out, &params)
                .context("ошибка brotli-сжатия")?;
            Ok(out)
        }
        Method::Other => anyhow::bail!("неизвестный метод сжатия"),
    }
}

/// Сжимает блок, откатываясь на `store`, если сжатие не дало выигрыша.
///
/// Возвращает данные и фактически использованный метод. Благодаря этому
/// архивы с JPEG/MP4 не раздуваются на доли процента.
pub fn compress_block(method: Method, level: i32, data: &[u8]) -> Result<(Vec<u8>, Method)> {
    if method == Method::Store || data.is_empty() {
        return Ok((data.to_vec(), Method::Store));
    }
    let packed = compress(method, level, data)?;
    if packed.len() >= data.len() {
        Ok((data.to_vec(), Method::Store))
    } else {
        Ok((packed, method))
    }
}

/// Сжимает поток в поток, не поднимая файл в память.
pub fn compress_stream<R: Read, W: Write>(
    method: Method,
    level: i32,
    reader: &mut R,
    out: &mut W,
) -> Result<()> {
    match method {
        Method::Store => {
            io::copy(reader, out).context("ошибка копирования в архив")?;
            Ok(())
        }
        Method::Deflate => {
            let mut enc = flate2::read::DeflateEncoder::new(
                reader,
                flate2::Compression::new(deflate_level(level)),
            );
            io::copy(&mut enc, out).context("ошибка deflate-сжатия")?;
            Ok(())
        }
        Method::Brotli => {
            let params = brotli_params(level, 0);
            brotli::BrotliCompress(reader, out, &params).context("ошибка brotli-сжатия")?;
            Ok(())
        }
        Method::Other => anyhow::bail!("неизвестный метод сжатия"),
    }
}

/// Открывает поток распаковки поверх сжатых байт.
pub fn decompress_reader<'a>(method: Method, stored: &'a [u8]) -> Result<Box<dyn Read + 'a>> {
    decompress_stream(method, stored)
}

/// Открывает поток распаковки поверх произвольного читателя.
///
/// Нужен, чтобы распаковывать блок прямо из файла-архива, не поднимая его в
/// память: один блок может быть больше оперативной памяти.
pub fn decompress_stream<'a, R: Read + 'a>(
    method: Method,
    reader: R,
) -> Result<Box<dyn Read + 'a>> {
    match method {
        Method::Store => Ok(Box::new(reader)),
        Method::Deflate => Ok(Box::new(flate2::read::DeflateDecoder::new(reader))),
        Method::Brotli => Ok(Box::new(brotli::Decompressor::new(reader, 64 * 1024))),
        Method::Other => anyhow::bail!("неизвестный метод сжатия"),
    }
}

/// Распаковывает буфер и проверяет, что длина совпала с заявленной.
pub fn decompress_exact(method: Method, stored: &[u8], expected: u64) -> Result<Vec<u8>> {
    let reader = decompress_reader(method, stored)?;
    let mut out = Vec::with_capacity(expected.min(8 * 1024 * 1024) as usize);
    let read = reader
        .take(expected.saturating_add(1))
        .read_to_end(&mut out)
        .context("ошибка распаковки")?;
    ensure!(
        read as u64 == expected,
        "размер после распаковки не совпал: ожидалось {} байт, получено {}",
        expected,
        read
    );
    Ok(out)
}

/// Читатель, который попутно считает BLAKE3 и общее число байт.
///
/// Позволяет хешировать содержимое файла, не читая его дважды: данные идут
/// прямо в компрессор, а хеш считается по пути.
pub struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
    total: u64,
}

impl<R: Read> HashingReader<R> {
    /// Оборачивает читатель.
    pub fn new(inner: R) -> Self {
        HashingReader {
            inner,
            hasher: blake3::Hasher::new(),
            total: 0,
        }
    }

    /// Возвращает хеш и число прочитанных байт.
    pub fn finish(self) -> ([u8; 32], u64) {
        (*self.hasher.finalize().as_bytes(), self.total)
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.hasher.update(&buf[..n]);
            self.total += n as u64;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        // Повторяющийся текст хорошо сжимается.
        "PlicZip собирает файлы в один контейнер. "
            .repeat(200)
            .into_bytes()
    }

    #[test]
    fn level_mapping() {
        assert_eq!(method_for_level(0), Method::Store);
        assert_eq!(method_for_level(1), Method::Deflate);
        assert_eq!(method_for_level(4), Method::Deflate);
        assert_eq!(method_for_level(5), Method::Brotli);
        assert_eq!(method_for_level(11), Method::Brotli);
    }

    #[test]
    fn roundtrip_all_methods() {
        let data = sample();
        for method in [Method::Store, Method::Deflate, Method::Brotli] {
            for level in [1, 5, 9] {
                if method == Method::Store && level != 1 {
                    continue;
                }
                let packed = compress(method, level, &data).unwrap();
                let back = decompress_exact(method, &packed, data.len() as u64).unwrap();
                assert_eq!(back, data, "метод {:?} степень {}", method, level);
            }
        }
    }

    #[test]
    fn compresses_repetitive_data() {
        let data = sample();
        for method in [Method::Deflate, Method::Brotli] {
            let packed = compress(method, 6, &data).unwrap();
            assert!(
                packed.len() < data.len() / 4,
                "{:?}: {} из {} — слишком слабое сжатие",
                method,
                packed.len(),
                data.len()
            );
        }
    }

    #[test]
    fn falls_back_to_store_when_no_gain() {
        // Псевдослучайные байты не сжимаются.
        let mut seed = 0x1234_5678u32;
        let data: Vec<u8> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        let (packed, used) = compress_block(Method::Brotli, 9, &data).unwrap();
        assert_eq!(used, Method::Store);
        assert_eq!(packed, data);
    }

    #[test]
    fn detects_wrong_expected_size() {
        let data = sample();
        let packed = compress(Method::Brotli, 5, &data).unwrap();
        assert!(decompress_exact(Method::Brotli, &packed, data.len() as u64 + 1).is_err());
    }

    #[test]
    fn hashing_reader_matches_direct_hash() {
        let data = sample();
        let mut reader = HashingReader::new(data.as_slice());
        let mut sink = Vec::new();
        io::copy(&mut reader, &mut sink).unwrap();
        let (hash, total) = reader.finish();
        assert_eq!(total, data.len() as u64);
        assert_eq!(hash, *blake3::hash(&data).as_bytes());
        assert_eq!(sink, data);
    }

    #[test]
    fn stream_roundtrip() {
        let data = sample();
        for method in [Method::Store, Method::Deflate, Method::Brotli] {
            let mut packed = Vec::new();
            compress_stream(method, 6, &mut data.as_slice(), &mut packed).unwrap();
            let back = decompress_exact(method, &packed, data.len() as u64).unwrap();
            assert_eq!(back, data);
        }
    }

    #[test]
    fn brotli_window_scales_with_input() {
        assert_eq!(brotli_window(8 * 1024), 15, "8 КБ — небольшое окно");
        assert_eq!(brotli_window(1), 10);
        assert_eq!(
            brotli_window(0),
            BROTLI_LGWIN,
            "неизвестный размер — максимум"
        );
        assert_eq!(brotli_window(512 * 1024 * 1024), BROTLI_LGWIN);
        // Окно не должно быть меньше самого входа, иначе теряется сжатие.
        for size in [1usize, 100, 4096, 65_536, 1 << 20] {
            assert!((1usize << brotli_window(size)) >= size.min(1 << BROTLI_LGWIN));
        }
    }
}
