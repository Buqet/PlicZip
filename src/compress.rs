//! Слой сжатия: выбор метода по степени и потоковые обёртки.

use anyhow::{ensure, Context, Result};
use std::io::{self, Read, Write};

use crate::format::Method;

/// Степень сжатия по умолчанию.
pub const DEFAULT_LEVEL: i32 = 5;
/// Минимальная степень сжатия: хранение без сжатия.
pub const MIN_LEVEL: i32 = 0;
/// Максимальная степень сжатия Brotli.
pub const MAX_LEVEL: i32 = 11;

const BROTLI_LGWIN: u32 = 22;

/// Проверяет степень сжатия и возвращает её без молчаливого clamp.
pub fn validate_level(level: i32) -> Result<i32> {
    ensure!(
        (MIN_LEVEL..=MAX_LEVEL).contains(&level),
        "степень сжатия должна быть от {MIN_LEVEL} до {MAX_LEVEL}, получено {level}"
    );
    Ok(level)
}

/// Выбирает алгоритм по привычной шкале 0..11.
pub fn method_for_level(level: i32) -> Method {
    match level {
        0 => Method::Store,
        1..=4 => Method::Deflate,
        _ => Method::Brotli,
    }
}

/// Сжимает блок в памяти и не увеличивает его размер без необходимости.
pub fn compress_block(method: Method, level: i32, input: &[u8]) -> Result<(Vec<u8>, Method)> {
    validate_level(level)?;
    if method == Method::Store || input.is_empty() {
        return Ok((input.to_vec(), Method::Store));
    }
    let mut packed = Vec::new();
    compress_stream(method, level, &mut &input[..], &mut packed)?;
    if packed.len() < input.len() {
        Ok((packed, method))
    } else {
        Ok((input.to_vec(), Method::Store))
    }
}

/// Потоково сжимает данные, не удерживая весь вход в памяти.
pub fn compress_stream<R: Read, W: Write>(
    method: Method,
    level: i32,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    validate_level(level)?;
    match method {
        Method::Store => {
            io::copy(input, output).context("ошибка копирования данных")?;
        }
        Method::Deflate => {
            let mut encoder =
                flate2::write::DeflateEncoder::new(output, flate2::Compression::new(level as u32));
            io::copy(input, &mut encoder).context("ошибка сжатия deflate")?;
            encoder.finish().context("ошибка завершения deflate")?;
        }
        Method::Brotli => {
            let mut encoder =
                brotli::CompressorWriter::new(output, 64 * 1024, level as u32, BROTLI_LGWIN);
            io::copy(input, &mut encoder).context("ошибка сжатия brotli")?;
            encoder.flush().context("ошибка завершения brotli")?;
        }
        Method::Other => anyhow::bail!("неподдерживаемый метод сжатия"),
    }
    Ok(())
}

/// Создаёт потоковый декодер над сжатым блоком.
pub fn decompress_stream<'a>(
    method: Method,
    payload: Box<dyn Read + 'a>,
) -> Result<Box<dyn Read + 'a>> {
    match method {
        Method::Store => Ok(payload),
        Method::Deflate => Ok(Box::new(flate2::read::DeflateDecoder::new(payload))),
        Method::Brotli => Ok(Box::new(brotli::Decompressor::new(payload, 64 * 1024))),
        Method::Other => anyhow::bail!("неподдерживаемый метод сжатия"),
    }
}

/// Читающий адаптер, который одновременно считает BLAKE3 и размер входа.
pub struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
    bytes: u64,
}

impl<R> HashingReader<R> {
    /// Создаёт адаптер над произвольным читающим потоком.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: blake3::Hasher::new(),
            bytes: 0,
        }
    }

    /// Возвращает BLAKE3-хеш всех прочитанных байт и их количество.
    pub fn finish(self) -> ([u8; 32], u64) {
        (*self.hasher.finalize().as_bytes(), self.bytes)
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            self.hasher.update(&buf[..read]);
            self.bytes = self.bytes.saturating_add(read as u64);
        }
        Ok(read)
    }
}
