//! Политика путей: одна строгая проверка для записи и для чтения.
//!
//! Архив должен распаковываться одинаково на Windows, macOS и Linux, поэтому
//! формат принимает только «портативные» пути. Это же закрывает классические
//! атаки вида zip-slip (`../../secret`), абсолютных путей, ADS на NTFS
//! (`file.txt:stream`) и зарезервированных имён Windows (`CON`, `NUL`).
//!
//! Проверка вызывается дважды: при создании архива (чтобы не записать мусор) и
//! при открытии (чтобы не распаковать мусор, даже если файл пришёл извне).

use anyhow::{bail, ensure, Context, Result};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Максимальная длина пути внутри архива.
pub const MAX_PATH_LEN: usize = 4096;

/// Максимальная длина одного сегмента.
pub const MAX_SEGMENT_LEN: usize = 255;

/// Символы, недопустимые в пути на Windows и потому запрещённые в архиве.
const FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// Зарезервированные имена Windows (сравнение регистронезависимое).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3",
    "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
    "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Проверяет путь элемента архива и нормализует его вид.
///
/// Принимает только относительные пути с разделителем `/`, без `.` и `..`,
/// без пустых сегментов, без управляющих и запрещённых символов.
pub fn validate_rel_path(raw: &str) -> Result<()> {
    ensure!(!raw.is_empty(), "пустой путь");
    ensure!(
        raw.len() <= MAX_PATH_LEN,
        "путь длиннее {} символов",
        MAX_PATH_LEN
    );
    ensure!(!raw.starts_with('/'), "абсолютный путь запрещён: {:?}", raw);
    ensure!(
        !raw.contains('\\'),
        "обратный слэш в пути запрещён, используйте '/': {:?}",
        raw
    );

    if let Some(ch) = raw.chars().find(|c| c.is_control()) {
        bail!("управляющий символ {:?} в пути {:?}", ch, raw);
    }
    if let Some(ch) = raw.chars().find(|c| FORBIDDEN_CHARS.contains(c)) {
        bail!("недопустимый для Windows символ {:?} в пути {:?}", ch, raw);
    }

    for segment in raw.split('/') {
        validate_segment(segment, raw)?;
    }

    Ok(())
}

fn validate_segment(segment: &str, full: &str) -> Result<()> {
    ensure!(!segment.is_empty(), "пустой сегмент пути в {:?}", full);
    ensure!(
        segment != "." && segment != "..",
        "переход по каталогам запрещён: {:?}",
        full
    );
    ensure!(
        segment.len() <= MAX_SEGMENT_LEN,
        "сегмент пути длиннее {} байт в {:?}",
        MAX_SEGMENT_LEN,
        full
    );
    ensure!(
        !segment.ends_with('.') && !segment.ends_with(' '),
        "сегмент не может заканчиваться точкой или пробелом (Windows обрежет): {:?}",
        full
    );

    // `CON`, `con.txt`, `NUL.tar.gz` — всё это зарезервированные имена.
    let stem = segment.split('.').next().unwrap_or(segment);
    if RESERVED_NAMES.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        bail!("зарезервированное имя Windows {:?} в пути {:?}", stem, full);
    }

    Ok(())
}

/// Превращает относительный путь файловой системы в путь архива.
pub fn path_to_archive_string(path: &Path) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part
                    .to_str()
                    .with_context(|| format!("имя {:?} не является корректным UTF-8", part))?
                    .to_string();
                parts.push(text);
            }
            Component::CurDir => {}
            _ => bail!("путь {:?} должен быть относительным", path),
        }
    }
    ensure!(!parts.is_empty(), "пустой относительный путь");
    let joined = parts.join("/");
    validate_rel_path(&joined)?;
    Ok(joined)
}

/// Безопасно склеивает корень распаковки с путём из архива.
///
/// Проверка путей уже гарантирует отсутствие `..`, но склейка дублирует
/// инвариант на уровне компонентов — на случай будущих изменений в формате.
pub fn join_safe(root: &Path, rel: &str) -> Result<PathBuf> {
    validate_rel_path(rel)?;
    let mut path = root.to_path_buf();
    for segment in rel.split('/') {
        path.push(segment);
    }
    let mut depth = 0i32;
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                cleaned.push(component.as_os_str());
                depth = 0;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    bail!("путь {:?} выходит за пределы каталога распаковки", rel);
                }
                depth -= 1;
                cleaned.pop();
            }
            Component::Normal(part) => {
                cleaned.push(part);
                depth += 1;
            }
        }
    }
    Ok(cleaned)
}

