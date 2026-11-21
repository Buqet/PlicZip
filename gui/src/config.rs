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

/// Язык интерфейса. Переключатель пока хранится в конфиге, но UI ещё отключён.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    Russian,
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Russian, Language::English];

    pub fn flag(self) -> &'static str {
        match self {
            Language::Russian => "🇷🇺",
            Language::English => "🇬🇧",
        }
    }

    pub fn completion(self) -> u8 {
        // Встроенные SOML-файлы синхронизированы по ключам.
        100
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
    /// Язык интерфейса.
    #[serde(default)]
    pub language: Language,
    /// Последние открытые архивы, от новых к старым.
    #[serde(default)]
    pub recent_archives: Vec<String>,
}

fn default_dark() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accent: Accent::Brass,
            dark: true,
            language: Language::Russian,
            recent_archives: Vec::new(),
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
            language: Language::English,
            recent_archives: vec!["backup.plz".into()],
        };
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.accent, Accent::Steel);
        assert!(!back.dark);
        assert_eq!(back.language, Language::English);
        assert_eq!(back.recent_archives, vec!["backup.plz"]);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.accent, Accent::Brass);
        assert!(cfg.dark);
        assert_eq!(cfg.language, Language::Russian);
        assert!(cfg.recent_archives.is_empty());
    }

    #[test]
    fn accent_names_are_in_russian() {
        assert_eq!(Accent::Brass.name(), "Латунь");
        assert_eq!(Accent::Steel.name(), "Сталь");
        assert_eq!(Accent::Green.name(), "Зелёный");
    }
}
