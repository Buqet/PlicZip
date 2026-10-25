//! Шифрование архива.
//!
//! Ключ выводится из пароля через **Argon2id**, данные шифруются
//! **ChaCha20-Poly1305** (AEAD: подделка и порча ловятся криптографией, а не
//! только хешем). Шифруется каждый блок отдельно и индекс целиком.
//!
//! Схема nonce (12 байт): номер блока (4 байта) + номер куска внутри блока
//! (4 байта) + 4 случайных байта базы из конверта. Для индекса номер блока и
//! номер куска равны `u32::MAX`.
//! Один и тот же ключ никогда не встречает один и тот же nonce дважды, потому
//! что соль своя у каждого архива, а счётчик уникален внутри архива.
//!
//! AAD (дополнительные аутентифицируемые данные) связывает шифротекст с
//! метаданными: подменить метод сжатия или переставить блоки местами не выйдет.

use anyhow::{anyhow, bail, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::io::{IsTerminal, Read, Write};
use zeroize::Zeroizing;

/// Длина соли KDF.
pub const SALT_LEN: usize = 16;

/// Длина nonce для ChaCha20-Poly1305.
pub const NONCE_LEN: usize = 12;

/// Длина ключа.
pub const KEY_LEN: usize = 32;

/// Длина тега аутентификации, который добавляется к каждому шифротексту.
pub const TAG_LEN: usize = 16;

/// Размер порции открытого текста, которая шифруется за один раз.
///
/// AEAD работает над сообщением целиком, поэтому файл-гигабайт пришлось бы
/// держать в памяти. Нарезка на куски фиксированного размера это снимает:
/// шифрование идёт потоком, память ограничена одним куском.
pub const CHUNK_LEN: usize = 1024 * 1024;

/// Номер «блока» для индекса: блоки данных занимают остальные значения.
pub const INDEX_BLOCK: u32 = u32::MAX;

/// Номер «куска» для индекса.
pub const INDEX_CHUNK: u32 = u32::MAX;

/// Параметры вывода ключа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Стоимость по памяти в КиБ.
    pub m_cost: u32,
    /// Число проходов.
    pub t_cost: u32,
    /// Степень параллелизма.
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        // 64 МиБ и три прохода: около половины секунды на слабой машине,
        // зато перебор пароля дорогой.
        KdfParams {
            m_cost: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

impl KdfParams {
    /// Проверяет, что параметры не выходят за разумные рамки.
    ///
    /// Верхняя граница нужна, чтобы вредоносный архив не заставил выделить
    /// десятки гигабайт памяти ещё до проверки пароля.
    pub fn validate(&self) -> Result<()> {
        if self.m_cost < 8 || self.m_cost > 1024 * 1024 {
            bail!("недопустимая стоимость KDF по памяти: {} КиБ", self.m_cost);
        }
        if self.t_cost == 0 || self.t_cost > 32 {
            bail!("недопустимое число проходов KDF: {}", self.t_cost);
        }
        if self.p_cost == 0 || self.p_cost > 64 {
            bail!("недопустимый параллелизм KDF: {}", self.p_cost);
        }
        Ok(())
    }
}

/// Всё, что нужно для расшифровки: параметры KDF, соль и база nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CryptoParams {
    /// Параметры вывода ключа.
    pub kdf: KdfParams,
    /// Соль.
    pub salt: [u8; SALT_LEN],
    /// База для nonce: 8 случайных байт.
    pub nonce_base: [u8; NONCE_LEN],
}

impl CryptoParams {
    /// Создаёт параметры со случайной солью и базой nonce.
    pub fn generate() -> Result<Self> {
        Ok(CryptoParams {
            kdf: KdfParams::default(),
            salt: random_array()?,
            nonce_base: random_array()?,
        })
    }

    /// Сериализует параметры в 40 байт для конверта.
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut out = [0u8; 40];
        out[0..4].copy_from_slice(&self.kdf.m_cost.to_le_bytes());
        out[4..8].copy_from_slice(&self.kdf.t_cost.to_le_bytes());
        out[8..12].copy_from_slice(&self.kdf.p_cost.to_le_bytes());
        out[12..28].copy_from_slice(&self.salt);
        out[28..40].copy_from_slice(&self.nonce_base);
        out
    }

    /// Разбирает параметры из конверта.
    pub fn from_bytes(raw: &[u8; 40]) -> Self {
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&raw[12..28]);
        let mut nonce_base = [0u8; NONCE_LEN];
        nonce_base.copy_from_slice(&raw[28..40]);
        CryptoParams {
            kdf: KdfParams {
                m_cost: u32::from_le_bytes(raw[0..4].try_into().unwrap()),
                t_cost: u32::from_le_bytes(raw[4..8].try_into().unwrap()),
                p_cost: u32::from_le_bytes(raw[8..12].try_into().unwrap()),
            },
            salt,
            nonce_base,
        }
    }
}

