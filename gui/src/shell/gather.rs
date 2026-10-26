//! Сбор множественного выделения в одно окно.
//!
//! Статический пункт контекстного меню Проводник вызывает отдельно для каждого
//! выделенного элемента: 10 файлов → 10 процессов `pliczip-gui --add <файл>`.
//! Чтобы получился один архив, процессы договариваются через каталог во
//! временной папке:
//!
//! 1. каждый кладёт свои пути в файл `<pid>-<время>.paths`;
//! 2. кто первым создал `leader.lock`, тот ведущий: ждёт, пока остальные
//!    успеют записаться, забирает все `.paths` и открывает окно;
//! 3. остальные ждут, пока их файл заберут, и тихо выходят. Если ведущий
//!    уже закончил, а файл остался — ведущим становится следующий.
//!
//! Брошенная блокировка (ведущий упал) старше [`STALE`] снимается.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Сколько ведущий ждёт остальных.
pub const SETTLE: Duration = Duration::from_millis(400);
/// Сколько ведомый ждёт, пока его заберут, прежде чем открыть окно сам.
const FOLLOWER_TIMEOUT: Duration = Duration::from_secs(5);
/// Возраст, после которого блокировка считается брошенной.
const STALE: Duration = Duration::from_secs(15);
const LOCK: &str = "leader.lock";
const EXT: &str = "paths";

/// Объединяет пути всех одновременно запущенных процессов с тем же `kind`.
///
/// `Some(пути)` — этот процесс открывает окно; `None` — пути переданы другому
/// процессу, этот должен выйти. При любой ошибке файловой системы возвращает
/// собственные пути: лучше лишнее окно, чем потерянный файл.
pub fn collect(kind: &str, own: Vec<PathBuf>) -> Option<Vec<PathBuf>> {
    let own: Vec<PathBuf> = own
        .into_iter()
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    let dir = std::env::temp_dir().join("pliczip-gather").join(kind);
    match gather(&dir, &own, SETTLE) {
        Ok(result) => result,
        Err(_) => Some(own),
    }
}

/// Протокол сбора в каталоге `dir`; см. описание модуля.
pub fn gather(dir: &Path, own: &[PathBuf], settle: Duration) -> io::Result<Option<Vec<PathBuf>>> {
    // Путь, который нельзя записать как UTF-8, формат всё равно не примет —
    // не участвуем в сборе, пусть окно покажет ошибку для него отдельно.
    let Some(lines) = own.iter().map(|p| p.to_str()).collect::<Option<Vec<_>>>() else {
        return Ok(Some(own.to_vec()));
    };
    fs::create_dir_all(dir)?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let token = format!("{}-{nanos}", std::process::id());
    let mine = dir.join(format!("{token}.{EXT}"));
    let tmp = dir.join(format!("{token}.tmp"));
    fs::write(&tmp, lines.join("\n"))?;
    fs::rename(&tmp, &mine)?;

    let lock = dir.join(LOCK);
    let deadline = Instant::now() + FOLLOWER_TIMEOUT;
    loop {
        if try_lock(&lock)? {
            if !mine.exists() {
                // Нас уже забрал предыдущий ведущий.
                let _ = fs::remove_file(&lock);
                return Ok(None);
            }
            std::thread::sleep(settle);
            let result = take_all(dir);
            let _ = fs::remove_file(&lock);
            return result.map(Some);
        }
        if !mine.exists() {
            return Ok(None);
        }
        if is_stale(&lock) {
            let _ = fs::remove_file(&lock);
            continue;
        }
        if Instant::now() >= deadline {
            // Ведущий завис: забираем свой файл назад и открываемся сами.
            return Ok(match fs::remove_file(&mine) {
                Ok(()) => Some(own.to_vec()),
                Err(_) => None, // Успели забрать в последний момент.
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn try_lock(lock: &Path) -> io::Result<bool> {
    match fs::OpenOptions::new().write(true).create_new(true).open(lock) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        // На Windows файл, который как раз удаляют, отвечает «нет доступа».
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Ok(false),
        Err(e) => Err(e),
    }
}

fn is_stale(lock: &Path) -> bool {
    fs::metadata(lock)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age > STALE)
        .unwrap_or(false)
}

/// Забирает все `.paths`: читает, удаляет, возвращает пути без повторов.
fn take_all(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXT))
        .collect();
    files.sort();
    let mut paths: Vec<PathBuf> = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        if fs::remove_file(&file).is_err() {
            continue; // Забрал кто-то другой.
        }
        paths.extend(text.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pliczip-gather-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn single_process_gets_its_own_paths() {
        let dir = scratch("single");
        let got = gather(&dir, &[PathBuf::from("/a/b.txt")], Duration::from_millis(10)).unwrap();
        assert_eq!(got, Some(vec![PathBuf::from("/a/b.txt")]));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "после сбора каталог пуст");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_launches_merge_into_one_window() {
        let dir = scratch("many");
        let handles: Vec<_> = (0..12)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let own = vec![PathBuf::from(format!("/x/file{i:02}.txt"))];
                    gather(&dir, &own, Duration::from_millis(300)).unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let leaders: Vec<_> = results.into_iter().flatten().collect();
        assert_eq!(leaders.len(), 1, "окно открывает ровно один процесс");
        assert_eq!(leaders[0].len(), 12, "ни один путь не потерян");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stale_lock_is_taken_over() {
        let dir = scratch("stale");
        fs::create_dir_all(&dir).unwrap();
        let lock = dir.join(LOCK);
        fs::write(&lock, b"").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        let got = gather(&dir, &[PathBuf::from("/a")], Duration::from_millis(10)).unwrap();
        assert_eq!(got, Some(vec![PathBuf::from("/a")]));
        fs::remove_dir_all(&dir).unwrap();
    }
}
