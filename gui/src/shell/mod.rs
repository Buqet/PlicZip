//! Интеграция с оболочкой ОС: разбор аргументов запуска, ассоциация `.plz`
//! и контекстное меню Проводника.
//!
//! Модуль не зависит от egui, поэтому вся логика проверяется тестами на любой
//! ОС: аргументы — [`args`], сбор множественного выделения — [`gather`], план
//! записей реестра — чистые данные в [`registry`]. Запись в реестр — тонкий
//! слой над WinAPI в `win.rs`, он собирается только под Windows.

// Вне Windows план реестра нужен только тестам.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod args;
pub mod gather;
pub mod registry;
#[cfg(windows)]
mod win;

pub use args::Launch;

/// Есть ли интеграция с оболочкой на этой ОС.
pub const SUPPORTED: bool = cfg!(windows);

/// Состояние интеграции в реестре текущего пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Ничего не зарегистрировано.
    Off,
    /// Зарегистрирована эта копия программы.
    On,
    /// Зарегистрирована другая копия (перенесли или обновили в другой каталог).
    OtherCopy(String),
}

/// Включает ассоциацию `.plz` и контекстное меню для текущего исполняемого файла.
pub fn register() -> Result<(), String> {
    #[cfg(windows)]
    {
        win::register(&current_exe()?)
    }
    #[cfg(not(windows))]
    {
        Err(UNSUPPORTED.into())
    }
}

/// Убирает всё, что записал [`register`]. Чужие ассоциации `.plz` не трогает.
pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    {
        win::unregister()
    }
    #[cfg(not(windows))]
    {
        Err(UNSUPPORTED.into())
    }
}

/// Текущее состояние интеграции.
pub fn status() -> Status {
    #[cfg(windows)]
    {
        match (win::registered_command(), current_exe()) {
            (None, _) => Status::Off,
            (Some(cmd), Ok(exe)) if registry::command_targets(&cmd, &exe) => Status::On,
            (Some(cmd), _) => Status::OtherCopy(cmd),
        }
    }
    #[cfg(not(windows))]
    {
        Status::Off
    }
}

#[cfg(windows)]
fn current_exe() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("не удалось определить путь программы: {e}"))
}

#[cfg(not(windows))]
const UNSUPPORTED: &str = "интеграция с контекстным меню пока есть только для Проводника Windows";
