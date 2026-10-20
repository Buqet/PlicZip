//! Маски для `--exclude` и `--include`.
//!
//! Поддерживаются три конструкции:
//!
//! * `*` — любые символы, кроме `/`;
//! * `?` — ровно один символ, кроме `/`;
//! * `**` — любые символы, включая `/` (для «на любой глубине»).
//!
//! Совпадение проверяется по пути внутри архива (через `/`), например
//! `src/lib.rs`. Отдельная зависимость под это не нужна: правил три, реализация
//! помещается в один экран, а поведение можно проверить тестами.

/// Правило отбора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matcher {
    includes: Vec<Glob>,
    excludes: Vec<Glob>,
}

impl Matcher {
    /// Собирает правило из списков масок.
    pub fn new(includes: &[String], excludes: &[String]) -> Self {
        Matcher {
            includes: includes.iter().map(|p| Glob::new(p)).collect(),
            excludes: excludes.iter().map(|p| Glob::new(p)).collect(),
        }
    }

    /// Есть ли вообще ограничения (если нет — ничего не проверяем).
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }

    /// Попадает ли файл в архив.
    ///
    /// Исключение всегда сильнее включения: это ожидаемое поведение
    /// «возьми всё, кроме …».
    pub fn allows_file(&self, path: &str) -> bool {
        if self.excludes.iter().any(|glob| glob.matches(path)) {
            return false;
        }
        if self.includes.is_empty() {
            return true;
        }
        self.includes.iter().any(|glob| glob.matches(path))
    }

    /// Стоит ли заходить в каталог.
    ///
    /// Внутрь исключённого каталога не заходим вообще. Включения каталог не
    /// отсекают: маска может совпасть с файлом на любой глубине.
    pub fn allows_dir(&self, path: &str) -> bool {
        !self.excludes.iter().any(|glob| glob.matches_dir(path))
    }
}

/// Одна маска.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    pattern: Vec<u8>,
}

impl Glob {
    /// Создаёт маску из строки.
    pub fn new(pattern: &str) -> Self {
        Glob {
            pattern: pattern.as_bytes().to_vec(),
        }
    }

    /// Проверяет совпадение с путём.
    pub fn matches(&self, text: &str) -> bool {
        matches_from(&self.pattern, text.as_bytes())
    }

    /// Проверяет совпадение с каталогом.
    ///
    /// Маска `**/target/**` описывает содержимое каталога, но логично, чтобы
    /// она отсекала и сам `target` — иначе обход всё равно заходит внутрь и
    /// отбрасывает файлы по одному.
    pub fn matches_dir(&self, text: &str) -> bool {
        if self.matches(text) {
            return true;
        }
        match self.pattern.strip_suffix(b"/**".as_slice()) {
            Some(prefix) => matches_from(prefix, text.as_bytes()),
            None => false,
        }
    }
}

fn matches_from(pattern: &[u8], text: &[u8]) -> bool {
    // `**/` означает «на любой глубине, в том числе здесь»: `**/*.tmp`
    // должен совпадать и с `a/b.tmp`, и с `b.tmp`.
    if let Some(rest) = pattern.strip_prefix(b"**/".as_slice()) {
        if matches_from(rest, text) {
            return true;
        }
        let mut start = 0usize;
        while let Some(found) = text[start..].iter().position(|&byte| byte == b'/') {
            start += found + 1;
            if matches_from(rest, &text[start..]) {
                return true;
            }
        }
        return false;
    }

    let Some(&head) = pattern.first() else {
        return text.is_empty();
    };

    match head {
        b'*' => {
            let greedy = pattern.get(1) == Some(&b'*');
            let rest = if greedy { &pattern[2..] } else { &pattern[1..] };
            // Ноль символов.
            if matches_from(rest, text) {
                return true;
            }
            for take in 0..text.len() {
                // Одиночная `*` не переходит через разделитель пути.
                if !greedy && text[take] == b'/' {
                    break;
                }
                if matches_from(rest, &text[take + 1..]) {
                    return true;
                }
            }
            false
        }
        b'?' => !text.is_empty() && text[0] != b'/' && matches_from(&pattern[1..], &text[1..]),
        byte => !text.is_empty() && text[0] == byte && matches_from(&pattern[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pattern: &str) -> Glob {
        Glob::new(pattern)
    }

    #[test]
    fn exact_match() {
        assert!(glob("src/lib.rs").matches("src/lib.rs"));
        assert!(!glob("src/lib.rs").matches("src/main.rs"));
    }

    #[test]
    fn star_does_not_cross_slash() {
        assert!(glob("*.txt").matches("note.txt"));
        assert!(!glob("*.txt").matches("dir/note.txt"));
        assert!(glob("src/*.rs").matches("src/lib.rs"));
        assert!(!glob("src/*.rs").matches("src/deep/lib.rs"));
    }

    #[test]
    fn double_star_crosses_slash() {
        assert!(glob("**/*.tmp").matches("a/b/c.tmp"));
        assert!(glob("**/*.tmp").matches("c.tmp"));
        assert!(glob("src/**").matches("src/a/b/c.rs"));
        assert!(glob("**/target/**").matches("x/target/debug/a"));
    }

    #[test]
    fn question_mark() {
        assert!(glob("a?c").matches("abc"));
        assert!(!glob("a?c").matches("ac"));
        assert!(!glob("a?c").matches("a/c"));
    }

    #[test]
    fn matcher_exclude_wins() {
        let m = Matcher::new(&[], &["**/*.tmp".to_string(), "**/target/**".to_string()]);
        assert!(m.allows_file("src/lib.rs"));
        assert!(!m.allows_file("src/scratch.tmp"));
        assert!(!m.allows_file("target/debug/app.exe"));
        assert!(m.allows_dir("src"));
        assert!(!m.allows_dir("target"));
    }

    #[test]
    fn matcher_include_is_whitelist() {
        let m = Matcher::new(&["**/*.rs".to_string()], &[]);
        assert!(m.allows_file("src/lib.rs"));
        assert!(!m.allows_file("README.md"));
        // Каталоги не режем: внутри может быть подходящий файл.
        assert!(m.allows_dir("src"));
    }

    #[test]
    fn empty_matcher_allows_everything() {
        let m = Matcher::new(&[], &[]);
        assert!(m.is_empty());
        assert!(m.allows_file("любой/путь.bin"));
    }

    #[test]
    fn cyrillic_paths() {
        assert!(glob("**/*.jpg").matches("фото/2026/снимок.jpg"));
        assert!(glob("фото/**").matches("фото/2026/снимок.jpg"));
    }
}
