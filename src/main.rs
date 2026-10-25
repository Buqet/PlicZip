//! Интерфейс командной строки PlicZip.
//!
//! Команды повторяют привычные сокращения 7-Zip: `a` (add), `x` (extract),
//! `l` (list), `t` (test), `i` (info).
//!
//! Пароль можно задать тремя способами, и все три не оставляют его в истории
//! команд: интерактивный запрос (`-p` без значения), файл (`--password-file`)
//! и переменная окружения `PLICZIP_PASSWORD`. Явное `--password=…`
//! поддерживается, но только через знак равенства — иначе `-p backup.plz`
//! съело бы имя архива как пароль.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use pliczip::prelude::*;
use pliczip::util::{
    format_unix_time, human_bytes, human_duration, pad_left, pad_right, plural, ratio,
    saved_percent,
};
use pliczip::{crypto, reader, util, writer};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use zeroize::Zeroizing;

/// Степень сжатия по умолчанию.
const DEFAULT_LEVEL: i32 = 5;

/// Переменная окружения с паролем — удобно для скриптов.
const PASSWORD_ENV: &str = "PLICZIP_PASSWORD";

#[derive(Parser, Debug)]
#[command(
    name = "pliczip",
    version,
    about = "PlicZip — архиватор с собственным форматом .plz",
    long_about = "PlicZip упаковывает файлы и каталоги в контейнер .plz.\n\
                  Внутри: индекс в конце файла, сжатие deflate/brotli, BLAKE3\n\
                  на каждый файл и блок, шифрование паролем и многотомные архивы.",
    disable_help_subcommand = true,
    after_help = "Пароль не обязательно указывать в команде:\n  \
                  pliczip a -p арх.plz доки            спросит пароль дважды\n  \
                  pliczip a --password=секрет арх.plz  явно, но видно в истории\n  \
                  pliczip a --password-file pw.txt арх.plz доки\n\n\
                  Примеры:\n  \
                  pliczip a backup.plz ./фото           упаковать каталог\n  \
                  pliczip a -x9 арх.plz a.txt b.txt     максимальное сжатие\n  \
                  pliczip a --volume-size 100M арх.plz  разбить на тома\n  \
                  pliczip l арх.plz -l                  подробный список\n  \
                  pliczip x арх.plz -o ./куда           распаковать\n  \
                  pliczip t арх.plz                     проверить целостность"
)]
struct Cli {
    /// Не показывать прогресс и сводки.
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Не включать UTF-8 в консоли Windows (если мешает).
    #[arg(long, global = true, hide = true)]
    no_utf8: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Создать архив из файлов и каталогов.
    #[command(alias = "a")]
    Add {
        /// Путь к создаваемому архиву.
        #[arg(value_name = "АРХИВ")]
        archive: PathBuf,

        /// Что упаковать (файлы и каталоги).
        #[arg(value_name = "ПУТИ", required = true)]
        inputs: Vec<PathBuf>,

        /// Степень сжатия: 0 — без сжатия, 1..4 — deflate, 5..11 — brotli.
        #[arg(short = 'x', long = "level", value_name = "N", default_value_t = DEFAULT_LEVEL, value_parser = clap::value_parser!(i32).range(0..=11))]
        level: i32,

        /// Склеивать мелкие файлы в общие solid-блоки.
        #[arg(short, long)]
        solid: bool,

        /// Целевой размер solid-блока.
        #[arg(short = 'b', long = "block-size", value_name = "РАЗМЕР", default_value = "8M", value_parser = parse_size)]
        block_size: u64,

        /// Комментарий, который сохранится в архиве.
        #[arg(short = 'c', long, value_name = "ТЕКСТ", default_value = "")]
        comment: String,

        /// Раскрывать символические ссылки вместо их сохранения.
        #[arg(long)]
        follow_links: bool,

        /// Зашифровать паролем. Без значения пароль спрашивается интерактивно.
        #[arg(short = 'p', long, value_name = "ПАРОЛЬ", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        password: Option<String>,

        /// Взять пароль из первой строки файла.
        #[arg(long = "password-file", value_name = "ПУТЬ")]
        password_file: Option<PathBuf>,

        /// Размер тома: архив разобьётся на файлы .001, .002, …
        #[arg(long = "volume-size", value_name = "РАЗМЕР", value_parser = parse_size)]
        volume_size: Option<u64>,

        /// Включить только эти маски (можно повторять).
        #[arg(long, value_name = "МАСКА")]
        include: Vec<String>,

        /// Исключить эти маски (можно повторять).
        #[arg(long, value_name = "МАСКА")]
        exclude: Vec<String>,

        /// Вывести машинночитаемый JSON.
        #[arg(long)]
        json: bool,
    },

