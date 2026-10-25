//! Проверки безопасности: архивы, собранные вручную, чтобы обойти валидатор
//! писателя и подсунуть читателю заведомо опасную структуру.
//!
//! Каждый тест строит байты формата напрямую и убеждается, что `Archive::open`
//! отвергает файл — то есть защита живёт в читателе, а не только в писателе.

use pliczip::format::{self, Kind, Method, PzBlock, PzEntry, PzIndex};
use pliczip::reader::Archive;
use std::fs;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

const DATA: &[u8] = b"hello";

fn workdir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("sec-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn data_hash() -> [u8; 32] {
    *blake3::hash(DATA).as_bytes()
}

fn block() -> PzBlock {
    PzBlock {
        offset: 4,
        stored_size: DATA.len() as u64,
        payload_size: 0,
        size: DATA.len() as u64,
        method: Method::Store,
        level: 0,
        hash: data_hash(),
    }
}

fn file_entry(path: &str) -> PzEntry {
    PzEntry {
        path: path.to_string(),
        kind: Kind::File,
        size: DATA.len() as u64,
        hash: data_hash(),
        mode: 0o644,
        mtime: 0,
        block: Some(0),
        block_offset: 0,
        link: String::new(),
    }
}

fn index(entries: Vec<PzEntry>, blocks: Vec<PzBlock>) -> PzIndex {
    PzIndex {
        version: format::FORMAT_VERSION,
        tool: "security-test".to_string(),
        created_at: 0,
        comment: String::new(),
        solid: false,
        blocks,
        entries,
    }
}

/// Собирает архивный файл вручную, минуя проверки писателя.
fn craft(index: &PzIndex) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&format::MAGIC);
    out.extend_from_slice(DATA);
    let mut cursor = std::io::Cursor::new(&mut out);
    cursor.seek(SeekFrom::End(0)).unwrap();
    let raw = format::serialize_index(index).unwrap();
    format::write_index_and_footer(&mut cursor, &raw, format::MAGIC).unwrap();
    out
}

fn write_archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn open_error(tag: &str, index: &PzIndex) -> String {
    let dir = workdir(tag);
    let path = write_archive(&dir, "evil.plz", &craft(index));
    let err = Archive::open(&path)
        .err()
        .unwrap_or_else(|| panic!("архив {} должен быть отвергнут, но открылся", tag));
    let text = format!("{:#}", err);
    let _ = fs::remove_dir_all(&dir);
    text
}

