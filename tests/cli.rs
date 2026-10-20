//! Интеграционные тесты CLI: реальные процессы, реальные файлы.
//!
//! Проверяется то, что нельзя проверить на уровне библиотеки: коды возврата,
//! поведение команд и совпадение байт после распаковки.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_pliczip")
}

fn workdir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("cli-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(binary())
        .args(args)
        .output()
        .expect("не удалось запустить pliczip");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn make_tree(dir: &Path) -> PathBuf {
    let src = dir.join("данные");
    fs::create_dir_all(src.join("вложенный")).unwrap();
    fs::write(src.join("текст.txt"), "строка для сжатия ".repeat(300)).unwrap();
    fs::write(
        src.join("вложенный/бинарный.bin"),
        (0u8..=255).cycle().take(70_000).collect::<Vec<u8>>(),
    )
    .unwrap();
    fs::write(src.join("пустой.txt"), b"").unwrap();
    src
}

#[test]
fn missing_input_path_gives_clear_error() {
    let dir = workdir("missing");
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let missing = dir.join("нет-такого-каталога");
    let missing_stamp = missing.to_string_lossy().into_owned();

    let (ok, _, err) = run(&["a", &stamp, &missing_stamp]);
    assert!(!ok, "отсутствующий путь не должен считаться успехом");
    assert!(err.contains("путь не найден"), "невнятная ошибка: {}", err);
    assert!(
        !archive.exists(),
        "при ошибке не должно оставаться недособранного архива"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_list_test_extract() {
    let dir = workdir("roundtrip");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();

    let (ok, out, err) = run(&["a", &stamp, &src_stamp, "-x", "7", "-c", "тест"]);
    assert!(ok, "упаковка провалилась: {} {}", out, err);
    assert!(archive.exists());

    let (ok, out, _) = run(&["l", &stamp]);
    assert!(ok);
    assert!(out.contains("текст.txt"), "в списке нет файла: {}", out);
    assert!(
        out.contains("вложенный/бинарный.bin"),
        "нет вложенного файла: {}",
        out
    );

    let (ok, out, _) = run(&["i", &stamp]);
    assert!(ok);
    assert!(out.contains("тест"), "комментарий не сохранился: {}", out);
    assert!(out.contains("PlicZip v"));

    let (ok, out, err) = run(&["t", &stamp]);
    assert!(ok, "проверка провалилась: {} {}", out, err);
    assert!(out.contains("проверено"), "{}", out);

    let out_dir = dir.join("распаковано");
    let out_stamp = out_dir.to_string_lossy().into_owned();
    let (ok, _, err) = run(&["x", &stamp, "-o", &out_stamp]);
    assert!(ok, "распаковка провалилась: {}", err);

    for rel in ["текст.txt", "вложенный/бинарный.bin", "пустой.txt"] {
        let original = fs::read(src.join(rel)).unwrap();
        let restored = fs::read(out_dir.join("данные").join(rel)).unwrap();
        assert_eq!(original, restored, "не совпал {}", rel);
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_command_reports_corruption_with_exit_code_one() {
    let dir = workdir("corrupt");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();

    let (ok, _, err) = run(&["a", &stamp, &src_stamp, "--solid", "-q"]);
    assert!(ok, "{}", err);

    let mut bytes = fs::read(&archive).unwrap();
    bytes[10] ^= 0xFF;
    fs::write(&archive, &bytes).unwrap();

    let output = Command::new(binary()).args(["t", &stamp]).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "повреждённый архив должен давать код 1, получено {:?}",
        output.status
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extract_reports_corruption() {
    let dir = workdir("extract-corrupt");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    // Сжатие выключено, чтобы порча дошла до проверки хеша, а не утонула
    // в ошибке декодера: так проверяется именно верификация содержимого.
    assert!(run(&["a", &stamp, &src_stamp, "--solid", "-x", "0", "-q"]).0);

    let mut bytes = fs::read(&archive).unwrap();
    bytes[12] ^= 0xFF;
    fs::write(&archive, &bytes).unwrap();

    let out_dir = dir.join("out");
    let out_stamp = out_dir.to_string_lossy().into_owned();
    let (ok, _, err) = run(&["x", &stamp, "-o", &out_stamp]);
    assert!(!ok, "распаковка повреждённого архива не должна удаваться");
    assert!(
        err.contains("контрольная сумма") || err.contains("повреждён"),
        "непонятная ошибка: {}",
        err
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_compressed_block_is_detected() {
    let dir = workdir("extract-corrupt-compressed");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    assert!(run(&["a", &stamp, &src_stamp, "-x", "9", "-q"]).0);

    let mut bytes = fs::read(&archive).unwrap();
    bytes[20] ^= 0xFF;
    fs::write(&archive, &bytes).unwrap();

    let out_dir = dir.join("out");
    let out_stamp = out_dir.to_string_lossy().into_owned();
    let (ok, _, _) = run(&["x", &stamp, "-o", &out_stamp]);
    assert!(!ok, "порча сжатого блока должна ловиться");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn truncation_is_detected() {
    let dir = workdir("truncated");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    assert!(run(&["a", &stamp, &src_stamp, "-q"]).0);

    let bytes = fs::read(&archive).unwrap();
    fs::write(&archive, &bytes[..bytes.len() - 3]).unwrap();

    let (ok, _, err) = run(&["l", &stamp]);
    assert!(!ok);
    assert!(
        err.contains("повреждён") || err.contains("обрезан"),
        "{}",
        err
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn not_an_archive_is_rejected() {
    let dir = workdir("notarchive");
    let fake = dir.join("fake.plz");
    fs::write(&fake, "это точно не архив, а обычный текст".as_bytes()).unwrap();
    let (ok, _, err) = run(&["l", &fake.to_string_lossy()]);
    assert!(!ok);
    assert!(err.contains("PLZ1") || err.contains("сигнатуры"), "{}", err);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn skip_existing_keeps_original_file() {
    let dir = workdir("skip");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    assert!(run(&["a", &stamp, &src_stamp, "-q"]).0);

    let out_dir = dir.join("out");
    fs::create_dir_all(out_dir.join("данные")).unwrap();
    fs::write(out_dir.join("данные/текст.txt"), b"ORIGINAL").unwrap();
    let out_stamp = out_dir.to_string_lossy().into_owned();

    let (ok, _, err) = run(&["x", &stamp, "-o", &out_stamp, "--skip-existing", "-q"]);
    assert!(ok, "{}", err);
    assert_eq!(
        fs::read(out_dir.join("данные/текст.txt")).unwrap(),
        b"ORIGINAL",
        "--skip-existing не должен перезаписывать"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn store_level_produces_larger_but_valid_archive() {
    let dir = workdir("store");
    let src = make_tree(&dir);
    let archive = dir.join("out.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    assert!(run(&["a", &stamp, &src_stamp, "-x", "0", "-q"]).0);

    let (ok, out, err) = run(&["t", &stamp]);
    assert!(ok, "{} {}", out, err);

    let (ok, out, _) = run(&["l", &stamp, "-l"]);
    assert!(ok);
    assert!(
        out.contains("store"),
        "при -x0 всё должно быть store: {}",
        out
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_level_roundtrips() {
    let dir = workdir("levels");
    let src = make_tree(&dir);
    let src_stamp = src.to_string_lossy().into_owned();

    for level in ["0", "1", "4", "5", "9", "11"] {
        let archive = dir.join(format!("l{}.plz", level));
        let stamp = archive.to_string_lossy().into_owned();
        let (ok, _, err) = run(&["a", &stamp, &src_stamp, "-x", level, "-q"]);
        assert!(ok, "степень {}: {}", level, err);

        let (ok, out, err) = run(&["t", &stamp, "-q"]);
        assert!(ok, "степень {}: {} {}", level, out, err);
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encrypted_archive_requires_password() {
    let dir = workdir("encrypted");
    let src = make_tree(&dir);
    let archive = dir.join("secret.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();

    let (ok, _, err) = run(&["a", &stamp, &src_stamp, "--password=секрет", "-q"]);
    assert!(ok, "упаковка с паролем: {}", err);

    // Без пароля команда обязана отказаться (stdin не терминал).
    let (ok, _, err) = run(&["t", &stamp, "-q"]);
    assert!(!ok);
    assert!(err.contains("пароль"), "{}", err);

    // Неверный пароль.
    let (ok, _, _) = run(&["t", &stamp, "--password=неверный", "-q"]);
    assert!(!ok, "неверный пароль не должен подходить");

    // Верный пароль.
    let (ok, _, err) = run(&["t", &stamp, "--password=секрет", "-q"]);
    assert!(ok, "верный пароль должен подходить: {}", err);

    // В сведениях видно, чем зашифровано.
    let (ok, out, _) = run(&["i", &stamp, "--password=секрет"]);
    assert!(ok);
    assert!(out.contains("ChaCha20"), "{}", out);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn volumes_are_written_and_read() {
    let dir = workdir("volumes");
    let src = make_tree(&dir);
    let archive = dir.join("vol.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();

    // Без сжатия, чтобы архив точно превысил размер тома.
    let (ok, _, err) = run(&[
        "a",
        &stamp,
        &src_stamp,
        "-x",
        "0",
        "--volume-size",
        "64K",
        "-q",
    ]);
    assert!(ok, "{}", err);
    assert!(dir.join("vol.plz.001").exists(), "нет первого тома");
    assert!(dir.join("vol.plz.002").exists(), "нет второго тома");
    assert!(!archive.exists(), "одиночного файла быть не должно");

    let out_dir = dir.join("out");
    let out_stamp = out_dir.to_string_lossy().into_owned();
    let (ok, _, err) = run(&["x", &stamp, "-o", &out_stamp, "-q"]);
    assert!(ok, "распаковка томов: {}", err);
    assert_eq!(
        fs::read(out_dir.join("данные/вложенный/бинарный.bin")).unwrap(),
        fs::read(src.join("вложенный/бинарный.bin")).unwrap()
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exclude_filter_is_applied() {
    let dir = workdir("exclude");
    let src = make_tree(&dir);
    fs::write(src.join("мусор.tmp"), b"temp").unwrap();
    let archive = dir.join("filtered.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();

    let (ok, _, err) = run(&["a", &stamp, &src_stamp, "--exclude", "**/*.tmp", "-q"]);
    assert!(ok, "{}", err);

    let (ok, out, _) = run(&["l", &stamp]);
    assert!(ok);
    assert!(
        !out.contains("мусор.tmp"),
        "исключённый файл попал в архив: {}",
        out
    );
    assert!(out.contains("текст.txt"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_listing_is_json() {
    let dir = workdir("json");
    let src = make_tree(&dir);
    let archive = dir.join("a.plz");
    let stamp = archive.to_string_lossy().into_owned();
    let src_stamp = src.to_string_lossy().into_owned();
    assert!(run(&["a", &stamp, &src_stamp, "-q"]).0);

    let (ok, out, err) = run(&["l", &stamp, "--json"]);
    assert!(ok, "{}", err);
    assert!(out.trim_start().starts_with('{'), "не JSON: {}", out);
    assert!(out.contains("\"entries\""));
    assert!(out.contains("текст.txt"));

    let (ok, out, _) = run(&["i", &stamp, "--json"]);
    assert!(ok, "{}", out);
    assert!(out.contains("\"version\""));

    let (ok, out, _) = run(&["t", &stamp, "--json"]);
    assert!(ok, "{}", out);
    assert!(out.contains("\"ok\": true"), "{}", out);

    let _ = fs::remove_dir_all(&dir);
}