    /// Распаковать архив.
    #[command(alias = "x")]
    Extract {
        /// Путь к архиву.
        #[arg(value_name = "АРХИВ")]
        archive: PathBuf,

        /// Куда распаковывать.
        #[arg(short = 'o', long, value_name = "КАТАЛОГ", default_value = ".")]
        output: PathBuf,

        /// Пароль от зашифрованного архива.
        #[arg(short = 'p', long, value_name = "ПАРОЛЬ", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        password: Option<String>,

        /// Взять пароль из первой строки файла.
        #[arg(long = "password-file", value_name = "ПУТЬ")]
        password_file: Option<PathBuf>,

        /// Не перезаписывать уже существующие файлы.
        #[arg(long)]
        skip_existing: bool,

        /// Не проверять контрольные суммы.
        #[arg(long)]
        no_verify: bool,

        /// Не восстанавливать символические ссылки.
        #[arg(long)]
        no_symlinks: bool,

        /// Не восстанавливать время изменения.
        #[arg(long)]
        no_mtime: bool,

        /// Максимально допустимая степень сжатия блока (0 — без ограничения).
        #[arg(long, value_name = "N", default_value_t = 100_000)]
        max_ratio: u64,

        /// Вывести машинночитаемый JSON.
        #[arg(long)]
        json: bool,
    },

    /// Показать содержимое архива.
    #[command(alias = "l")]
    List {
        /// Путь к архиву.
        #[arg(value_name = "АРХИВ")]
        archive: PathBuf,

        /// Пароль от зашифрованного архива.
        #[arg(short = 'p', long, value_name = "ПАРОЛЬ", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        password: Option<String>,

        /// Взять пароль из первой строки файла.
        #[arg(long = "password-file", value_name = "ПУТЬ")]
        password_file: Option<PathBuf>,

        /// Подробный список: метод, дата, степень сжатия.
        #[arg(short, long)]
        long: bool,

        /// Вывести машинночитаемый JSON.
        #[arg(long)]
        json: bool,
    },

    /// Проверить целостность архива, ничего не распаковывая.
    #[command(alias = "t")]
    Test {
        /// Путь к архиву.
        #[arg(value_name = "АРХИВ")]
        archive: PathBuf,

        /// Пароль от зашифрованного архива.
        #[arg(short = 'p', long, value_name = "ПАРОЛЬ", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        password: Option<String>,

        /// Взять пароль из первой строки файла.
        #[arg(long = "password-file", value_name = "ПУТЬ")]
        password_file: Option<PathBuf>,

        /// Максимально допустимая степень сжатия блока (0 — без ограничения).
        #[arg(long, value_name = "N", default_value_t = 100_000)]
        max_ratio: u64,

        /// Вывести машинночитаемый JSON.
        #[arg(long)]
        json: bool,
    },