/// Случайный массив нужной длины из системного источника энтропии.
pub fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf)
        .map_err(|err| anyhow!("нет доступа к источнику случайности: {}", err))?;
    Ok(buf)
}

/// Выводит ключ из пароля.
pub fn derive_key(
    password: &[u8],
    salt: &[u8],
    kdf: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    kdf.validate()?;
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|err| anyhow!("неверные параметры KDF: {}", err))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, &mut *key)
        .map_err(|err| anyhow!("не удалось вывести ключ: {}", err))?;
    Ok(key)
}

/// Собирает nonce из базы, номера блока и номера куска.
///
/// Пара (блок, кусок) уникальна внутри архива, а ключ уникален для архива
/// благодаря соли — поэтому один nonce не встречается дважды.
pub fn nonce_for_chunk(base: &[u8; NONCE_LEN], block: u32, chunk: u32) -> [u8; NONCE_LEN] {
    let mut nonce = *base;
    nonce[0..4].copy_from_slice(&block.to_le_bytes());
    nonce[4..8].copy_from_slice(&chunk.to_le_bytes());
    nonce
}

/// Сколько байт занимает шифротекст для `plain` байт открытого текста.
pub fn sealed_len(plain: u64) -> u64 {
    let chunks = plain.div_ceil(CHUNK_LEN as u64);
    plain + chunks * TAG_LEN as u64
}

fn cipher_for(key: &[u8; KEY_LEN]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(key))
}

/// Шифрует фрагмент, привязывая его к `aad`.
pub fn encrypt(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    cipher_for(key)
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| anyhow!("ошибка шифрования"))
}

/// Расшифровывает фрагмент. Неверный пароль и порча данных неотличимы —
/// так и должно быть, иначе это оракул для перебора.
pub fn decrypt(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    cipher_for(key)
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| anyhow!("неверный пароль или архив повреждён"))
}

/// Просит пароль для нового архива: два раза, чтобы не запереть себя снаружи.
pub fn new_password(reason: &str) -> Result<Zeroizing<String>> {
    if !std::io::stdin().is_terminal() {
        bail!("для создания зашифрованного архива задайте пароль через -p");
    }
    let first = rpassword::prompt_password(format!("{}: ", reason))
        .map_err(|err| anyhow!("не удалось прочитать пароль: {}", err))?;
    if first.is_empty() {
        bail!("пустой пароль недопустим");
    }
    let second = rpassword::prompt_password("Повторите пароль: ")
        .map_err(|err| anyhow!("не удалось прочитать пароль: {}", err))?;
    if first != second {
        bail!("пароли не совпадают");
    }
    Ok(Zeroizing::new(first))
}

