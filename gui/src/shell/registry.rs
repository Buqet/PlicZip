//! План записей реестра для Проводника — чистые данные, без WinAPI.
//!
//! Всё пишется в `HKCU\Software\Classes`: права администратора не нужны,
//! другие пользователи не затрагиваются. Пункты собраны в каскадное подменю
//! «PlicZip» (`SubCommands` + вложенный `shell`), как у 7-Zip.
//!
//! На архивах `.plz`:   PlicZip ▸ Распаковать сюда / Распаковать в «имя\» / Проверить
//! На файлах и папках:  PlicZip ▸ Добавить в архив… / Сжать в .plz
//!
//! В Windows 11 классические пункты показываются в «Показать дополнительные
//! параметры» (Shift+F10); для верхнего уровня нового меню нужен COM-обработчик
//! `IExplorerCommand` и пакет MSIX — это отдельная задача.

use std::path::Path;

use super::args;

/// Корень, относительно которого заданы все ключи (внутри `HKEY_CURRENT_USER`).
pub const CLASSES: &str = r"Software\Classes";
/// Программный идентификатор типа файла.
pub const PROG_ID: &str = "PlicZip.Archive";
/// Расширение архивов.
pub const EXT: &str = ".plz";
/// Каноническое имя подменю. Одинаковое у `.plz` и у `*`: Проводник
/// показывает глагол один раз, и у архива остаётся только его подменю.
pub const VERB: &str = "PlicZip";

/// Одно строковое значение реестра: ключ относительно [`CLASSES`],
/// имя значения (`None` — значение по умолчанию) и данные.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub key: String,
    pub name: Option<&'static str>,
    pub data: String,
}

/// Ключи, которые целиком принадлежат PlicZip и удаляются при отключении.
pub fn owned_keys() -> Vec<String> {
    vec![
        PROG_ID.to_string(),
        format!(r"*\shell\{VERB}"),
        format!(r"Directory\shell\{VERB}"),
    ]
}

/// Командная строка пункта меню: `"C:\…\pliczip-gui.exe" --ключ "%1"`.
pub fn command(exe: &Path, flag: Option<&str>) -> String {
    let exe = exe.display();
    match flag {
        Some(flag) => format!("\"{exe}\" {flag} \"%1\""),
        None => format!("\"{exe}\" \"%1\""),
    }
}

/// Указывает ли записанная команда на этот исполняемый файл.
pub fn command_targets(cmd: &str, exe: &Path) -> bool {
    let quoted = format!("\"{}\"", exe.display());
    cmd.to_lowercase().starts_with(&quoted.to_lowercase())
}

/// Все значения, которые нужно записать для `exe`.
pub fn plan(exe: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let mut set = |key: String, name: Option<&'static str>, data: String| {
        out.push(Value { key, name, data })
    };
    let icon = format!("\"{}\",0", exe.display());

    // Тип файла и ассоциация.
    set(EXT.into(), None, PROG_ID.into());
    set(format!(r"{EXT}\OpenWithProgids"), Some(PROG_ID), String::new());
    set(PROG_ID.into(), None, "Архив PlicZip".into());
    set(format!(r"{PROG_ID}\DefaultIcon"), None, icon.clone());
    set(format!(r"{PROG_ID}\shell"), None, "open".into());
    set(format!(r"{PROG_ID}\shell\open"), None, "Открыть в PlicZip".into());
    set(format!(r"{PROG_ID}\shell\open\command"), None, command(exe, None));

    // Подменю на архиве.
    let archive_items = [
        ("1_extract_here", "Распаковать сюда", args::EXTRACT_HERE),
        ("2_extract_to", "Распаковать в папку с именем архива", args::EXTRACT_TO),
        ("3_test", "Проверить целостность", args::TEST),
    ];
    submenu(&mut set, &format!(r"{PROG_ID}\shell\{VERB}"), &icon, exe, &archive_items);

    // Подменю на любых файлах и на папках.
    let add_items = [
        ("1_add", "Добавить в архив…", args::ADD),
        ("2_compress", "Сжать в .plz", args::COMPRESS),
    ];
    for base in [r"*\shell", r"Directory\shell"] {
        submenu(&mut set, &format!(r"{base}\{VERB}"), &icon, exe, &add_items);
    }
    out
}

