//! Маски для `--exclude` и `--include`.
//!
//! Поддерживаются три конструкции:
//!
//! * `*` — любые символы, кроме `/`;
//! * `?` — ровно один символ (не байт), кроме `/`;
//! * `**` — любые символы, включая `/` (для «на любой глубине»).
//!
//! Особый случай — `**/` в начале сегмента (в начале маски или после `/`):
//! это «ноль или больше каталогов», поэтому `**/*.tmp` совпадает и с `a/b.tmp`,
//! и с `b.tmp`, а `a/**/b` — и с `a/x/b`, и с `a/b`.
//!
//! Совпадение проверяется по пути внутри архива (через `/`), например
//! `src/lib.rs`. Обратные слэши и в масках, и в путях считаются `/`, так что
//! `src\lib.rs` на Windows работает так же, как `src/lib.rs`.
//!
//! Маска **без** `/` (например, `*.tmp` или `target`) в [`Matcher`] действует
//! на любой глубине, как в `.gitignore`. Маска со `/` привязана к корню архива.
//!
//! Отдельная зависимость под это не нужна: правил немного, а поведение
//! проверяется тестами. Сопоставление идёт по символам (`char`), а не по байтам,
//! и работает за `O(длина маски × длина пути)` без рекурсии и бэктрекинга.

/// Правило отбора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matcher {
    includes: Vec<Glob>,
    excludes: Vec<Glob>,
}

impl Matcher {
    /// Собирает правило из списков масок.
    ///
    /// Пустые маски игнорируются. Маска без `/` применяется на любой глубине.
    pub fn new(includes: &[String], excludes: &[String]) -> Self {
        Matcher {
            includes: build_globs(includes),
            excludes: build_globs(excludes),
        }
    }

    /// Есть ли вообще ограничения (если нет — ничего не проверяем).
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }

    /// Попадает ли файл в архив.
    ///
    /// Исключение всегда сильнее включения: это ожидаемое поведение
    /// «возьми всё, кроме …». Файл внутри исключённого каталога тоже
    /// отбрасывается, даже если сама маска совпадает только с каталогом
    /// (`--exclude target` отсекает и `target/debug/app.exe`).
    pub fn allows_file(&self, path: &str) -> bool {
        let path = normalize(path);
        if self.ancestors_excluded(&path) {
            return false;
        }
        if self.excludes.iter().any(|glob| glob.matches_chars(&path)) {
            return false;
        }
        if self.includes.is_empty() {
            return true;
        }
        self.includes.iter().any(|glob| glob.matches_chars(&path))
    }

    /// Стоит ли заходить в каталог.
    ///
    /// Внутрь исключённого каталога не заходим вообще. Включения каталог не
    /// отсекают: маска может совпасть с файлом на любой глубине.
    pub fn allows_dir(&self, path: &str) -> bool {
        let path = normalize(path);
        !self.ancestors_excluded(&path)
            && !self
                .excludes
                .iter()
                .any(|glob| glob.matches_dir_chars(&path))
    }

    /// Исключён ли хотя бы один из родительских каталогов пути.
    fn ancestors_excluded(&self, path: &[char]) -> bool {
        path.iter().enumerate().any(|(index, &ch)| {
            ch == '/'
                && index > 0
                && self
                    .excludes
                    .iter()
                    .any(|glob| glob.matches_dir_chars(&path[..index]))
        })
    }
}

/// Превращает список строк в маски: пустые отбрасываем, маски без `/`
/// делаем «на любой глубине» (добавляем `**/`).
fn build_globs(patterns: &[String]) -> Vec<Glob> {
    patterns
        .iter()
        .map(|pattern| pattern.replace('\\', "/"))
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            if pattern.contains('/') {
                Glob::new(&pattern)
            } else {
                Glob::new(&format!("**/{pattern}"))
            }
        })
        .collect()
}

/// Одна маска.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    tokens: Vec<Token>,
    /// Маска для самого каталога: `**/target/**` описывает содержимое, но
    /// логично, чтобы она отсекала и сам `target` — иначе обход всё равно
    /// заходит внутрь и отбрасывает файлы по одному.
    dir_tokens: Option<Vec<Token>>,
}

impl Glob {
    /// Создаёт маску из строки. Маска разбирается один раз.
    pub fn new(pattern: &str) -> Self {
        let tokens = compile(&normalize(pattern));
        let dir_tokens = match tokens.as_slice() {
            [rest @ .., Token::Literal('/'), Token::DoubleStar] => Some(rest.to_vec()),
            _ => None,
        };
        Glob { tokens, dir_tokens }
    }

    /// Проверяет совпадение с путём.
    pub fn matches(&self, text: &str) -> bool {
        self.matches_chars(&normalize(text))
    }

    /// Проверяет совпадение с каталогом.
    pub fn matches_dir(&self, text: &str) -> bool {
        self.matches_dir_chars(&normalize(text))
    }

