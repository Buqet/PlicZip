//! Интерфейс командной строки PlicZip.
//!
//! Команды повторяют привычные сокращения 7-Zip: `a` (add), `x` (extract),
//! `l` (list), `t` (test), `i` (info).

use anyhow::{bail, Context, Result};
use argh::FromArgs;
use pliczip::prelude::*;
use pliczip::util::{
    format_unix_time, human_bytes, human_duration, pad_left, pad_right, plural, ratio,
    saved_percent,
};
use pliczip::{crypto, external, reader, util, writer};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use zeroize::Zeroizing;

/// степень сжатия по умолчанию.
const DEFAULT_LEVEL: i32 = 5;

/// переменная окружения с паролем — удобно для скриптов.
const PASSWORD_ENV: &str = "PLICZIP_PASSWORD";

#[derive(FromArgs, Debug)]
/// plicZip — архиватор с собственным форматом .plz.
#[argh(help_triggers("-h", "--help", "help"))]
struct Cli {
    /// не показывать прогресс и сводки.
    #[argh(switch, short = 'q')]
    quiet: bool,

    /// не включать UTF-8 в консоли Windows (если мешает).
    #[argh(switch, hidden_help)]
    no_utf8: bool,

    /// подкоманда.
    #[argh(subcommand)]
    command: Cmd,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum Cmd {
    Add(Add),
    Extract(Extract),
    List(List),
    Test(Test),
    Info(Info),
    Repair(Repair),
    Update(Update),
    Keygen(Keygen),
}

#[derive(FromArgs, Debug)]
/// создать архив из файлов и каталогов.
#[argh(subcommand, name = "add")]
struct Add {
    /// путь к создаваемому архиву.
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    /// что упаковать (файлы и каталоги).
    #[argh(positional, arg_name = "ПУТИ")]
    inputs: Vec<PathBuf>,
    /// степень сжатия: 0 — без сжатия, 1..4 — deflate, 5..11 — brotli.
    #[argh(
        option,
        short = 'x',
        long = "level",
        default = "DEFAULT_LEVEL",
        from_str_fn(parse_level)
    )]
    level: i32,
    /// использовать Density (Lion) вместо метода, выбранного уровнем.
    #[argh(switch)]
    density: bool,
    /// склеивать мелкие файлы в общие solid-блоки.
    #[argh(switch, short = 's')]
    solid: bool,
    /// целевой размер solid-блока.
    #[argh(
        option,
        short = 'b',
        long = "block-size",
        default = "parse_size(\"8M\").unwrap()",
        from_str_fn(parse_size)
    )]
    block_size: u64,
    /// комментарий, который сохранится в архиве.
    #[argh(option, short = 'c', default = "String::new()")]
    comment: String,
    /// раскрывать символические ссылки вместо их сохранения.
    #[argh(switch)]
    follow_links: bool,
    /// зашифровать паролем. Без значения пароль спрашивается интерактивно.
    #[argh(option, short = 'p')]
    /// пароль от зашифрованного архива.
    password: Option<String>,
    /// взять пароль из первой строки файла.
    #[argh(option, long = "password-file")]
    /// взять пароль из первой строки файла.
    password_file: Option<PathBuf>,
    /// взять ключ из бинарного файла.
    #[argh(option, long = "keyfile")]
    /// взять ключ из бинарного файла.
    keyfile: Option<PathBuf>,
    /// размер тома.
    #[argh(option, long = "volume-size", from_str_fn(parse_size))]
    volume_size: Option<u64>,
    /// добавить запись для восстановления объёмом ПРОЦЕНТ от архива.
    #[argh(option, from_str_fn(parse_percent))]
    recovery: Option<u32>,
    /// включить только эти маски (можно повторять).
    #[argh(option)]
    include: Vec<String>,
    /// исключить эти маски (можно повторять).
    #[argh(option)]
    exclude: Vec<String>,
    /// вывести машинночитаемый RON.
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// распаковать архив.
#[argh(subcommand, name = "extract")]
struct Extract {
    /// путь к архиву.
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    /// куда распаковывать.
    #[argh(option, short = 'o', default = "PathBuf::from(\".\")")]
    output: PathBuf,
    /// пароль от зашифрованного архива.
    #[argh(option, short = 'p')]
    /// пароль от зашифрованного архива.
    password: Option<String>,
    /// взять пароль из первой строки файла.
    #[argh(option, long = "password-file")]
    /// взять пароль из первой строки файла.
    password_file: Option<PathBuf>,
    /// взять ключ из бинарного файла.
    #[argh(option, long = "keyfile")]
    /// взять ключ из бинарного файла.
    keyfile: Option<PathBuf>,
    /// не перезаписывать уже существующие файлы.
    #[argh(switch)]
    skip_existing: bool,
    /// не проверять контрольные суммы.
    #[argh(switch)]
    no_verify: bool,
    /// не восстанавливать символические ссылки.
    #[argh(switch)]
    no_symlinks: bool,
    /// не восстанавливать время изменения.
    #[argh(switch)]
    no_mtime: bool,
    /// максимально допустимая степень сжатия блока.
    #[argh(option, default = "100_000")]
    /// максимально допустимая степень сжатия блока.
    max_ratio: u64,
    /// вывести машинночитаемый RON.
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// показать содержимое архива.
#[argh(subcommand, name = "list")]
struct List {
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    #[argh(option, short = 'p')]
    /// пароль от зашифрованного архива.
    password: Option<String>,
    #[argh(option, long = "password-file")]
    /// взять пароль из первой строки файла.
    password_file: Option<PathBuf>,
    #[argh(option, long = "keyfile")]
    /// взять ключ из бинарного файла.
    keyfile: Option<PathBuf>,
    /// подробный список: метод, дата, степень сжатия.
    #[argh(switch, short = 'l')]
    long: bool,
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// проверить целостность архива, ничего не распаковывая.
#[argh(subcommand, name = "test")]
struct Test {
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    #[argh(option, short = 'p')]
    /// пароль от зашифрованного архива.
    password: Option<String>,
    #[argh(option, long = "password-file")]
    /// взять пароль из первой строки файла.
    password_file: Option<PathBuf>,
    #[argh(option, long = "keyfile")]
    /// взять ключ из бинарного файла.
    keyfile: Option<PathBuf>,
    #[argh(option, default = "100_000")]
    /// максимально допустимая степень сжатия блока.
    max_ratio: u64,
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// показать сведения об архиве.
#[argh(subcommand, name = "info")]
struct Info {
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    #[argh(option, short = 'p')]
    /// пароль от зашифрованного архива.
    password: Option<String>,
    #[argh(option, long = "password-file")]
    /// взять пароль из первой строки файла.
    password_file: Option<PathBuf>,
    #[argh(option, long = "keyfile")]
    /// взять ключ из бинарного файла.
    keyfile: Option<PathBuf>,
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// восстановить повреждённый архив по записи parity.
#[argh(subcommand, name = "repair")]
struct Repair {
    #[argh(positional, arg_name = "АРХИВ")]
    archive: PathBuf,
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// проверить наличие новой версии на GitHub.
#[argh(subcommand, name = "update")]
struct Update {
    #[argh(switch)]
    /// вывести машинночитаемый RON.
    ron: bool,
}

#[derive(FromArgs, Debug)]
/// создать случайный бинарный keyfile.
#[argh(subcommand, name = "keygen")]
struct Keygen {
    #[argh(positional, arg_name = "ПУТЬ")]
    path: PathBuf,
    #[argh(option, default = "32", from_str_fn(parse_key_bytes))]
    /// размер ключа в байтах.
    bytes: usize,
}

fn parse_cli() -> Cli {
    let raw: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let command_names = [
        "add", "extract", "list", "test", "info", "repair", "update", "keygen",
    ];
    let aliases = [
        ("a", "add"),
        ("x", "extract"),
        ("l", "list"),
        ("t", "test"),
        ("i", "info"),
        ("r", "repair"),
    ];
    let mut normalized = Vec::with_capacity(raw.len() + 3);
    let mut command_seen = false;
    let mut quiet = false;
    let mut no_utf8 = false;
    for arg in raw.iter().skip(1) {
        if !command_seen && command_names.contains(&arg.as_str()) {
            command_seen = true;
            normalized.push(arg.clone());
            continue;
        }
        if !command_seen {
            if let Some((_, canonical)) = aliases.iter().find(|(alias, _)| *alias == arg) {
                command_seen = true;
                normalized.push((*canonical).to_owned());
                continue;
            }
        }
        match arg.as_str() {
            "-q" | "--quiet" => quiet = true,
            "--no-utf8" => no_utf8 = true,
            "-p" | "--password" => {
                normalized.push("--password".to_owned());
                normalized.push(String::new());
            }
            value if value.starts_with("--password=") => {
                normalized.push("--password".to_owned());
                normalized.push(value[11..].to_owned());
            }
            _ => normalized.push(arg.clone()),
        }
    }
    let mut args = Vec::with_capacity(normalized.len() + 3);
    args.push(raw.first().cloned().unwrap_or_else(|| "pliczip".to_owned()));
    if quiet {
        args.push("--quiet".to_owned());
    }
    if no_utf8 {
        args.push("--no-utf8".to_owned());
    }
    args.extend(normalized);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    Cli::from_args(&[refs[0]], &refs[1..]).unwrap_or_else(|early| {
        if early.status.is_ok() {
            println!("{}", early.output);
            std::process::exit(0);
        }
        eprintln!(
            "{}\nRun {} --help for more information.",
            early.output, refs[0]
        );
        std::process::exit(1);
    })
}

fn main() -> ExitCode {
    let cli = parse_cli();
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
        Cmd::Add(Add {
            archive,
            inputs,
            level,
            density,
            solid,
            block_size,
            comment,
            follow_links,
            password,
            password_file,
            keyfile,
            volume_size,
            recovery,
            include,
            exclude,
            ron,
        }) => {
            let started = Instant::now();
            if keyfile.is_some() && (password.is_some() || password_file.is_some()) {
                bail!("нельзя комбинировать --password/--password-file и --keyfile");
            }
            let key_bytes = keyfile.as_deref().map(password_from_keyfile).transpose()?;
            let secret = password_for_create(password.as_deref(), password_file.as_deref(), None)?;
            let encrypted = secret.is_some() || key_bytes.is_some();

            let opts = CreateOptions {
                level,
                method: density.then_some(Method::Density),
                solid,
                block_size,
                comment,
                follow_links,
                show_progress: !quiet && !ron,
                password: secret,
                password_bytes: key_bytes,
                volume_size,
                include,
                exclude,
                progress: None,
                recovery,
            };
            let stats = writer::create(&archive, &inputs, &opts)?;

            if ron {
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
                    "recovery_percent": recovery,
                    "recovery_bytes": stats.recovery_bytes,
                    "seconds": started.elapsed().as_secs_f64(),
                });
                println!("{}", ron_pretty(&document)?);
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
                    if encrypted {
                        " · зашифрован"
                    } else {
                        ""
                    }
                );
                if let Some(percent) = recovery {
                    println!(
                        "  восстановление: {}% ({})",
                        percent,
                        human_bytes(stats.recovery_bytes)
                    );
                }
                println!("  время:     {}", human_duration(started.elapsed()));
                if encrypted {
                    println!();
                    println!("  Пароль не восстановить: без него архив не открыть.");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Extract(Extract {
            archive,
            output,
            password,
            password_file,
            keyfile,
            skip_existing,
            no_verify,
            no_symlinks,
            no_mtime,
            max_ratio,
            ron,
        }) => {
            let started = Instant::now();
            let secret = password_for_open(
                password.as_deref(),
                password_file.as_deref(),
                keyfile.as_deref(),
            )?;
            if external_kind(&archive)?.is_some() {
                let key = external_password_bytes(secret.as_ref(), keyfile.as_deref())?;
                let (kind, stats) = external::extract(
                    &archive,
                    &output,
                    key.as_ref().map(|value| value.as_slice()),
                    skip_existing,
                )?;
                if ron {
                    println!(
                        "{}",
                        ron_pretty(&serde_json::json!({
                            "ok": true,
                            "format": kind.name(),
                            "files": stats.files,
                            "dirs": stats.dirs,
                            "bytes": stats.bytes,
                            "skipped": stats.skipped,
                            "seconds": started.elapsed().as_secs_f64(),
                        }))?
                    );
                } else if !quiet {
                    println!(
                        "PlicZip: распакован {}-архив: {}, {}, {} за {}",
                        kind.name(),
                        plural(stats.files, "файл", "файла", "файлов"),
                        human_bytes(stats.bytes),
                        plural(stats.dirs, "каталог", "каталога", "каталогов"),
                        human_duration(started.elapsed())
                    );
                }
                return Ok(ExitCode::SUCCESS);
            }
            let mut ar = open_archive(&archive, secret.as_ref(), keyfile.as_deref())?;
            let opts = reader::ExtractOptions {
                output,
                skip_existing,
                verify: !no_verify,
                symlinks: !no_symlinks,
                restore_mtime: !no_mtime,
                max_ratio,
                show_progress: !quiet && !ron,
                progress: None,
            };
            let stats = ar.extract(&opts)?;

            if ron {
                let document = serde_json::json!({
                    "ok": true,
                    "files": stats.files,
                    "dirs": stats.dirs,
                    "links": stats.links,
                    "skipped": stats.skipped,
                    "bytes": stats.bytes,
                    "seconds": started.elapsed().as_secs_f64(),
                });
                println!("{}", ron_pretty(&document)?);
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

        Cmd::List(List {
            archive,
            password,
            password_file,
            keyfile,
            long,
            ron,
        }) => {
            let secret = password_for_open(
                password.as_deref(),
                password_file.as_deref(),
                keyfile.as_deref(),
            )?;
            if external_kind(&archive)?.is_some() {
                let key = external_password_bytes(secret.as_ref(), keyfile.as_deref())?;
                let (kind, entries) =
                    external::list(&archive, key.as_ref().map(|value| value.as_slice()))?;
                print_external_list(&archive, kind, &entries, long, ron)?;
                return Ok(ExitCode::SUCCESS);
            }
            let ar = open_archive(&archive, secret.as_ref(), keyfile.as_deref())?;
            let (files, dirs, links) = ar.index.counts();

            if ron {
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
                println!("{}", ron_pretty(&document)?);
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

        Cmd::Test(Test {
            archive,
            password,
            password_file,
            keyfile,
            max_ratio,
            ron,
        }) => {
            let started = Instant::now();
            let secret = password_for_open(
                password.as_deref(),
                password_file.as_deref(),
                keyfile.as_deref(),
            )?;
            if external_kind(&archive)?.is_some() {
                let key = external_password_bytes(secret.as_ref(), keyfile.as_deref())?;
                let (kind, files) =
                    external::test(&archive, key.as_ref().map(|value| value.as_slice()))?;
                if ron {
                    println!(
                        "{}",
                        ron_pretty(&serde_json::json!({
                            "ok": true,
                            "format": kind.name(),
                            "files": files,
                            "seconds": started.elapsed().as_secs_f64(),
                        }))?
                    );
                } else if !quiet {
                    println!(
                        "  проверен {}-архив: {}, за {}",
                        kind.name(),
                        plural(files, "файл", "файла", "файлов"),
                        human_duration(started.elapsed())
                    );
                }
                return Ok(ExitCode::SUCCESS);
            }
            let mut ar = open_archive(&archive, secret.as_ref(), keyfile.as_deref())?;
            let rr_report = ar.verify_recovery(!quiet && !ron)?;
            let rr_clean = rr_report.as_ref().is_none_or(|report| report.is_clean());
            if !ron {
                if let Some(report) = &rr_report {
                    print_recovery_report(report, quiet);
                }
            }
            let rr_ron = rr_report.as_ref().map(recovery_report_ron);
            match ar.test(!quiet && !ron, max_ratio, None) {
                Ok(report) => {
                    if ron {
                        let document = serde_json::json!({
                            "ok": rr_clean,
                            "blocks": report.blocks,
                            "files": report.files,
                            "bytes": report.bytes,
                            "recovery": rr_ron,
                        });
                        println!("{}", ron_pretty(&document)?);
                    } else if !quiet {
                        println!(
                            "  проверено {}, {} за {}",
                            plural(report.blocks, "блок", "блока", "блоков"),
                            plural(report.files, "файл", "файла", "файлов"),
                            human_duration(started.elapsed())
                        );
                    }
                    if rr_clean {
                        Ok(ExitCode::SUCCESS)
                    } else {
                        if !ron {
                            eprintln!(
                                "pliczip: данные архива целы, но повреждена запись для восстановления"
                            );
                        }
                        Ok(ExitCode::from(1))
                    }
                }
                Err(err) => {
                    if ron {
                        let document = serde_json::json!({
                            "ok": false,
                            "error": format!("{:#}", err),
                            "recovery": rr_ron,
                        });
                        println!("{}", ron_pretty(&document)?);
                    } else {
                        eprintln!("pliczip: архив повреждён: {:#}", err);
                    }
                    Ok(ExitCode::from(1))
                }
            }
        }

        Cmd::Info(Info {
            archive,
            password,
            password_file,
            keyfile,
            ron,
        }) => {
            let secret = password_for_open(
                password.as_deref(),
                password_file.as_deref(),
                keyfile.as_deref(),
            )?;
            if external_kind(&archive)?.is_some() {
                let key = external_password_bytes(secret.as_ref(), keyfile.as_deref())?;
                let (kind, entries) =
                    external::list(&archive, key.as_ref().map(|value| value.as_slice()))?;
                let files = entries.iter().filter(|entry| !entry.is_dir).count();
                let dirs = entries.iter().filter(|entry| entry.is_dir).count();
                let bytes: u64 = entries
                    .iter()
                    .filter(|entry| !entry.is_dir)
                    .map(|entry| entry.size)
                    .sum();
                if ron {
                    println!(
                        "{}",
                        ron_pretty(&serde_json::json!({
                            "archive": archive.to_string_lossy(),
                            "format": kind.name(),
                            "encrypted": key.is_some(),
                            "files": files,
                            "dirs": dirs,
                            "bytes": bytes,
                            "entries": entries.iter().map(|entry| serde_json::json!({
                                "path": entry.path,
                                "size": entry.size,
                                "kind": if entry.is_dir { "dir" } else { "file" },
                            })).collect::<Vec<_>>(),
                        }))?
                    );
                } else {
                    println!("Архив: {} ({})", archive.display(), kind.name());
                    println!(
                        "  файлов: {}, каталогов: {}, размер: {}",
                        files,
                        dirs,
                        human_bytes(bytes)
                    );
                }
                return Ok(ExitCode::SUCCESS);
            }
            let ar = open_archive(&archive, secret.as_ref(), keyfile.as_deref())?;
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
            let index_len = ar
                .data_len()
                .saturating_sub(ar.index_start)
                .saturating_sub(pliczip::format::FOOTER_LEN);

            if ron {
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
                    "recovery": ar.recovery.as_ref().map(|trailer| serde_json::json!({
                        "version": pliczip::recovery::VERSION,
                        "percent": trailer.plan.percent,
                        "shard_size": trailer.plan.shard_size,
                        "data_shards": trailer.plan.data_shards,
                        "parity_shards": trailer.plan.parity_shards,
                        "groups": trailer.plan.groups,
                        "protected_bytes": trailer.plan.protected_len,
                        "recovery_bytes": trailer.plan.section_len(),
                        "merkle_root": hex(&trailer.merkle_root),
                    })),
                });
                println!("{}", ron_pretty(&document)?);
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
                if index.solid {
                    "solid"
                } else {
                    "обычный"
                }
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
            match &ar.recovery {
                Some(trailer) => {
                    let plan = &trailer.plan;
                    println!(
                        "Восстановление: {}% ({}): {} данных + {} parity по {}, {}",
                        plan.percent,
                        human_bytes(plan.section_len()),
                        plural(plan.data_shards as usize, "кусок", "куска", "кусков"),
                        plan.parity_shards,
                        human_bytes(plan.shard_size),
                        plural(plan.groups as usize, "группа", "группы", "групп")
                    );
                    println!("Корень Меркла: {}", hex(&trailer.merkle_root));
                }
                None => println!("Восстановление: нет"),
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Repair(Repair { archive, ron }) => {
            let report = pliczip::recovery::repair_file(&archive, !quiet && !ron)?;
            if ron {
                println!(
                    "{}",
                    ron_pretty(&serde_json::json!({
                        "ok": true,
                        "restored_data": report.restored_data,
                        "restored_parity": report.restored_parity,
                        "repaired_trailers": report.repaired_trailers,
                    }))?
                );
            } else if !quiet {
                println!(
                    "PlicZip: восстановлено data-кусков: {}, parity-кусков: {}, трейлеры: {}",
                    report.restored_data,
                    report.restored_parity,
                    if report.repaired_trailers {
                        "исправлены"
                    } else {
                        "целы"
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Update(Update { ron }) => {
            let release = pliczip::update::check_latest()?;
            let available = release.latest != format!("v{}", release.current)
                && release.latest != release.current;
            if ron {
                println!(
                    "{}",
                    ron_pretty(&serde_json::json!({
                        "ok": true,
                        "current": release.current,
                        "latest": release.latest,
                        "update_available": available,
                        "url": release.url,
                    }))?
                );
            } else if available {
                println!("Доступно обновление PlicZip: {}", release.latest);
                println!("Скачать: {}", release.url);
            } else {
                println!(
                    "Установлена актуальная версия PlicZip ({})",
                    release.current
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Keygen(Keygen { path, bytes }) => {
            let mut key = Zeroizing::new(vec![0u8; bytes]);
            getrandom::getrandom(&mut key)
                .map_err(|err| anyhow::anyhow!("не удалось создать случайный keyfile: {err}"))?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .with_context(|| format!("не удалось создать keyfile {:?}", path))?;
            let write_result = (|| -> Result<()> {
                file.write_all(&key)
                    .with_context(|| format!("не удалось записать keyfile {:?}", path))?;
                file.sync_all()
                    .with_context(|| format!("не удалось синхронизировать keyfile {:?}", path))?;
                Ok(())
            })();
            if let Err(err) = write_result {
                let _ = std::fs::remove_file(&path);
                return Err(err);
            }
            println!("Создан keyfile {} ({} байт)", path.display(), bytes);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Разбирает объём записи для восстановления: `5` или `5%`.
fn parse_level(text: &str) -> Result<i32, String> {
    let value = text
        .parse::<i32>()
        .map_err(|_| "степень должна быть целым числом".to_owned())?;
    if (0..=11).contains(&value) {
        Ok(value)
    } else {
        Err("степень должна быть от 0 до 11".to_owned())
    }
}

fn parse_percent(text: &str) -> Result<u32, String> {
    let digits = text.trim().trim_end_matches('%').trim();
    let value: u32 = digits
        .parse()
        .map_err(|_| format!("ожидалось целое число процентов, получено {:?}", text))?;
    let range = pliczip::recovery::MIN_PERCENT..=pliczip::recovery::MAX_PERCENT;
    if !range.contains(&value) {
        return Err(format!(
            "объём должен быть от {}% до {}%",
            range.start(),
            range.end()
        ));
    }
    Ok(value)
}

fn parse_key_bytes(text: &str) -> Result<usize, String> {
    let value: usize = text
        .parse()
        .map_err(|_| format!("ожидалось число байт, получено {:?}", text))?;
    if !(16..=4096).contains(&value) {
        return Err("размер keyfile должен быть от 16 до 4096 байт".into());
    }
    Ok(value)
}

fn ron_pretty<T: serde::Serialize>(value: &T) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
}

fn recovery_report_ron(report: &pliczip::recovery::VerifyReport) -> serde_json::Value {
    serde_json::json!({
        "ok": report.is_clean(),
        "data_shards": report.data_shards,
        "parity_shards": report.parity_shards,
        "bad_data": report.bad_data,
        "bad_parity": report.bad_parity,
        "table_ok": report.table_ok,
        "trailer_copy_ok": report.trailer_copy_ok,
        "trailer_end_ok": report.trailer_end_ok,
        "recoverable": report.recoverable(),
    })
}

fn print_recovery_report(report: &pliczip::recovery::VerifyReport, quiet: bool) {
    if report.is_clean() {
        if !quiet {
            println!(
                "  запись для восстановления цела: {} данных и {} parity",
                plural(report.data_shards as usize, "кусок", "куска", "кусков"),
                report.parity_shards
            );
        }
        return;
    }
    eprintln!(
        "pliczip: запись для восстановления: повреждено кусков данных — {}, parity — {}",
        report.bad_data.len(),
        report.bad_parity.len()
    );
    if !report.table_ok {
        eprintln!("  таблица хешей не сходится с корнем Меркла");
    }
    if !report.trailer_copy_ok || !report.trailer_end_ok {
        eprintln!("  один из трейлеров повреждён (используется уцелевший)");
    }
    let shown: Vec<String> = report
        .bad_data
        .iter()
        .take(10)
        .map(u64::to_string)
        .collect();
    if !shown.is_empty() {
        eprintln!(
            "  повреждённые куски данных: {}{}",
            shown.join(", "),
            if report.bad_data.len() > shown.len() {
                ", …"
            } else {
                ""
            }
        );
    }
    if report.recoverable() {
        eprintln!("  parity хватает: запустите `pliczip repair` для восстановления");
    } else {
        eprintln!("  parity не хватает, чтобы восстановить все повреждённые куски");
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

/// Читает произвольный бинарный keyfile и кодирует его без потери байтов.
fn password_from_keyfile(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("не удалось прочитать keyfile {:?}", path))?;
    if bytes.is_empty() {
        bail!("keyfile {:?} пуст", path);
    }
    Ok(Zeroizing::new(bytes))
}

fn external_kind(path: &Path) -> Result<Option<external::Kind>> {
    if path.exists() {
        external::detect(path)
    } else {
        Ok(None)
    }
}

fn external_password_bytes(
    password: Option<&Zeroizing<String>>,
    keyfile: Option<&Path>,
) -> Result<Option<Zeroizing<Vec<u8>>>> {
    if let Some(path) = keyfile {
        return Ok(Some(password_from_keyfile(path)?));
    }
    Ok(password.map(|value| Zeroizing::new(value.as_bytes().to_vec())))
}

fn print_external_list(
    archive: &Path,
    kind: external::Kind,
    entries: &[external::Entry],
    long: bool,
    ron: bool,
) -> Result<()> {
    if ron {
        let document = serde_json::json!({
            "archive": archive.to_string_lossy(),
            "format": kind.name(),
            "entries": entries.iter().map(|entry| serde_json::json!({
                "path": entry.path,
                "kind": if entry.is_dir { "dir" } else { "file" },
                "size": entry.size,
            })).collect::<Vec<_>>(),
        });
        println!("{}", ron_pretty(&document)?);
        return Ok(());
    }

    println!("Архив: {} ({})", archive.display(), kind.name());
    println!();
    if long {
        println!("  {}  Путь", pad_left("Размер", 10));
    } else {
        println!("  Путь");
    }
    for entry in entries {
        let name = if entry.is_dir {
            format!("{}/", entry.path)
        } else {
            entry.path.clone()
        };
        if long {
            println!("  {}  {}", pad_left(&human_bytes(entry.size), 10), name);
        } else {
            println!("  {}", name);
        }
    }
    println!();
    println!(
        "  итого: {}",
        plural(entries.len(), "элемент", "элемента", "элементов")
    );
    Ok(())
}

fn open_archive(
    archive: &Path,
    password: Option<&Zeroizing<String>>,
    keyfile: Option<&Path>,
) -> Result<Archive> {
    if password.is_some() && keyfile.is_some() {
        bail!("нельзя комбинировать --password/--password-file и --keyfile");
    }
    if let Some(keyfile_path) = keyfile {
        let key = password_from_keyfile(keyfile_path)?;
        return Archive::open_with_bytes(archive, &key);
    }
    Archive::open_with(archive, password.map(|value| value.as_str()))
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
    keyfile: Option<&Path>,
) -> Result<Option<Zeroizing<String>>> {
    if (value.is_some() || file.is_some()) && keyfile.is_some() {
        bail!("нельзя комбинировать --password/--password-file и --keyfile");
    }
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
fn password_for_open(
    value: Option<&str>,
    file: Option<&Path>,
    keyfile: Option<&Path>,
) -> Result<Option<Zeroizing<String>>> {
    if (value.is_some() || file.is_some()) && keyfile.is_some() {
        bail!("нельзя комбинировать --password/--password-file и --keyfile");
    }
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
