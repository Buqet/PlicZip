//! Настройки GUI: акцент, тема, последняя папка. Сохраняются в JSON.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Сменный акцент интерфейса (решение № 3 из docs/design.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Accent {
    /// Латунь — акцент по умолчанию.
    #[default]
    Brass,
    /// Сталь — холодный сине-серый.
    Steel,
    /// Зелёный — тот же, что у «проверено».
    Green,
}

impl Accent {
    pub const ALL: [Accent; 3] = [Accent::Brass, Accent::Steel, Accent::Green];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Brass => "Латунь",
            Accent::Steel => "Сталь",
            Accent::Green => "Зелёный",
        }
    }
}

/// Настройки приложения.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Акцент интерфейса.
    #[serde(default)]
    pub accent: Accent,
    /// Тёмная тема; `false` — светлая.
    #[serde(default = "default_dark")]
    pub dark: bool,
}

fn default_dark() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accent: Accent::Brass,
            dark: true,
        }
    }
}

impl Config {
    /// Каталог конфигурации: `%APPDATA%/pliczip` или `~/.config/pliczip`.
    fn dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("pliczip"))
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config").join("pliczip"))
        }
    }

    pub fn path() -> Option<PathBuf> {
        Some(Self::dir()?.join("config.json"))
    }

    /// Читает конфиг; при любой ошибке — значения по умолчанию.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Пишет конфиг; ошибки молча игнорируются — настройки не критичны.
    pub fn save(&self) {
        let Some(dir) = Self::dir() else { return };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("config.json"), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_json() {
        let cfg = Config {
            accent: Accent::Steel,
            dark: false,
        };
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.accent, Accent::Steel);
        assert!(!back.dark);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.accent, Accent::Brass);
        assert!(cfg.dark);
    }

    #[test]
    fn accent_names_are_in_russian() {
        assert_eq!(Accent::Brass.name(), "Латунь");
        assert_eq!(Accent::Steel.name(), "Сталь");
        assert_eq!(Accent::Green.name(), "Зелёный");
    }
}