/// Берёт пароль из флага `-p` или спрашивает интерактивно.
pub fn password(explicit: Option<&str>, reason: &str) -> Result<Zeroizing<String>> {
    match explicit {
        Some(value) => {
            if value.is_empty() {
                bail!("пустой пароль недопустим");
            }
            Ok(Zeroizing::new(value.to_string()))
        }
        None => {
            if !std::io::stdin().is_terminal() {
                bail!("архив зашифрован: передайте пароль через -p");
            }
            let value = rpassword::prompt_password(format!("{}: ", reason))
                .map_err(|err| anyhow!("не удалось прочитать пароль: {}", err))?;
            if value.is_empty() {
                bail!("пустой пароль недопустим");
            }
            Ok(Zeroizing::new(value))
        }
    }
}

/// AAD для куска: привязывает шифротекст к его месту в архиве.
pub fn chunk_aad(block: u32, chunk: u32) -> [u8; 8] {
    let mut aad = [0u8; 8];
    aad[0..4].copy_from_slice(&block.to_le_bytes());
    aad[4..8].copy_from_slice(&chunk.to_le_bytes());
    aad
}

/// AAD индекса.
pub const INDEX_AAD: &[u8] = b"pliczip-index";

fn to_io(err: anyhow::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
}

/// Потоковый шифратор: копит кусок и отдаёт шифротекст во внутренний писатель.
pub struct EncryptingWriter<W: Write> {
    inner: W,
    key: [u8; KEY_LEN],
    base: [u8; NONCE_LEN],
    block: u32,
    chunk: u32,
    buf: Vec<u8>,
    written: u64,
    plain: u64,
}

impl<W: Write> EncryptingWriter<W> {
    /// Оборачивает писатель, привязывая поток к номеру блока.
    pub fn new(inner: W, key: &[u8; KEY_LEN], base: &[u8; NONCE_LEN], block: u32) -> Self {
        EncryptingWriter {
            inner,
            key: *key,
            base: *base,
            block,
            chunk: 0,
            buf: Vec::with_capacity(CHUNK_LEN),
            written: 0,
            plain: 0,
        }
    }

    /// Сколько байт шифротекста уже записано.
    pub fn written(&self) -> u64 {
        self.written
    }

    fn flush_chunk(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let nonce = nonce_for_chunk(&self.base, self.block, self.chunk);
        let aad = chunk_aad(self.block, self.chunk);
        let sealed = encrypt(&self.key, &nonce, &aad, &self.buf).map_err(to_io)?;
        self.inner.write_all(&sealed)?;
        self.written += sealed.len() as u64;
        self.buf.clear();
        self.chunk += 1;
        Ok(())
    }

    /// Дописывает последний кусок и возвращает писатель, длину шифротекста
    /// и длину открытого текста.
    pub fn finish(mut self) -> std::io::Result<(W, u64, u64)> {
        self.flush_chunk()?;
        self.inner.flush()?;
        Ok((self.inner, self.written, self.plain))
    }
}

impl<W: Write> Write for EncryptingWriter<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let mut taken = 0usize;
        while taken < data.len() {
            let room = CHUNK_LEN - self.buf.len();
            let part = room.min(data.len() - taken);
            self.buf.extend_from_slice(&data[taken..taken + part]);
            taken += part;
            self.plain += part as u64;
            if self.buf.len() == CHUNK_LEN {
                self.flush_chunk()?;
            }
        }
        Ok(taken)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Кусок не сбрасываем: иначе на каждый flush приходился бы лишний тег.
        self.inner.flush()
    }
}

/// Потоковый расшифровщик: отдаёт открытый текст порциями.
pub struct DecryptingReader<R: Read> {
    inner: R,
    key: [u8; KEY_LEN],
    base: [u8; NONCE_LEN],
    block: u32,
    chunk: u32,
    remaining: u64,
    buf: Vec<u8>,
    pos: usize,
}

