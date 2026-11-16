//! Локализация GUI из встроенных SOML-файлов.

use crate::config::Language;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Locale {
    pub language: String,
    pub menu_help: String,
    pub menu_view: String,
    pub menu_settings: String,
    pub menu_about: String,
    pub update_check: String,
    pub update_checking: String,
    pub update_available: String,
    pub update_none: String,
    pub settings_title: String,
    pub settings_general: String,
    pub settings_language: String,
    pub language_russian: String,
    pub language_english: String,
    pub settings_language_disabled: String,
    pub about_changes: String,
    pub about_close: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, String>,
}

impl Locale {
    pub fn load(language: Language) -> Self {
        let source = match language {
            Language::Russian => include_str!("../locales/ru.soml"),
            Language::English => include_str!("../locales/en.soml"),
        };
        soml::from_str(source).expect("embedded SOML locale must be valid")
    }

    pub fn text(&self, key: &str) -> String {
        self.extra
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_locales_parse() {
        assert_eq!(Locale::load(Language::Russian).language, "Русский");
        assert_eq!(Locale::load(Language::English).language, "English");
    }
}