    fn matches_chars(&self, text: &[char]) -> bool {
        run(&self.tokens, text)
    }

    fn matches_dir_chars(&self, text: &[char]) -> bool {
        let text = text.strip_suffix(&['/']).unwrap_or(text);
        if run(&self.tokens, text) {
            return true;
        }
        match &self.dir_tokens {
            Some(tokens) => run(tokens, text),
            None => false,
        }
    }
}

/// Элемент разобранной маски.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// Конкретный символ.
    Literal(char),
    /// `?` — один символ, кроме `/`.
    AnyChar,
    /// `*` — любое число символов, кроме `/`.
    Star,
    /// `**` — любое число символов, включая `/`.
    DoubleStar,
    /// `**/` в начале сегмента — ноль или больше каталогов.
    DirStar,
}

/// Приводит строку к символам и заменяет `\` на `/`.
fn normalize(text: &str) -> Vec<char> {
    text.chars()
        .map(|ch| if ch == '\\' { '/' } else { ch })
        .collect()
}

/// Разбирает маску в токены.
fn compile(pattern: &[char]) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(pattern.len());
    let mut index = 0usize;

    while index < pattern.len() {
        match pattern[index] {
            '*' => {
                let mut run_len = 1usize;
                while pattern.get(index + run_len) == Some(&'*') {
                    run_len += 1;
                }
                let at_segment_start = index == 0 || pattern[index - 1] == '/';

                if run_len == 1 {
                    tokens.push(Token::Star);
                    index += 1;
                } else if at_segment_start && pattern.get(index + run_len) == Some(&'/') {
                    // `**/` целиком.
                    tokens.push(Token::DirStar);
                    index += run_len + 1;
                } else {
                    // `**`, `***` и так далее считаем за `**`.
                    tokens.push(Token::DoubleStar);
                    index += run_len;
                }
            }
            '?' => {
                tokens.push(Token::AnyChar);
                index += 1;
            }
            ch => {
                tokens.push(Token::Literal(ch));
                index += 1;
            }
        }
    }

    tokens
}