    /// Показать сведения об архиве.
    #[command(alias = "i")]
    Info {
        /// Путь к архиву.
        #[arg(value_name = "АРХИВ")]
        archive: PathBuf,

        /// Пароль от зашифрованного архива.
        #[arg(short = 'p', long, value_name = "ПАРОЛЬ", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        password: Option<String>,

        /// Взять пароль из первой строки файла.
        #[arg(long = "password-file", value_name = "ПУТЬ")]
        password_file: Option<PathBuf>,

        /// Вывести машинночитаемый JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if !cli.no_utf8 {
        util::enable_utf8_console();
    }
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("\npliczip: ошибка: {:#}", err);
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let quiet = cli.quiet;
    match cli.command {
        Cmd::Add {
            archive,
            inputs,
            level,
            solid,
            block_size,
            comment,
            follow_links,
            password,
            password_file,
            volume_size,
            include,
            exclude,
            json,
        } => {
            let started = Instant::now();
            let secret = password_for_create(password.as_deref(), password_file.as_deref())?;
            let encrypted = secret.is_some();

            let opts = CreateOptions {
                level,
                solid,
                block_size,
                comment,
                follow_links,
                show_progress: !quiet && !json,
                password: secret,
                volume_size,
                include,
                exclude,
                progress: None,
            };
            let stats = writer::create(&archive, &inputs, &opts)?;

            if json {
                let document = serde_json::json!({
                    "ok": true,
                    "archive": archive.to_string_lossy(),
                    "files": stats.files,
                    "dirs": stats.dirs,
                    "links": stats.links,
                    "blocks": stats.blocks,
                    "raw_bytes": stats.raw_bytes,
                    "data_bytes": stats.data_bytes,
                    "archive_bytes": stats.archive_bytes,
                    "volumes": stats.volumes,
                    "encrypted": stats.encrypted,
                    "seconds": started.elapsed().as_secs_f64(),
                });
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else if !quiet {
                println!();
                println!("PlicZip: создан архив {}", archive.display());
                println!(
                    "  элементов: {}, {}, {}, {}",
                    plural(stats.files, "файл", "файла", "файлов"),
                    plural(stats.dirs, "каталог", "каталога", "каталогов"),
                    plural(stats.links, "ссылка", "ссылки", "ссылок"),
                    plural(stats.blocks, "блок", "блока", "блоков")
                );
                println!(
                    "  данные:    {} -> {}  ({:.1}% экономии, {:.2}:1)",
                    human_bytes(stats.raw_bytes),
                    human_bytes(stats.data_bytes),
                    saved_percent(stats.raw_bytes, stats.data_bytes),
                    ratio(stats.raw_bytes, stats.data_bytes)
                );
                println!(
                    "  файл:      {}{}{}",
                    human_bytes(stats.archive_bytes),
                    if stats.volumes > 1 {
                        format!(" в {} томах", stats.volumes)
                    } else {
                        String::new()
                    },
                    if encrypted { " · зашифрован" } else { "" }
                );
                println!("  время:     {}", human_duration(started.elapsed()));
                if encrypted {
                    println!();
                    println!("  Пароль не восстановить: без него архив не открыть.");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Extract {
            archive,
            output,
            password,
            password_file,
            skip_existing,
            no_verify,
            no_symlinks,
            no_mtime,
            max_ratio,
            json,
        } => {
            let started = Instant::now();
            let secret = password_for_open(password.as_deref(), password_file.as_deref())?;
            let mut ar = Archive::open_with(&archive, secret.as_deref().map(String::as_str))?;
            let opts = reader::ExtractOptions {
                output,
                skip_existing,
                verify: !no_verify,
                symlinks: !no_symlinks,
                restore_mtime: !no_mtime,
                max_ratio,
                show_progress: !quiet && !json,
                progress: None,
            };
            let stats = ar.extract(&opts)?;

            if json {
                let document = serde_json::json!({
                    "ok": true,
                    "files": stats.files,
                    "dirs": stats.dirs,
                    "links": stats.links,
                    "skipped": stats.skipped,
                    "bytes": stats.bytes,
                    "seconds": started.elapsed().as_secs_f64(),
                });
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else if !quiet {
                println!();
                println!(
                    "PlicZip: распаковано {} ({}), {}, {} за {}",
                    plural(stats.files, "файл", "файла", "файлов"),
                    human_bytes(stats.bytes),
                    plural(stats.dirs, "каталог", "каталога", "каталогов"),
                    plural(stats.links, "ссылка", "ссылки", "ссылок"),
                    human_duration(started.elapsed())
                );
                if stats.skipped > 0 {
                    println!("  пропущено элементов: {}", stats.skipped);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::List {
            archive,
            password,
            password_file,
            long,
            json,
        } => {
            let secret = password_for_open(password.as_deref(), password_file.as_deref())?;
            let ar = Archive::open_with(&archive, secret.as_deref().map(String::as_str))?;
            let (files, dirs, links) = ar.index.counts();

            if json {
                let entries: Vec<serde_json::Value> = ar
                    .entries()
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "path": entry.path,
                            "kind": entry.kind,
                            "size": entry.size,
                            "mode": entry.mode,
                            "mtime": entry.mtime,
                            "hash": hex(&entry.hash),
                            "method": entry.block
                                .and_then(|bi| ar.index.blocks.get(bi as usize))
                                .map(|block| block.method.name()),
                        })
                    })
                    .collect();
                let document = serde_json::json!({
                    "archive": archive.to_string_lossy(),
                    "version": ar.index.version,
                    "encrypted": ar.is_encrypted(),
                    "solid": ar.index.solid,
                    "tool": ar.index.tool,
                    "created_at": ar.index.created_at,
                    "files": files,
                    "dirs": dirs,
                    "links": links,
                    "blocks": ar.index.blocks.len(),
                    "entries": entries,
                });
                println!("{}", serde_json::to_string_pretty(&document)?);
                return Ok(ExitCode::SUCCESS);
            }

            println!("Архив: {}", archive.display());
            if ar.is_encrypted() {
                println!("Зашифрован паролем");
            }
            println!();
            if long {
                println!(
                    "  {}  {}  {}  Путь",
                    pad_left("Размер", 10),
                    pad_right("Метод", 8),
                    pad_right("Сжатие", 7)
                );
            } else {
                println!("  {}  Путь", pad_left("Размер", 10));
            }

            for entry in ar.entries() {
                print_entry(&ar, entry, long);
            }

            println!();
            println!(
                "  итого: {}: {}, {}, {}",
                plural(ar.entries().len(), "элемент", "элемента", "элементов"),
                plural(files, "файл", "файла", "файлов"),
                plural(dirs, "каталог", "каталога", "каталогов"),
                plural(links, "ссылка", "ссылки", "ссылок")
            );
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Test {
            archive,
            password,
            password_file,
            max_ratio,
            json,
        } => {
            let started = Instant::now();
            let secret = password_for_open(password.as_deref(), password_file.as_deref())?;
            let mut ar = Archive::open_with(&archive, secret.as_deref().map(String::as_str))?;
            match ar.test(!quiet && !json, max_ratio, None) {
                Ok(report) => {
                    if json {
                        let document = serde_json::json!({
                            "ok": true,
                            "blocks": report.blocks,
                            "files": report.files,
                            "bytes": report.bytes,
                        });
                        println!("{}", serde_json::to_string_pretty(&document)?);
                    } else if !quiet {
                        println!(
                            "  проверено {}, {} за {}",
                            plural(report.blocks, "блок", "блока", "блоков"),
                            plural(report.files, "файл", "файла", "файлов"),
                            human_duration(started.elapsed())
                        );
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(err) => {
                    if json {
                        let document = serde_json::json!({
                            "ok": false,
                            "error": format!("{:#}", err),
                        });
                        println!("{}", serde_json::to_string_pretty(&document)?);
                    } else {
                        eprintln!("pliczip: архив повреждён: {:#}", err);
                    }
                    Ok(ExitCode::from(1))
                }
            }
        }

        Cmd::Info {
            archive,
            password,
            password_file,
            json,
        } => {
            let secret = password_for_open(password.as_deref(), password_file.as_deref())?;
            let ar = Archive::open_with(&archive, secret.as_deref().map(String::as_str))?;
            let index = &ar.index;
            let (files, dirs, links) = index.counts();
            let raw = index.total_size();
            let stored = index.stored_size();
            let file_size: u64 = ar
                .files()
                .iter()
                .filter_map(|path| std::fs::metadata(path).ok())
                .map(|meta| meta.len())
                .sum();
            let index_len = file_size
                .saturating_sub(ar.index_start)
                .saturating_sub(pliczip::format::FOOTER_LEN);

            if json {
                let document = serde_json::json!({
                    "archive": archive.to_string_lossy(),
                    "version": index.version,
                    "encrypted": ar.is_encrypted(),
                    "solid": index.solid,
                    "tool": index.tool,
                    "created_at": index.created_at,
                    "comment": index.comment,
                    "files": files,
                    "dirs": dirs,
                    "links": links,
                    "blocks": index.blocks.len(),
                    "volumes": ar.files().len(),
                    "raw_bytes": raw,
                    "stored_bytes": stored,
                    "archive_bytes": file_size,
                    "index_bytes": index_len,
                });
                println!("{}", serde_json::to_string_pretty(&document)?);
                return Ok(ExitCode::SUCCESS);
            }

            println!("Архив:      {}", archive.display());
            println!("Формат:     PlicZip v{}", index.version);
            println!(
                "Шифрование: {}",
                if ar.is_encrypted() {
                    "ChaCha20-Poly1305, ключ через Argon2id"
                } else {
                    "нет"
                }
            );
            println!("Создан:     {}", format_unix_time(index.created_at));
            println!("Инструмент: {}", index.tool);
            if !index.comment.is_empty() {
                println!("Комментарий: {}", index.comment);
            }
            println!(
                "Режим:      {}",
                if index.solid { "solid" } else { "обычный" }
            );
            println!(
                "Содержимое: {}: {}, {}, {}",
                plural(index.entries.len(), "элемент", "элемента", "элементов"),
                plural(files, "файл", "файла", "файлов"),
                plural(dirs, "каталог", "каталога", "каталогов"),
                plural(links, "ссылка", "ссылки", "ссылок")
            );
            println!("Блоков:     {}", index.blocks.len());
            if ar.files().len() > 1 {
                println!("Томов:      {}", ar.files().len());
            }
            println!(
                "Данные:     {} -> {}  ({:.1}% экономии, {:.2}:1)",
                human_bytes(raw),
                human_bytes(stored),
                saved_percent(raw, stored),
                ratio(raw, stored)
            );
            println!("Файл:       {}", human_bytes(file_size));
            println!(
                "Индекс:     {} по смещению {}",
                human_bytes(index_len),
                ar.index_start
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Читает пароль из первой строки файла.
fn password_from_file(path: &Path) -> Result<Zeroizing<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("не удалось прочитать файл пароля {:?}", path))?;
    let line = text
        .lines()
        .next()
        .unwrap_or("")
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if line.is_empty() {
        bail!("файл пароля {:?} пуст", path);
    }
    Ok(Zeroizing::new(line))
}

fn password_from_env() -> Option<Zeroizing<String>> {
    match std::env::var(PASSWORD_ENV) {
        Ok(value) if !value.is_empty() => Some(Zeroizing::new(value)),
        _ => None,
    }
}

/// Пароль для создания архива. `None` — архив без шифрования.
///
/// Порядок: файл → явное значение → интерактивный ввод → переменная окружения.
fn password_for_create(
    value: Option<&str>,
    file: Option<&Path>,
) -> Result<Option<Zeroizing<String>>> {
    if let Some(path) = file {
        return Ok(Some(password_from_file(path)?));
    }
    if let Some(value) = value {
        if value.is_empty() {
            return Ok(Some(crypto::new_password("Пароль для архива")?));
        }
        return Ok(Some(Zeroizing::new(value.to_string())));
    }
    Ok(password_from_env())
}

/// Пароль для чтения архива. `None` — пусть `Archive` спросит, если потребуется.
fn password_for_open(value: Option<&str>, file: Option<&Path>) -> Result<Option<Zeroizing<String>>> {
    if let Some(path) = file {
        return Ok(Some(password_from_file(path)?));
    }
    if let Some(value) = value {
        if value.is_empty() {
            return Ok(Some(crypto::password(None, "Пароль архива")?));
        }
        return Ok(Some(Zeroizing::new(value.to_string())));
    }
    Ok(password_from_env())
}

fn print_entry(ar: &Archive, entry: &PzEntry, long: bool) {
    let size = match entry.kind {
        Kind::File => human_bytes(entry.size),
        Kind::Dir => "<каталог>".to_string(),
        Kind::Symlink => "<ссылка>".to_string(),
        Kind::Other => "<неизвестно>".to_string(),
    };
    let (method, compression) = match entry.block {
        Some(bi) if (bi as usize) < ar.index.blocks.len() => {
            let block = &ar.index.blocks[bi as usize];
            let saved = if block.size > 0 {
                format!("{:.1}:1", ratio(block.size, block.stored_size))
            } else {
                "-".to_string()
            };
            (block.method.name().to_string(), saved)
        }
        _ => ("-".to_string(), "-".to_string()),
    };
    let path = match entry.kind {
        Kind::Dir => format!("{}/", entry.path),
        Kind::Symlink => format!("{} -> {}", entry.path, entry.link),
        _ => entry.path.clone(),
    };

    if long {
        println!(
            "  {}  {}  {}  {}{}",
            pad_left(&size, 10),
            pad_right(&method, 8),
            pad_right(&compression, 7),
            if entry.mtime > 0 {
                format!("{}  ", format_unix_time(entry.mtime))
            } else {
                String::new()
            },
            path
        );
    } else {
        println!("  {}  {}", pad_left(&size, 10), path);
    }
}

/// Печатает хеш в hex.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

/// Разбирает размер вида `8M`, `512K`, `1G` или просто число байт.
fn parse_size(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("пустой размер".to_string());
    }
    let (digits, multiplier) = match trimmed.chars().last().unwrap() {
        'k' | 'K' => (&trimmed[..trimmed.len() - 1], 1024u64),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 1024 * 1024),
        'g' | 'G' => (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024),
        _ => (trimmed, 1),
    };
    let value: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("не понимаю размер {:?}", text))?;
    value
        .checked_mul(multiplier)
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("недопустимый размер {:?}", text))
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("8M").unwrap(), 8 * 1024 * 1024);
        assert_eq!(parse_size("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("100").unwrap(), 100);
        assert!(parse_size("0").is_err());
        assert!(parse_size("abc").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn hex_prints_bytes() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
