use anyhow::{bail, Context, Result};
use clap::Parser;
use ron::ser::PrettyConfig;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MIN_SIZE: u64 = 100 * 1024 * 1024;

#[derive(Parser, Debug)]
#[command(about = "Бенчмарк PlicZip для бинарных файлов больше 100 MiB")]
struct Args {
    #[arg(long, value_name = "PATH")]
    pliczip: PathBuf,
    #[arg(long, value_name = "PATH", required = true)]
    input: Vec<PathBuf>,
    #[arg(long, value_name = "DIR")]
    output: PathBuf,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    repetitions: u32,
}

#[derive(Clone, Copy)]
struct Method {
    name: &'static str,
    flags: &'static [&'static str],
}

const METHODS: &[Method] = &[
    Method {
        name: "store",
        flags: &["--level", "0"],
    },
    Method {
        name: "deflate-q4",
        flags: &["--level", "4"],
    },
    Method {
        name: "brotli-q5",
        flags: &["--level", "5"],
    },
    Method {
        name: "brotli-q11",
        flags: &["--level", "11"],
    },
    Method {
        name: "density",
        flags: &["--density", "--level", "5"],
    },
];

#[derive(Serialize)]
struct InputInfo {
    path: String,
    name: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct Row {
    input: String,
    input_path: String,
    input_size_bytes: u64,
    input_sha256: String,
    method: String,
    repetition: u32,
    archive_size_bytes: u64,
    archive_ratio: f64,
    pack_seconds: f64,
    extract_seconds: f64,
    pack_mib_s: f64,
    extract_mib_s: f64,
    round_trip_sha256_ok: bool,
}

#[derive(Serialize)]
struct Summary {
    input: String,
    method: String,
    archive_size_bytes_median: f64,
    archive_ratio_median: f64,
    pack_seconds_median: f64,
    extract_seconds_median: f64,
    pack_mib_s_median: f64,
    extract_mib_s_median: f64,
}

#[derive(Serialize)]
struct Report {
    started_at_unix_seconds: u64,
    finished_at_unix_seconds: u64,
    pliczip: String,
    pliczip_version: String,
    repetitions: u32,
    minimum_input_size_bytes: u64,
    inputs: Vec<InputInfo>,
    methods: Vec<String>,
    all_round_trip_sha256_checks_passed: bool,
    rows: Vec<Row>,
    summaries: Vec<Summary>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let pliczip = fs::canonicalize(&args.pliczip)
        .with_context(|| format!("pliczip binary not found: {}", args.pliczip.display()))?;
    if !pliczip.is_file() {
        bail!("pliczip is not a regular file: {}", pliczip.display());
    }
    let inputs = validate_inputs(&args.input)?;
    fs::create_dir_all(&args.output)?;
    let started_at = now_seconds();
    let temp = tempfile_dir();
    fs::create_dir_all(&temp)?;
    let mut rows = Vec::new();