impl<R: Read> DecryptingReader<R> {
    /// Оборачивает читатель, зная длину открытого текста.
    pub fn new(
        inner: R,
        key: &[u8; KEY_LEN],
        base: &[u8; NONCE_LEN],
        block: u32,
        plain_len: u64,
    ) -> Self {
        DecryptingReader {
            inner,
            key: *key,
            base: *base,
            block,
            chunk: 0,
            remaining: plain_len,
            buf: Vec::new(),
            pos: 0,
        }
    }
}

impl<R: Read> Read for DecryptingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            if self.remaining == 0 || out.is_empty() {
                return Ok(0);
            }
            let take = (CHUNK_LEN as u64).min(self.remaining) as usize;
            let mut sealed = vec![0u8; take + TAG_LEN];
            self.inner.read_exact(&mut sealed)?;
            let nonce = nonce_for_chunk(&self.base, self.block, self.chunk);
            let aad = chunk_aad(self.block, self.chunk);
            self.buf = decrypt(&self.key, &nonce, &aad, &sealed).map_err(to_io)?;
            self.pos = 0;
            self.remaining -= take as u64;
            self.chunk += 1;
        }
        let take = (self.buf.len() - self.pos).min(out.len());
        out[..take].copy_from_slice(&self.buf[self.pos..self.pos + take]);
        self.pos += take;
        Ok(take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Быстрые параметры KDF, чтобы тесты не ждали полсекунды каждый.
    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 64,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn key_is_deterministic() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key("пароль".as_bytes(), &salt, &fast()).unwrap();
        let b = derive_key("пароль".as_bytes(), &salt, &fast()).unwrap();
        assert_eq!(*a, *b);
    }

    #[test]
    fn different_password_gives_different_key() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key("пароль".as_bytes(), &salt, &fast()).unwrap();
        let b = derive_key("другой".as_bytes(), &salt, &fast()).unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn different_salt_gives_different_key() {
        let a = derive_key("пароль".as_bytes(), &[1u8; SALT_LEN], &fast()).unwrap();
        let b = derive_key("пароль".as_bytes(), &[2u8; SALT_LEN], &fast()).unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn roundtrip() {
        let key = [0x42u8; KEY_LEN];
        let base = [0x11u8; NONCE_LEN];
        let data = "секретные данные архива".as_bytes();
        let ct = encrypt(&key, &nonce_for_chunk(&base, 0, 1), &chunk_aad(0, 1), data).unwrap();
        assert_ne!(&ct[..], data);
        assert_eq!(ct.len(), data.len() + TAG_LEN);
        let pt = decrypt(&key, &nonce_for_chunk(&base, 0, 1), &chunk_aad(0, 1), &ct).unwrap();
        assert_eq!(pt, data);
    }

    #[test]
    fn wrong_key_fails() {
        let base = [0x11u8; NONCE_LEN];
        let ct = encrypt(&[1u8; KEY_LEN], &nonce_for_chunk(&base, 0, 0), b"", b"data").unwrap();
        assert!(decrypt(&[2u8; KEY_LEN], &nonce_for_chunk(&base, 0, 0), b"", &ct).is_err());
    }

    #[test]
    fn tampering_fails() {
        let key = [9u8; KEY_LEN];
        let base = [3u8; NONCE_LEN];
        let nonce = nonce_for_chunk(&base, 0, 5);
        let aad = chunk_aad(0, 5);
        let mut ct = encrypt(&key, &nonce, &aad, "данные".as_bytes()).unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        assert!(decrypt(&key, &nonce, &aad, &ct).is_err());
    }

    #[test]
    fn wrong_aad_fails() {
        let key = [9u8; KEY_LEN];
        let base = [3u8; NONCE_LEN];
        let ct = encrypt(
            &key,
            &nonce_for_chunk(&base, 0, 5),
            &chunk_aad(0, 5),
            "данные".as_bytes(),
        )
        .unwrap();
        assert!(decrypt(&key, &nonce_for_chunk(&base, 0, 5), &chunk_aad(0, 6), &ct).is_err());
    }

    #[test]
    fn nonces_differ_by_position() {
        let base = [5u8; NONCE_LEN];
        assert_ne!(nonce_for_chunk(&base, 0, 0), nonce_for_chunk(&base, 0, 1));
        assert_ne!(nonce_for_chunk(&base, 0, 0), nonce_for_chunk(&base, 1, 0));
        assert_eq!(nonce_for_chunk(&base, 0, 0)[8..], base[8..]);
    }

    #[test]
    fn params_survive_roundtrip() {
        let params = CryptoParams {
            kdf: KdfParams {
                m_cost: 1234,
                t_cost: 2,
                p_cost: 3,
            },
            salt: [1u8; SALT_LEN],
            nonce_base: [2u8; NONCE_LEN],
        };
        let restored = CryptoParams::from_bytes(&params.to_bytes());
        assert_eq!(restored, params);
    }

    #[test]
    fn absurd_kdf_params_rejected() {
        let bad = KdfParams {
            m_cost: 8 * 1024 * 1024,
            t_cost: 1,
            p_cost: 1,
        };
        assert!(bad.validate().is_err());
        let zero = KdfParams {
            m_cost: 1024,
            t_cost: 0,
            p_cost: 1,
        };
        assert!(zero.validate().is_err());
    }

    #[test]
    fn random_is_not_constant() {
        let a = random_array::<16>().unwrap();
        let b = random_array::<16>().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn sealed_len_counts_tags() {
        assert_eq!(sealed_len(0), 0);
        assert_eq!(sealed_len(1), 1 + TAG_LEN as u64);
        assert_eq!(
            sealed_len(CHUNK_LEN as u64),
            CHUNK_LEN as u64 + TAG_LEN as u64
        );
        assert_eq!(
            sealed_len(CHUNK_LEN as u64 + 1),
            CHUNK_LEN as u64 + 1 + 2 * TAG_LEN as u64
        );
    }

    fn streaming_roundtrip(len: usize) {
        let key = [7u8; KEY_LEN];
        let base = [2u8; NONCE_LEN];
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

        let mut sealed = Vec::new();
        let writer = EncryptingWriter::new(&mut sealed, &key, &base, 1);
        let mut writer = writer;
        std::io::Write::write_all(&mut writer, &data).unwrap();
        let (_, written, plain) = writer.finish().unwrap();
        assert_eq!(written, sealed.len() as u64);
        assert_eq!(written, sealed_len(len as u64));
        assert_eq!(plain, len as u64);

        let reader = DecryptingReader::new(sealed.as_slice(), &key, &base, 1, len as u64);
        let mut back = Vec::new();
        std::io::Read::read_to_end(&mut Box::new(reader), &mut back).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn streaming_roundtrip_various_sizes() {
        for len in [
            0usize,
            1,
            1000,
            CHUNK_LEN - 1,
            CHUNK_LEN,
            CHUNK_LEN + 1,
            CHUNK_LEN * 2 + 7,
        ] {
            streaming_roundtrip(len);
        }
    }

    #[test]
    fn streaming_detects_tampering() {
        let key = [7u8; KEY_LEN];
        let base = [2u8; NONCE_LEN];
        let data = vec![0xABu8; CHUNK_LEN + 10];
        let mut sealed = Vec::new();
        let mut writer = EncryptingWriter::new(&mut sealed, &key, &base, 3);
        std::io::Write::write_all(&mut writer, &data).unwrap();
        writer.finish().unwrap();

        let victim = sealed.len() / 2;
        sealed[victim] ^= 0xFF;
        let mut reader =
            DecryptingReader::new(sealed.as_slice(), &key, &base, 3, data.len() as u64);
        let mut back = Vec::new();
        let outcome = std::io::Read::read_to_end(&mut reader, &mut back);
        assert!(outcome.is_err(), "порча куска должна ловиться");
    }
}
