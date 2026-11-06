//! Интеграция графической оболочки с контекстным меню Windows Explorer.
//!
//! Регистрация выполняется только для текущего пользователя (HKCU), поэтому
//! права администратора не нужны и системный реестр не изменяется.

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
const ROOT: &str = r"HKCU\Software\Classes";

#[cfg(windows)]
fn reg_add(key: &str, value: &str) -> Result<(), String> {
    let status = std::process::Command::new("reg")
        .args(["add", key, "/ve", "/t", "REG_SZ", "/d", value, "/f"])
        .status()
        .map_err(|err| format!("не удалось запустить reg.exe: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg.exe завершился с кодом {status}"))
    }
}

#[cfg(windows)]
fn reg_add_named(key: &str, name: &str, value: &str) -> Result<(), String> {
    let status = std::process::Command::new("reg")
        .args(["add", key, "/v", name, "/t", "REG_SZ", "/d", value, "/f"])
        .status()
        .map_err(|err| format!("не удалось запустить reg.exe: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg.exe завершился с кодом {status}"))
    }
}

#[cfg(windows)]
fn reg_delete(key: &str) -> Result<(), String> {
    let status = std::process::Command::new("reg")
        .args(["delete", key, "/f"])
        .status()
        .map_err(|err| format!("не удалось запустить reg.exe: {err}"))?;
    if status.success() || status.code() == Some(1) {
        Ok(())
    } else {
        Err(format!("reg.exe завершился с кодом {status}"))
    }
}

#[cfg(windows)]
fn command(exe: &Path, args: &str) -> String {
    // Кавычки передаются в значение реестра как обычные символы: Explorer
    // подставит %1 уже после разбора команды.
    format!(r#""{}" {}"#, exe.display(), args)
}

/// Зарегистрировать команды PlicZip в контекстном меню текущего пользователя.
#[cfg(windows)]
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("не удалось определить путь PlicZip: {err}"))?;
    let open_command = command(&exe, "--context-open \"%1\"");
    let add_command = command(&exe, "--context-add \"%1\"");
    let open_key = format!(r#"{ROOT}\SystemFileAssociations\.plz\shell\PlicZip"#);
    let add_key = format!(r#"{ROOT}\*\shell\PlicZipAdd"#);
    let dir_key = format!(r#"{ROOT}\Directory\shell\PlicZipAdd"#);

    reg_add(&format!(r#"{open_key}\command"#), &open_command)?;
    reg_add(&format!(r#"{add_key}\command"#), &add_command)?;
    reg_add(&format!(r#"{dir_key}\command"#), &add_command)?;
    reg_add_named(&open_key, "MUIVerb", "Открыть в PlicZip")?;
    reg_add_named(&add_key, "MUIVerb", "Добавить в архив PlicZip")?;
    reg_add_named(&dir_key, "MUIVerb", "Добавить в архив PlicZip")?;
    Ok(())
}

/// Удалить команды PlicZip из контекстного меню текущего пользователя.
#[cfg(windows)]
pub fn uninstall() -> Result<(), String> {
    for key in [
        format!(r#"{ROOT}\SystemFileAssociations\.plz\shell\PlicZip"#),
        format!(r#"{ROOT}\*\shell\PlicZipAdd"#),
        format!(r#"{ROOT}\Directory\shell\PlicZipAdd"#),
    ] {
        reg_delete(&key)?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn install() -> Result<(), String> {
    Err("интеграция с Explorer доступна только в Windows".into())
}

#[cfg(not(windows))]
pub fn uninstall() -> Result<(), String> {
    Err("интеграция с Explorer доступна только в Windows".into())
}

/// Обновить кэш значков и контекстных меню Explorer.
#[cfg(windows)]
pub fn refresh() {
    let _ = std::process::Command::new("ie4uinit.exe")
        .arg("-show")
        .status();
}

#[cfg(not(windows))]
pub fn refresh() {}