#[test]
fn sane_archive_opens() {
    // Контрольный пример: без него остальные тесты ничего не доказывают.
    let dir = workdir("sane");
    let idx = index(vec![file_entry("ok.txt")], vec![block()]);
    let path = write_archive(&dir, "ok.plz", &craft(&idx));
    let archive = Archive::open(&path).expect("корректный архив должен открываться");
    assert_eq!(archive.entries().len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn traversal_path_rejected() {
    let err = open_error(
        "traversal",
        &index(vec![file_entry("../escape.txt")], vec![block()]),
    );
    assert!(
        err.contains("переход по каталогам") || err.contains("недопустимый путь"),
        "{}",
        err
    );
}

#[test]
fn deep_traversal_rejected() {
    let err = open_error(
        "deep",
        &index(vec![file_entry("a/b/../../../escape.txt")], vec![block()]),
    );
    assert!(err.contains("переход по каталогам"), "{}", err);
}

#[test]
fn absolute_path_rejected() {
    for (tag, bad) in [
        ("abs-unix", "/etc/passwd"),
        ("abs-win", "C:/Windows/system32/evil.dll"),
    ] {
        let err = open_error(tag, &index(vec![file_entry(bad)], vec![block()]));
        assert!(
            err.contains("абсолютный") || err.contains("недопустимый"),
            "{}: {}",
            bad,
            err
        );
    }
}

#[test]
fn ads_and_reserved_names_rejected() {
    for (tag, bad) in [
        ("ads", "file.txt:hidden"),
        ("reserved", "CON"),
        ("reserved-ext", "dir/NUL.txt"),
    ] {
        let err = open_error(tag, &index(vec![file_entry(bad)], vec![block()]));
        assert!(!err.is_empty(), "{} должен быть отвергнут", bad);
    }
}

#[test]
fn duplicate_paths_rejected() {
    // Регистр не должен давать двум файлам «слиться» в один на Windows.
    let err = open_error(
        "dupes",
        &index(
            vec![file_entry("Report.txt"), file_entry("report.txt")],
            vec![block()],
        ),
    );
    assert!(err.contains("повторяющийся путь"), "{}", err);
}

#[test]
fn gap_in_block_rejected() {
    let mut first = file_entry("a.bin");
    first.size = 2;
    first.block_offset = 0;
    let mut second = file_entry("b.bin");
    second.size = 2;
    second.block_offset = 3; // дыра на смещении 2

    let err = open_error("gap", &index(vec![first, second], vec![block()]));
    assert!(
        err.contains("разрыв") || err.contains("не полностью"),
        "{}",
        err
    );
}

#[test]
fn overlap_in_block_rejected() {
    let mut first = file_entry("a.bin");
    first.size = 4;
    first.block_offset = 0;
    let mut second = file_entry("b.bin");
    second.size = 1;
    second.block_offset = 0; // начинается там, где уже начался первый файл

    let err = open_error("overlap", &index(vec![first, second], vec![block()]));
    assert!(
        err.contains("разрыв") || err.contains("наложение"),
        "{}",
        err
    );
}

#[test]
fn file_beyond_block_rejected() {
    let mut entry = file_entry("big.bin");
    entry.size = 100; // больше, чем весь блок
    let err = open_error("beyond", &index(vec![entry], vec![block()]));
    assert!(err.contains("границу блока"), "{}", err);
}

#[test]
fn unknown_version_rejected() {
    let mut idx = index(vec![file_entry("ok.txt")], vec![block()]);
    idx.version = 3;
    let err = open_error("version", &idx);
    assert!(err.contains("версия"), "{}", err);
}

#[test]
fn zero_size_block_rejected() {
    let mut blk = block();
    blk.size = 0;
    blk.stored_size = 0;
    let err = open_error("zeroblock", &index(vec![], vec![blk]));
    assert!(err.contains("нулевой"), "{}", err);
}

#[test]
fn directory_with_data_rejected() {
    let mut entry = file_entry("dir");
    entry.kind = Kind::Dir;
    // Каталог не должен ссылаться на блок данных.
    entry.size = DATA.len() as u64;
    let err = open_error("dirblock", &index(vec![entry], vec![block()]));
    assert!(err.contains("не могут содержать данные"), "{}", err);
}

#[test]
fn tampered_index_hash_rejected() {
    let dir = workdir("indexhash");
    let idx = index(vec![file_entry("ok.txt")], vec![block()]);
    let mut bytes = craft(&idx);
    // Хеш индекса лежит в 32 байтах перед конечной магией.
    let pos = bytes.len() - 36;
    bytes[pos] ^= 0xFF;
    let path = write_archive(&dir, "bad.plz", &bytes);
    let err = Archive::open(&path).err().expect("должно быть отвергнуто");
    assert!(format!("{:#}", err).contains("хеш индекса"), "{:#}", err);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bad_magic_rejected() {
    let dir = workdir("magic");
    let mut bytes = craft(&index(vec![file_entry("ok.txt")], vec![block()]));
    bytes[0] = b'X';
    let path = write_archive(&dir, "bad.plz", &bytes);
    assert!(Archive::open(&path).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn symlink_escaping_is_skipped_not_fatal() {
    // Ссылка наружу не должна ронять распаковку и не должна создаваться.
    let dir = workdir("symlink");
    let mut link_entry = file_entry("link");
    link_entry.kind = Kind::Symlink;
    link_entry.size = 0;
    link_entry.block = None;
    link_entry.link = "../../../etc/passwd".to_string();

    let mut file = file_entry("ok.txt");
    file.block_offset = 0;

    let idx = index(vec![link_entry, file], vec![block()]);
    let path = write_archive(&dir, "links.plz", &craft(&idx));

    let mut archive = Archive::open(&path).expect("архив должен открываться");
    let out = dir.join("out");
    let stats = archive
        .extract(&pliczip::reader::ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .expect("безопасная ссылка не должна ломать распаковку");
    assert_eq!(stats.files, 1);
    assert_eq!(stats.skipped, 1, "опасная ссылка должна быть пропущена");
    assert!(!out.join("link").exists(), "ссылка наружу создана!");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn symlink_chain_escaping_is_skipped() {
    // `a/l1 -> ..` и `l3 -> a/l1/..` поодиночке безопасны, но вместе `l3`
    // указывает на каталог выше корня распаковки.
    let dir = workdir("symlink-chain");
    let mut l1 = file_entry("a/l1");
    l1.kind = Kind::Symlink;
    l1.size = 0;
    l1.block = None;
    l1.link = "..".to_string();
    let mut l3 = l1.clone();
    l3.path = "l3".to_string();
    l3.link = "a/l1/..".to_string();
    let file = file_entry("ok.txt");

    let idx = index(vec![l1, l3, file], vec![block()]);
    let path = write_archive(&dir, "chain.plz", &craft(&idx));

    let mut archive = Archive::open(&path).expect("архив должен открываться");
    let out = dir.join("out");
    let stats = archive
        .extract(&pliczip::reader::ExtractOptions {
            output: out.clone(),
            show_progress: false,
            ..Default::default()
        })
        .expect("распаковка не должна падать");
    assert_eq!(stats.files, 1);
    // На Windows без прав на ссылки a/l1 тоже может не создаться — это не ошибка.
    #[cfg(unix)]
    {
        assert_eq!(stats.links, 1, "безопасная a/l1 должна создаться");
        assert_eq!(stats.skipped, 1, "цепочка наружу должна быть пропущена");
    }
    assert!(stats.skipped >= 1, "цепочка наружу должна быть пропущена");
    assert!(
        fs::symlink_metadata(out.join("l3")).is_err(),
        "ссылка l3 наружу создана!"
    );

    let _ = fs::remove_dir_all(&dir);
}