    for input in &inputs {
        let source = Path::new(&input.path);
        let source_size = input.size_bytes;
        for method in METHODS {
            for repetition in 1..=args.repetitions {
                let work = temp.join(format!("{}-{}-{}", input.name, method.name, repetition));
                fs::create_dir_all(&work)?;
                let archive = work.join("archive.plz");
                let extracted = work.join("extracted");
                fs::create_dir_all(&extracted)?;
                let mut pack_command = Command::new(&pliczip);
                pack_command.args(["--quiet", "add"]);
                pack_command.args(method.flags);
                pack_command.arg(&archive).arg(source);
                let pack_seconds = run_command(&mut pack_command)?;
                let archive_size = fs::metadata(&archive)?.len();

                let mut extract_command = Command::new(&pliczip);
                extract_command
                    .args(["--quiet", "extract"])
                    .arg(&archive)
                    .args(["--output"])
                    .arg(&extracted);
                let extract_seconds = run_command(&mut extract_command)?;
                let restored = extracted.join(&input.name);
                let restored_sha256 = sha256_file(&restored)?;
                let round_trip_ok = restored_sha256 == input.sha256;
                if !round_trip_ok {
                    bail!("SHA-256 mismatch for {}", source.display());
                }
                let pack_mib_s = source_size as f64 / 1024.0 / 1024.0 / pack_seconds;
                let extract_mib_s = source_size as f64 / 1024.0 / 1024.0 / extract_seconds;
                rows.push(Row {
                    input: input.name.clone(),
                    input_path: input.path.clone(),
                    input_size_bytes: source_size,
                    input_sha256: input.sha256.clone(),
                    method: method.name.to_owned(),
                    repetition,
                    archive_size_bytes: archive_size,
                    archive_ratio: archive_size as f64 / source_size as f64,
                    pack_seconds,
                    extract_seconds,
                    pack_mib_s,
                    extract_mib_s,
                    round_trip_sha256_ok: round_trip_ok,
                });
                println!(
                    "{}: {} #{} OK — {:.1} MiB, pack {:.1} MiB/s, extract {:.1} MiB/s",
                    input.name,
                    method.name,
                    repetition,
                    archive_size as f64 / 1024.0 / 1024.0,
                    pack_mib_s,
                    extract_mib_s
                );
            }
        }
    }
    let summaries = summarize(&inputs, &rows);
    let report = Report {
        started_at_unix_seconds: started_at,
        finished_at_unix_seconds: now_seconds(),
        pliczip: pliczip.display().to_string(),
        pliczip_version: command_output(&pliczip, &["--version"])?,
        repetitions: args.repetitions,
        minimum_input_size_bytes: MIN_SIZE,
        inputs,
        methods: METHODS
            .iter()
            .map(|method| method.name.to_owned())
            .collect(),
        all_round_trip_sha256_checks_passed: rows.iter().all(|row| row.round_trip_sha256_ok),
        rows,
        summaries,
    };
    let output = args.output.join("large-binary-results.ron");
    let text = ron::ser::to_string_pretty(&report, PrettyConfig::default())?;
    File::create(&output)?.write_all(format!("{text}\n").as_bytes())?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn validate_inputs(paths: &[PathBuf]) -> Result<Vec<InputInfo>> {
    paths
        .iter()
        .map(|path| {
            let path = fs::canonicalize(path)
                .with_context(|| format!("input not found: {}", path.display()))?;
            let metadata = fs::metadata(&path)?;
            if !metadata.is_file() {
                bail!("input is not a regular file: {}", path.display());
            }
            if metadata.len() <= MIN_SIZE {
                bail!("input must be larger than 100 MiB: {}", path.display());
            }
            let name = path
                .file_name()
                .context("input has no file name")?
                .to_string_lossy()
                .into_owned();
            Ok(InputInfo {
                path: path.display().to_string(),
                name,
                size_bytes: metadata.len(),
                sha256: sha256_file(&path)?,
            })
        })
        .collect()
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8 * 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn run_command(command: &mut Command) -> Result<f64> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let started = Instant::now();
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(started.elapsed().as_secs_f64().max(f64::EPSILON))
}

fn command_output(program: &Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(Command::new(program).args(args).output()?.stdout)?
            .trim()
            .to_owned(),
    )
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn summarize(inputs: &[InputInfo], rows: &[Row]) -> Vec<Summary> {
    let mut result = Vec::new();
    for input in inputs {
        for method in METHODS {
            let selected: Vec<&Row> = rows
                .iter()
                .filter(|row| row.input == input.name && row.method == method.name)
                .collect();
            let mut archive_sizes: Vec<f64> = selected
                .iter()
                .map(|row| row.archive_size_bytes as f64)
                .collect();
            let mut ratios: Vec<f64> = selected.iter().map(|row| row.archive_ratio).collect();
            let mut pack_seconds: Vec<f64> = selected.iter().map(|row| row.pack_seconds).collect();
            let mut extract_seconds: Vec<f64> =
                selected.iter().map(|row| row.extract_seconds).collect();
            let mut pack_speed: Vec<f64> = selected.iter().map(|row| row.pack_mib_s).collect();
            let mut extract_speed: Vec<f64> =
                selected.iter().map(|row| row.extract_mib_s).collect();
            result.push(Summary {
                input: input.name.clone(),
                method: method.name.to_owned(),
                archive_size_bytes_median: median(&mut archive_sizes),
                archive_ratio_median: median(&mut ratios),
                pack_seconds_median: median(&mut pack_seconds),
                extract_seconds_median: median(&mut extract_seconds),
                pack_mib_s_median: median(&mut pack_speed),
                extract_mib_s_median: median(&mut extract_speed),
            });
        }
    }
    result
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn tempfile_dir() -> PathBuf {
    std::env::temp_dir().join(format!("pliczip-large-benchmark-{}", std::process::id()))
}
