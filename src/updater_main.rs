//! Маленький одноразовый процесс обновления Windows.

#[cfg(not(windows))]
fn main() {
    eprintln!("PlicZip updater доступен только в Windows");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    let mut pid: Option<u32> = None;
    let mut setup: Option<PathBuf> = None;
    let mut restart: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wait-pid" => pid = args.next().and_then(|value| value.parse().ok()),
            "--setup" => setup = args.next().map(PathBuf::from),
            "--restart" => restart = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    let (Some(pid), Some(setup), Some(restart)) = (pid, setup, restart) else {
        eprintln!("usage: pliczip-updater --wait-pid PID --setup SETUP.exe --restart PlicZip.exe");
        std::process::exit(2);
    };

    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline && process_exists(pid) {
        thread::sleep(Duration::from_millis(250));
    }
    if process_exists(pid) {
        eprintln!("не удалось дождаться закрытия PlicZip");
        std::process::exit(1);
    }

    let status = match Command::new(&setup)
        .args(["/SILENT", "/NORESTART"])
        .status()
    {
        Ok(status) => status,
        Err(error) => {
            eprintln!("не удалось запустить Setup: {error}");
            std::process::exit(1);
        }
    };
    if !status.success() {
        eprintln!("Setup завершился с кодом {status}");
        std::process::exit(status.code().unwrap_or(1));
    }
    if let Err(error) = Command::new(&restart).spawn() {
        eprintln!("не удалось перезапустить PlicZip: {error}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.contains(&pid.to_string()) && !text.contains("No tasks")
        })
        .unwrap_or(false)
}