fn submenu(
    set: &mut impl FnMut(String, Option<&'static str>, String),
    root: &str,
    icon: &str,
    exe: &Path,
    items: &[(&str, &str, &str)],
) {
    set(root.into(), Some("MUIVerb"), "PlicZip".into());
    set(root.into(), Some("Icon"), icon.into());
    // Пустой SubCommands включает каскад из вложенного ключа `shell`.
    set(root.into(), Some("SubCommands"), String::new());
    set(root.into(), Some("MultiSelectModel"), "Player".into());
    for (id, title, flag) in items {
        let key = format!(r"{root}\shell\{id}");
        set(key.clone(), Some("MUIVerb"), (*title).into());
        // Player — пункт доступен при любом числе выделенных элементов
        // (у Document предел — 15). Процессы потом собирает `gather`.
        set(key.clone(), Some("MultiSelectModel"), "Player".into());
        set(format!(r"{key}\command"), None, command(exe, Some(flag)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\PlicZip\pliczip-gui.exe")
    }

    fn get<'a>(plan: &'a [Value], key: &str, name: Option<&str>) -> Option<&'a str> {
        plan.iter()
            .find(|v| v.key == key && v.name == name)
            .map(|v| v.data.as_str())
    }

    #[test]
    fn association_points_to_exe() {
        let plan = plan(&exe());
        assert_eq!(get(&plan, ".plz", None), Some(PROG_ID));
        assert_eq!(
            get(&plan, r"PlicZip.Archive\shell\open\command", None),
            Some(r#""C:\Program Files\PlicZip\pliczip-gui.exe" "%1""#)
        );
    }

    #[test]
    fn archive_submenu_has_extract_and_test() {
        let plan = plan(&exe());
        let root = r"PlicZip.Archive\shell\PlicZip";
        assert_eq!(get(&plan, root, Some("SubCommands")), Some(""));
        assert_eq!(
            get(&plan, &format!(r"{root}\shell\1_extract_here\command"), None),
            Some(r#""C:\Program Files\PlicZip\pliczip-gui.exe" --extract-here "%1""#)
        );
        assert!(get(&plan, &format!(r"{root}\shell\2_extract_to\command"), None).unwrap().contains("--extract-to"));
        assert!(get(&plan, &format!(r"{root}\shell\3_test\command"), None).unwrap().contains("--test"));
    }

    #[test]
    fn files_and_folders_get_add_items() {
        let plan = plan(&exe());
        for root in [r"*\shell\PlicZip", r"Directory\shell\PlicZip"] {
            assert_eq!(get(&plan, root, Some("MUIVerb")), Some("PlicZip"));
            assert!(get(&plan, &format!(r"{root}\shell\1_add\command"), None).unwrap().contains("--add"));
            assert!(get(&plan, &format!(r"{root}\shell\2_compress\command"), None).unwrap().contains("--compress"));
        }
    }

    #[test]
    fn every_written_key_is_removed_on_unregister() {
        // Всё, кроме значений у `.plz`, лежит внутри принадлежащих нам ключей.
        let owned = owned_keys();
        for v in plan(&exe()) {
            if v.key == EXT || v.key.starts_with(&format!(r"{EXT}\")) {
                continue;
            }
            assert!(
                owned.iter().any(|k| v.key == *k || v.key.starts_with(&format!(r"{k}\"))),
                "ключ {} не удалится при отключении",
                v.key
            );
        }
    }

    #[test]
    fn detects_own_command() {
        let cmd = command(&exe(), None);
        assert!(command_targets(&cmd, &exe()));
        assert!(command_targets(&cmd.to_uppercase(), &exe()));
        assert!(!command_targets(&cmd, Path::new(r"D:\old\pliczip-gui.exe")));
    }
}
