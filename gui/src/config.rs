//! Настройки GUI: акцент, тема, последняя папка. Сохраняются в KDL.

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
    /// Голубой — спокойный холодный акцент.
    Cyan,
    /// Фиолетовый — насыщенный контрастный акцент.
    Violet,
    /// Розовый — тёплый выразительный акцент.
    Rose,
    /// Оранжевый — энергичный акцент.
    Orange,
    /// Синий — глубокий классический акцент.
    Blue,
}

impl Accent {
    pub const ALL: [Accent; 8] = [
        Accent::Brass,
        Accent::Steel,
        Accent::Green,
        Accent::Cyan,
        Accent::Violet,
        Accent::Rose,
        Accent::Orange,
        Accent::Blue,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Accent::Brass => "accent-brass",
            Accent::Steel => "accent-steel",
            Accent::Green => "accent-green",
            Accent::Cyan => "accent-cyan",
            Accent::Violet => "accent-violet",
            Accent::Rose => "accent-rose",
            Accent::Orange => "accent-orange",
            Accent::Blue => "accent-blue",
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
        Some(Self::dir()?.join("config.kdl"))
    }

    fn legacy_path() -> Option<PathBuf> {
        Some(Self::dir()?.join("config.json"))
    }

    /// Читает KDL-конфиг; старый JSON автоматически мигрируется при первом запуске.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_kdl(&text).unwrap_or_default();
        }

        let Some(legacy_path) = Self::legacy_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(legacy_path) else {
            return Self::default();
        };
        let config: Self = serde_json::from_str(&text).unwrap_or_default();
        config.save();
        config
    }

    /// Пишет KDL-конфиг; ошибки молча игнорируются — настройки не критичны.
    pub fn save(&self) {
        let Some(dir) = Self::dir() else { return };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let mut text = String::new();
        text.push_str(&format!(
            "accent {}\n",
            kdl_string(&format!("{:?}", self.accent))
        ));
        text.push_str(&format!("dark #{}\n", self.dark));
        text.push_str(&format!(
            "language {}\n",
            kdl_string(&format!("{:?}", self.language))
        ));
        for archive in &self.recent_archives {
            text.push_str(&format!("recent_archives {}\n", kdl_string(archive)));
        }
        let _ = std::fs::write(dir.join("config.kdl"), text);
    }
}

fn parse_kdl(text: &str) -> Option<Config> {
    let document: kdl::KdlDocument = text.parse().ok()?;
    let accent = match document
        .get_arg("accent")
        .and_then(kdl::KdlValue::as_string)
        .unwrap_or("Brass")
    {
        "Brass" => Accent::Brass,
        "Steel" => Accent::Steel,
        "Green" => Accent::Green,
        "Cyan" => Accent::Cyan,
        "Violet" => Accent::Violet,
        "Rose" => Accent::Rose,
        "Orange" => Accent::Orange,
        "Blue" => Accent::Blue,
        _ => return None,
    };
    let language = match document
        .get_arg("language")
        .and_then(kdl::KdlValue::as_string)
        .unwrap_or("Russian")
    {
        "Russian" => Language::Russian,
        "English" => Language::English,
        _ => return None,
    };
    let dark = document
        .get_arg("dark")
        .and_then(kdl::KdlValue::as_bool)
        .unwrap_or(true);
    let recent_archives = document
        .iter_args("recent_archives")
        .map(|value| value.as_string().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(Config {
        accent,
        dark,
        language,
        recent_archives,
    })
}

fn kdl_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_kdl() {
        let text =
            "accent \"Steel\"\ndark #false\nlanguage \"English\"\nrecent_archives \"backup.plz\"\n";
        let back = parse_kdl(text).unwrap();
        assert_eq!(back.accent, Accent::Steel);
        assert!(!back.dark);
        assert_eq!(back.language, Language::English);
        assert_eq!(back.recent_archives, vec!["backup.plz"]);
    }

    #[test]
    fn kdl_string_escapes_archive_paths() {
        assert_eq!(kdl_string("a\\b\"c\n"), "\"a\\\\b\\\"c\\n\"");
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
    fn accent_keys_cover_all_variants() {
        assert_eq!(Accent::ALL.len(), 8);
        assert_eq!(Accent::Brass.key(), "accent-brass");
        assert_eq!(Accent::Blue.key(), "accent-blue");
    }
}