/// Сопоставление без рекурсии: для каждого токена храним множество позиций
/// в тексте, до которых можно дойти. Время `O(токены × длина текста)`,
/// поэтому патологические маски вроде `*a*a*a*b` не зависают.
fn run(tokens: &[Token], text: &[char]) -> bool {
    let len = text.len();
    let mut current = vec![false; len + 1];
    let mut next = vec![false; len + 1];
    current[0] = true;

    for &token in tokens {
        next.iter_mut().for_each(|slot| *slot = false);

        match token {
            Token::Literal(expected) => {
                for pos in 0..len {
                    if current[pos] && text[pos] == expected {
                        next[pos + 1] = true;
                    }
                }
            }
            Token::AnyChar => {
                for pos in 0..len {
                    if current[pos] && text[pos] != '/' {
                        next[pos + 1] = true;
                    }
                }
            }
            Token::Star => {
                // Можно дойти до `pos`, если есть старт `s <= pos`, между
                // которым и `pos` нет `/`.
                let mut active = false;
                for pos in 0..=len {
                    active |= current[pos];
                    next[pos] = active;
                    if pos < len && text[pos] == '/' {
                        active = false;
                    }
                }
            }
            Token::DoubleStar => {
                let mut active = false;
                for pos in 0..=len {
                    active |= current[pos];
                    next[pos] = active;
                }
            }
            Token::DirStar => {
                // Ноль каталогов: остаёмся на месте. Каждый следующий каталог —
                // это «любые символы без `/`» и затем сам `/`.
                let mut active = false;
                for pos in 0..=len {
                    let after_slash = pos > 0 && text[pos - 1] == '/';
                    let segment_continues = pos > 0 && text[pos - 1] != '/';
                    let reach = current[pos] || (after_slash && active);
                    next[pos] = reach;
                    active = (active && segment_continues) || reach;
                }
            }
        }

        std::mem::swap(&mut current, &mut next);
        if !current.iter().any(|&reachable| reachable) {
            return false;
        }
    }

    current[len]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pattern: &str) -> Glob {
        Glob::new(pattern)
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
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
    fn double_star_in_the_middle() {
        let g = glob("a/**/b");
        assert!(g.matches("a/b"));
        assert!(g.matches("a/x/b"));
        assert!(g.matches("a/x/y/b"));
        assert!(!g.matches("a/xb"));
        assert!(!g.matches("ab"));
    }

    #[test]
    fn double_star_inside_a_segment() {
        let g = glob("a**b");
        assert!(g.matches("ab"));
        assert!(g.matches("axb"));
        assert!(g.matches("a/x/b"));
    }

    #[test]
    fn three_stars_are_double_star() {
        assert!(glob("src/***").matches("src/a/b"));
    }

    #[test]
    fn bare_double_star_matches_everything() {
        let g = glob("**");
        assert!(g.matches(""));
        assert!(g.matches("a"));
        assert!(g.matches("a/b/c"));
    }

    #[test]
    fn question_mark() {
        assert!(glob("a?c").matches("abc"));
        assert!(!glob("a?c").matches("ac"));
        assert!(!glob("a?c").matches("a/c"));
    }

    #[test]
    fn question_mark_counts_chars_not_bytes() {
        assert!(glob("a?c").matches("aбc"));
        assert!(!glob("a?c").matches("aббc"));
        assert!(glob("ф??то").matches("фбвто"));
        assert!(glob("?").matches("я"));
    }

    #[test]
    fn star_counts_chars_not_bytes() {
        assert!(glob("*.txt").matches("заметка.txt"));
        assert!(glob("ф*о").matches("фото"));
    }

    #[test]
    fn backslash_is_separator() {
        assert!(glob("src/*.rs").matches("src\\lib.rs"));
        assert!(glob("src\\*.rs").matches("src/lib.rs"));
        assert!(!glob("src/*.rs").matches("src\\deep\\lib.rs"));
    }

    #[test]
    fn pathological_pattern_is_fast() {
        let text = "a".repeat(5000);
        assert!(!glob("*a*a*a*a*a*a*a*a*a*b").matches(&text));
        assert!(glob("*a*a*a*a*a*a*a*a*a*a").matches(&text));
    }

    #[test]
    fn matches_dir_strips_trailing_glob() {
        let g = glob("**/target/**");
        assert!(g.matches_dir("target"));
        assert!(g.matches_dir("a/target"));
        assert!(g.matches_dir("a/target/"));
        assert!(g.matches_dir("a/target/debug"));
        assert!(!g.matches_dir("targets"));
        assert!(!g.matches_dir("a/src"));
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
    fn matcher_exclude_beats_include() {
        let m = Matcher::new(&strings(&["**/*.rs"]), &strings(&["src/generated/**"]));
        assert!(m.allows_file("src/lib.rs"));
        assert!(!m.allows_file("src/generated/api.rs"));
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
    fn matcher_excluded_parent_dir() {
        // Маска совпадает только с каталогом, но файлы внутри тоже отсекаются.
        let m = Matcher::new(&[], &strings(&["build/out"]));
        assert!(!m.allows_file("build/out/app.exe"));
        assert!(!m.allows_file("build/out/deep/app.exe"));
        assert!(!m.allows_dir("build/out"));
        assert!(m.allows_file("build/src/app.rs"));
        assert!(m.allows_dir("build"));
    }

    #[test]
    fn matcher_pattern_without_slash_is_unanchored() {
        let m = Matcher::new(&[], &strings(&["*.tmp", "target"]));
        assert!(!m.allows_file("c.tmp"));
        assert!(!m.allows_file("a/b/c.tmp"));
        assert!(!m.allows_file("target/a.o"));
        assert!(!m.allows_file("x/target/a.o"));
        assert!(!m.allows_dir("x/target"));
        assert!(m.allows_file("targets/a.o"));
        assert!(m.allows_file("src/lib.rs"));
    }

    #[test]
    fn matcher_pattern_with_slash_is_anchored() {
        let m = Matcher::new(&[], &strings(&["src/*.rs"]));
        assert!(!m.allows_file("src/lib.rs"));
        assert!(m.allows_file("other/src/lib.rs"));
        assert!(m.allows_file("src/deep/lib.rs"));
    }

    #[test]
    fn matcher_normalizes_backslashes() {
        let m = Matcher::new(&[], &strings(&["target\\debug"]));
        assert!(!m.allows_file("target/debug/app.exe"));
        assert!(!m.allows_file("target\\debug\\app.exe"));
        assert!(m.allows_file("target/release/app.exe"));
    }

    #[test]
    fn empty_matcher_allows_everything() {
        let m = Matcher::new(&[], &[]);
        assert!(m.is_empty());
        assert!(m.allows_file("любой/путь.bin"));
        assert!(m.allows_dir("любой/путь"));
    }

    #[test]
    fn empty_patterns_are_ignored() {
        let m = Matcher::new(&strings(&[""]), &strings(&[""]));
        assert!(m.is_empty());
        assert!(m.allows_file("a/b"));
    }

    #[test]
    fn cyrillic_paths() {
        assert!(glob("**/*.jpg").matches("фото/2026/снимок.jpg"));
        assert!(glob("фото/**").matches("фото/2026/снимок.jpg"));
        assert!(glob("фото/????/*.jpg").matches("фото/2026/снимок.jpg"));
        assert!(glob("фото/?/*.jpg").matches("фото/я/снимок.jpg"));

        let m = Matcher::new(&[], &strings(&["чертежи"]));
        assert!(!m.allows_file("проект/чертежи/план.pdf"));
        assert!(m.allows_file("проект/документы/план.pdf"));
    }
}
