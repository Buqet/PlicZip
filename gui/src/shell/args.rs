//! Аргументы запуска GUI.
//!
//! Двойной клик по `.plz` передаёт путь архива, пункты контекстного меню —
//! ключ действия и путь: `pliczip-gui --extract-here "C:\…\backup.plz"`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// С чем запущено окно.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Launch {
    /// Просто открыть окно.
    #[default]
    Empty,
    /// Открыть архив (ассоциация, «Открыть в PlicZip»).
    Open(PathBuf),
    /// Диалог создания архива с выбранными файлами («Добавить в архив…»).
    Add(Vec<PathBuf>),
    /// Сразу сжать рядом с настройками по умолчанию («Сжать в .plz»).
    Compress(Vec<PathBuf>),
    /// Распаковать в каталог архива.
    ExtractHere(PathBuf),
    /// Распаковать в подкаталог с именем архива.
    ExtractTo(PathBuf),
    /// Проверить целостность.
    Test(PathBuf),
    /// Встроить в Проводник и выйти (для установщика).
    Register,
    /// Убрать из Проводника и выйти (для деинсталлятора).
    Unregister,
}

pub const ADD: &str = "--add";
pub const COMPRESS: &str = "--compress";
pub const EXTRACT_HERE: &str = "--extract-here";
pub const EXTRACT_TO: &str = "--extract-to";
pub const TEST: &str = "--test";
pub const REGISTER: &str = "--register-shell";
pub const UNREGISTER: &str = "--unregister-shell";

/// Разбирает аргументы (без имени программы).
///
/// Ошибки не бывает: незнакомые ключи и действия без пути сводятся к прежнему
/// поведению — открыть первый аргумент, не начинающийся с `-`.
pub fn parse<I, S>(args: I) -> Launch
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let Some(first) = args.first() else {
        return Launch::Empty;
    };
    let rest: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
    let action = match first.to_str().unwrap_or_default() {
        ADD if !rest.is_empty() => Some(Launch::Add(rest)),
        COMPRESS if !rest.is_empty() => Some(Launch::Compress(rest)),
        EXTRACT_HERE if rest.len() == 1 => Some(Launch::ExtractHere(rest[0].clone())),
        EXTRACT_TO if rest.len() == 1 => Some(Launch::ExtractTo(rest[0].clone())),
        TEST if rest.len() == 1 => Some(Launch::Test(rest[0].clone())),
        REGISTER => Some(Launch::Register),
        UNREGISTER => Some(Launch::Unregister),
        _ => None,
    };
    action.unwrap_or_else(|| {
        args.iter()
            .find(|a| !a.to_string_lossy().starts_with('-'))
            .map(|a| Launch::Open(PathBuf::from(a)))
            .unwrap_or_default()
    })
}

/// Куда сохранить архив для «Сжать в .plz»: рядом с источником, его именем.
///
/// Один файл `отчёт.docx` → `отчёт.plz`, каталог `фото.2024` → `фото.2024.plz`
/// (у каталога расширения нет), несколько элементов → имя их общего каталога.
/// Существующий файл не перезаписывается: `имя (2).plz`, `имя (3).plz`…
pub fn archive_name_for(
    sources: &[PathBuf],
    is_dir: impl Fn(&Path) -> bool,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let fallback = || "архив".to_string();
    let (dir, base) = match sources {
        [] => (PathBuf::new(), fallback()),
        [one] => {
            let name = if is_dir(one) { one.file_name() } else { one.file_stem() };
            (
                one.parent().map(Path::to_path_buf).unwrap_or_default(),
                name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(fallback),
            )
        }
        [first, ..] => {
            let dir = first.parent().map(Path::to_path_buf).unwrap_or_default();
            let base = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(fallback);
            (dir, base)
        }
    };
    let mut candidate = dir.join(format!("{base}.plz"));
    let mut n = 2;
    while exists(&candidate) && n < 10_000 {
        candidate = dir.join(format!("{base} ({n}).plz"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn plain_path_opens_archive() {
        assert_eq!(parse(["backup.plz"]), Launch::Open(p("backup.plz")));
        assert_eq!(parse(Vec::<String>::new()), Launch::Empty);
        // Старое поведение: незнакомые ключи пропускаются.
        assert_eq!(parse(["--verbose", "a.plz"]), Launch::Open(p("a.plz")));
        assert_eq!(parse(["--verbose"]), Launch::Empty);
    }

    #[test]
    fn context_menu_actions() {
        assert_eq!(parse([ADD, "a.txt", "b"]), Launch::Add(vec![p("a.txt"), p("b")]));
        assert_eq!(parse([COMPRESS, "dir"]), Launch::Compress(vec![p("dir")]));
        assert_eq!(parse([EXTRACT_HERE, "a.plz"]), Launch::ExtractHere(p("a.plz")));
        assert_eq!(parse([EXTRACT_TO, "a.plz"]), Launch::ExtractTo(p("a.plz")));
        assert_eq!(parse([TEST, "a.plz"]), Launch::Test(p("a.plz")));
        assert_eq!(parse([REGISTER]), Launch::Register);
        assert_eq!(parse([UNREGISTER]), Launch::Unregister);
    }

    #[test]
    fn actions_without_paths_fall_back() {
        assert_eq!(parse([ADD]), Launch::Empty);
        assert_eq!(parse([EXTRACT_HERE]), Launch::Empty);
        // Два архива для распаковки — не наш формат вызова: открываем первый.
        assert_eq!(parse([EXTRACT_HERE, "a.plz", "b.plz"]), Launch::Open(p("a.plz")));
    }

    #[test]
    fn archive_names() {
        let no = |_: &Path| false;
        let dirs = |q: &Path| q.extension().is_none();
        assert_eq!(archive_name_for(&[p("/d/отчёт.docx")], dirs, no), p("/d/отчёт.plz"));
        assert_eq!(archive_name_for(&[p("/d/фото")], dirs, no), p("/d/фото.plz"));
        assert_eq!(archive_name_for(&[p("/d/фото.2024")], |_| true, no), p("/d/фото.2024.plz"));
        assert_eq!(archive_name_for(&[p("/work/a.txt"), p("/work/b.txt")], dirs, no), p("/work/work.plz"));
        assert_eq!(archive_name_for(&[p("/a.txt"), p("/b.txt")], dirs, no), p("/архив.plz"));
        assert_eq!(archive_name_for(&[], dirs, no), p("архив.plz"));
    }

    #[test]
    fn archive_name_never_overwrites() {
        let taken = [p("/d/x.plz"), p("/d/x (2).plz")];
        let exists = |q: &Path| taken.iter().any(|t| t == q);
        assert_eq!(archive_name_for(&[p("/d/x.txt")], |_| false, exists), p("/d/x (3).plz"));
    }
}