/// Проверяет, что цель символической ссылки не выводит за пределы распаковки.
///
/// Цель разрешается относительно каталога самой ссылки, поэтому `../` допустим
/// ровно столько раз, сколько уровней уже есть над ссылкой.
pub fn validate_link_target(entry_path: &str, target: &str) -> Result<()> {
    ensure!(!target.is_empty(), "пустая цель ссылки");
    ensure!(
        target.len() <= MAX_PATH_LEN,
        "цель ссылки длиннее {} символов",
        MAX_PATH_LEN
    );
    ensure!(
        !target.chars().any(|c| c.is_control()),
        "управляющий символ в цели ссылки {:?}",
        target
    );

    // Абсолютные цели: `/etc/passwd`, `C:\Windows`, UNC.
    if target.starts_with('/') || target.starts_with('\\') {
        bail!("абсолютная цель ссылки запрещена: {:?}", target);
    }
    let bytes = target.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        bail!("цель ссылки с буквой диска запрещена: {:?}", target);
    }

    // Глубина каталога, в котором лежит ссылка.
    let mut depth = entry_path.split('/').count() as i64 - 1;
    for segment in target.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    bail!(
                        "цель ссылки выводит за пределы распаковки: {:?} -> {:?}",
                        entry_path,
                        target
                    );
                }
            }
            _ => depth += 1,
        }
    }

    Ok(())
}

/// Сколько ссылок подряд можно пройти при разрешении цели (как `MAXSYMLINKS`).
pub const MAX_LINK_HOPS: usize = 40;

/// Проверяет, что ссылка не выводит за пределы распаковки с учётом **других
/// ссылок из того же архива**.
///
/// [`validate_link_target`] смотрит на каждую ссылку по отдельности и не видит
/// цепочек: `a/l1 -> ..` и `l3 -> a/l1/..` поодиночке безопасны, но вместе `l3`
/// указывает на каталог выше корня распаковки. Здесь цель разрешается так же,
/// как это сделает ОС: если промежуточный сегмент сам является ссылкой из
/// архива, путь продолжается от её цели.
///
/// `links` — все ссылки архива: путь элемента в нижнем регистре → цель.
pub fn validate_link_chain(
    entry_path: &str,
    target: &str,
    links: &HashMap<String, String>,
) -> Result<()> {
    let mut stack: Vec<&str> = entry_path.split('/').filter(|s| !s.is_empty()).collect();
    stack.pop(); // сама ссылка; разрешаем от её каталога
    let mut pending: Vec<&str> = target.split(['/', '\\']).rev().collect();
    let mut hops = 0usize;

    while let Some(segment) = pending.pop() {
        match segment {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    bail!(
                        "цепочка ссылок выводит за пределы распаковки: {:?} -> {:?}",
                        entry_path,
                        target
                    );
                }
            }
            name => {
                stack.push(name);
                let current = stack.join("/").to_lowercase();
                if let Some(next) = links.get(&current) {
                    hops += 1;
                    ensure!(
                        hops <= MAX_LINK_HOPS,
                        "слишком длинная или зацикленная цепочка ссылок: {:?}",
                        entry_path
                    );
                    let bytes = next.as_bytes();
                    if next.starts_with('/')
                        || next.starts_with('\\')
                        || (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
                    {
                        bail!(
                            "цепочка ссылок ведёт к абсолютному пути: {:?} -> {:?}",
                            entry_path,
                            next
                        );
                    }
                    stack.pop();
                    pending.extend(next.split(['/', '\\']).rev());
                }
            }
        }
    }
    Ok(())
}

/// Вычисляет глубину пути в архиве (число сегментов).
pub fn path_depth(rel: &str) -> usize {
    rel.split('/').filter(|s| !s.is_empty()).count()
}

/// Готовит путь к обращению к файловой системе.
///
/// Windows без включённого `LongPathsEnabled` режет пути на 260 символах, а
/// формат разрешает до 4096. Длинные пути переводим в verbatim-форму `\\?\…`,
/// которую Win32 не ограничивает. Короткие не трогаем: лишний префикс ломает
/// некоторые сторонние API.
#[cfg(windows)]
pub fn native_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") || text.len() < 248 {
        return path.to_path_buf();
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(dir) => dir.join(path),
            Err(_) => return path.to_path_buf(),
        }
    };
    let absolute = absolute.to_string_lossy();
    match absolute.strip_prefix(r"\\") {
        Some(rest) => PathBuf::from(format!(r"\\?\UNC\{}", rest)),
        None => PathBuf::from(format!(r"\\?\{}", absolute)),
    }
}

/// На остальных платформах ограничения длины нет.
#[cfg(not(windows))]
pub fn native_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths() {
        for p in ["a.txt", "dir/a.txt", "a/b/c/d.bin", "фото/2024 лето.jpg"] {
            assert!(validate_rel_path(p).is_ok(), "должен принять {:?}", p);
        }
    }

    #[test]
    fn rejects_traversal() {
        for p in ["../a", "a/../../b", "a/../b", "..", "."] {
            assert!(validate_rel_path(p).is_err(), "должен отвергнуть {:?}", p);
        }
    }

    #[test]
    fn rejects_absolute_and_windows_forms() {
        for p in ["/etc/passwd", "C:/Windows", "a\\b", "a//b", "a/", "a/b/"] {
            assert!(validate_rel_path(p).is_err(), "должен отвергнуть {:?}", p);
        }
    }

    #[test]
    fn rejects_windows_specials() {
        for p in [
            "CON",
            "con.txt",
            "dir/NUL",
            "a:b",
            "file.txt:stream",
            "bad?.txt",
            "x*",
        ] {
            assert!(validate_rel_path(p).is_err(), "должен отвергнуть {:?}", p);
        }
    }

    #[test]
    fn rejects_trailing_dot_or_space() {
        for p in ["a./b", "a /b", "dir./x"] {
            assert!(validate_rel_path(p).is_err(), "должен отвергнуть {:?}", p);
        }
    }

    #[test]
    fn join_safe_stays_inside() {
        let root = Path::new("out");
        let joined = join_safe(root, "a/b.txt").unwrap();
        assert_eq!(joined, root.join("a").join("b.txt"));
        assert!(join_safe(root, "../evil").is_err());
        assert!(join_safe(root, "a/../../evil").is_err());
    }

    #[test]
    fn link_targets() {
        assert!(validate_link_target("dir/link", "file.txt").is_ok());
        assert!(validate_link_target("dir/link", "../file.txt").is_ok());
        assert!(validate_link_target("dir/link", "../../file.txt").is_err());
        assert!(validate_link_target("link", "../file.txt").is_err());
        assert!(validate_link_target("dir/link", "/etc/passwd").is_err());
        assert!(validate_link_target("dir/link", "C:\\Windows").is_err());
    }

    #[test]
    fn link_chains() {
        let links: HashMap<String, String> = [
            ("a/l1".to_string(), "..".to_string()),
            ("l3".to_string(), "a/l1/..".to_string()),
            ("loop1".to_string(), "loop2".to_string()),
            ("loop2".to_string(), "loop1".to_string()),
            ("d/abs".to_string(), "/etc".to_string()),
            ("docs".to_string(), "a".to_string()),
        ]
        .into_iter()
        .collect();
        // Поодиночке обе ссылки проходят старую проверку...
        assert!(validate_link_target("a/l1", "..").is_ok());
        assert!(validate_link_target("l3", "a/l1/..").is_ok());
        // ...но цепочка выводит наружу.
        assert!(validate_link_chain("a/l1", "..", &links).is_ok());
        assert!(validate_link_chain("l3", "a/l1/..", &links).is_err());
        assert!(validate_link_chain("loop1", "loop2", &links).is_err());
        assert!(validate_link_chain("x", "d/abs/passwd", &links).is_err());
        assert!(validate_link_chain("y", "docs/file.txt", &links).is_ok());
        assert!(validate_link_chain("A/L1", "..", &links).is_ok());
    }
}
