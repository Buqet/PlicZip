//! Мелкие утилиты: форматирование размеров, прогресс в stderr, UTF-8 в консоли.

use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Получатель обновлений прогресса.
///
/// Библиотека вызывает [`ProgressSink::update`] по мере работы; CLI может
/// игнорировать его (stderr-прогресс остаётся), GUI — пересылать в окно.
pub trait ProgressSink: Send + Sync {
    /// `done` — сколько уже обработано, `total` — общий объём (0 — неизвестен).
    fn update(&self, done: u64, total: u64);
}

/// Прогноз остатка по скользящему окну, а не по всему времени с начала.
///
/// На смешанных данных среднее с начала врёт в разы: первые секунды —
/// индексация и быстрые файлы, потом идут гигабайты. Окно в 5 секунд даёт
/// честную скорость здесь и сейчас; пока операция короче окна, оценка
/// считается по тому, что уже есть.
pub struct EtaEstimator {
    samples: std::collections::VecDeque<(Instant, u64)>,
}

impl Default for EtaEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl EtaEstimator {
    /// Окно прогноза.
    const WINDOW: Duration = Duration::from_secs(5);

    /// Создаёт пустой оценщик.
    pub fn new() -> Self {
        EtaEstimator {
            samples: std::collections::VecDeque::new(),
        }

    }

    /// Отмечает новое значение счётчика.
    pub fn sample(&mut self, done: u64) {
        self.sample_at(Instant::now(), done);
    }

    /// То же, что [`Self::sample`], но с явным моментом времени (для тестов).
    fn sample_at(&mut self, now: Instant, done: u64) {
        self.samples.push_back((now, done));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > Self::WINDOW {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Текущая скорость в байтах в секунду; `None`, пока данных мало.
    pub fn speed(&self) -> Option<f64> {
        let first = self.samples.front()?;
        let last = self.samples.back()?;
        let dt = last.0.duration_since(first.0).as_secs_f64();
        if dt < 0.3 {
            return None;
        }
        let delta = last.1.saturating_sub(first.1) as f64;
        Some(delta / dt)
    }

    /// Остаток времени; `None`, если скорость ещё неизвестна.
    pub fn eta(&self, done: u64, total: u64) -> Option<Duration> {
        if total == 0 {
            return None;
        }
        let speed = self.speed()?;
        if speed <= 1.0 {
            return None;
        }
        let remaining = total.saturating_sub(done) as f64;
        Some(Duration::from_secs_f64(remaining / speed))
    }
}

/// Форматирует размер в двоичных единицах: `1.5 KiB`, `12.0 MiB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{} B", n);
    }
    let mut value = n as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, UNITS[unit])
}

/// Форматирует длительность: `1.2s`, `3m 05s`.
pub fn human_duration(d: Duration) -> String {
    let total = d.as_secs();
    if total < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else if total < 3600 {
        format!("{}m {:02}s", total / 60, total % 60)
    } else {
        format!("{}h {:02}m", total / 3600, (total % 3600) / 60)
    }
}

/// Во сколько раз содержимое уменьшилось при упаковке (0, если пусто).
pub fn ratio(raw: u64, stored: u64) -> f64 {
    if stored == 0 {
        return 0.0;
    }
    raw as f64 / stored as f64
}

/// Во сколько процентов сжалось содержимое (0 %, если пусто).
pub fn saved_percent(raw: u64, stored: u64) -> f64 {
    if raw == 0 {
        return 0.0;
    }
    100.0 - (stored as f64 / raw as f64 * 100.0)
}

/// Однострочный прогресс-бар в stderr.
///
/// Ничего не печатает, если stderr — не терминал (например, при перенаправлении
/// в файл), чтобы не мусорить в логах.
pub struct Progress {
    enabled: bool,
    label: &'static str,
    total: u64,
    done: u64,
    last: Instant,
    started: Instant,
    printed: bool,
    sink: Option<Arc<dyn ProgressSink>>,
    eta: EtaEstimator,
}

impl Progress {
    /// Создаёт прогресс с известным общим объёмом работ.
    pub fn new(label: &'static str, total: u64, enabled: bool) -> Self {
        Progress {
            enabled: enabled && std::io::stderr().is_terminal(),
            label,
            total,
            done: 0,
            last: Instant::now(),
            started: Instant::now(),
            printed: false,
            sink: None,
            eta: EtaEstimator::new(),
        }
    }

    /// Привязывает получатель обновлений (GUI, тесты, скрипты).
    pub fn with_sink(mut self, sink: Arc<dyn ProgressSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Устанавливает текущее значение счётчика.
    pub fn set(&mut self, done: u64) {
        self.done = done;
        self.report();
    }

    /// Прибавляет к счётчику.
    pub fn add(&mut self, n: u64) {
        self.done = self.done.saturating_add(n);
        self.report();
    }

    fn report(&mut self) {
        if let Some(sink) = &self.sink {
            sink.update(self.done, self.total);
        }
        self.maybe_print();
    }

    fn maybe_print(&mut self) {
        self.eta.sample(self.done);
        if !self.enabled {
            return;
        }
        if self.last.elapsed() < Duration::from_millis(120) {
            return;
        }
        self.last = Instant::now();
        self.printed = true;
        let line = self.line();
        let mut err = std::io::stderr();
        let _ = write!(err, "\r{}\x1b[K", line);
        let _ = err.flush();
    }

    /// Строка прогресса: объём, процент, скорость и остаток.
    fn line(&self) -> String {
        let percent = self
            .done
            .saturating_mul(100)
            .checked_div(self.total)
            .map_or(100, |p| p.min(100));
        let mut line = if self.total == 0 {
            format!("  {} {}", self.label, human_bytes(self.done))
        } else {
            format!(
                "  {} {} / {}  {:>3}%",
                self.label,
                human_bytes(self.done),
                human_bytes(self.total),
                percent
            )
        };
        // Скорость и остаток — по последним секундам, а не средним с начала.
        if let Some(speed) = self.eta.speed() {
            line.push_str(&format!("  {} MiB/s", speed / (1024.0 * 1024.0)));
            if let Some(eta) = self.eta.eta(self.done, self.total) {
                line.push_str(&format!("  осталось {}", human_duration(eta)));
            }
        }
        line
    }

    /// Завершает строку прогресса итоговым сообщением.
    pub fn finish_with(&mut self, message: &str) {
        if self.enabled {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r\x1b[K  {}\n", message);
            let _ = err.flush();
        }
    }

    /// Завершает прогресс без дополнительного текста.
    pub fn finish(&mut self) {
        if self.enabled && self.printed {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r\x1b[K");
            let _ = err.flush();
        }
    }

    /// Сколько времени прошло с начала работы.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Включает UTF-8 в консоли Windows, чтобы русский текст не превращался в мусор.
#[cfg(windows)]
#[allow(unsafe_code)] // единственный вызов WinAPI во всём проекте
pub fn enable_utf8_console() {
    const CP_UTF8: u32 = 65001;
    extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> i32;
        fn SetConsoleCP(code_page: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(CP_UTF8);
        SetConsoleCP(CP_UTF8);
    }
}

/// На не-Windows консоль уже в UTF-8.
#[cfg(not(windows))]
pub fn enable_utf8_console() {}

/// Форматирует время из unix-секунд как `2024-05-01 12:00:00` (UTC).
///
/// Своя реализация вместо `chrono`: нужна одна функция, а лишняя зависимость
/// в архиваторе ни к чему.
pub fn format_unix_time(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year,
        month,
        day,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Переводит число дней от 1970-01-01 в календарную дату (алгоритм Хауарда Хиннанта).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (year + if month <= 2 { 1 } else { 0 }, month, day)
}

/// Русская форма множественного числа: `plural(4, "каталог", "каталога", "каталогов")`.
pub fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let n100 = n % 100;
    let n10 = n % 10;
    let word = if (11..=14).contains(&n100) {
        many
    } else if n10 == 1 {
        one
    } else if (2..=4).contains(&n10) {
        few
    } else {
        many
    };
    format!("{} {}", n, word)
}

/// Дополняет строку справа пробелами с учётом символов, а не байт.
///
/// Обычное `{:<width}` считает байты UTF-8, поэтому кириллица разъезжается.
pub fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let mut out = text.to_string();
    out.push_str(&" ".repeat(width - len));
    out
}

/// Дополняет строку слева пробелами с учётом символов.
pub fn pad_left(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let mut out = " ".repeat(width - len);
    out.push_str(text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn ratios() {
        assert_eq!(ratio(100, 50), 2.0);
        assert_eq!(ratio(100, 0), 0.0);
        assert!((saved_percent(100, 25) - 75.0).abs() < 1e-9);
    }

    #[test]
    fn durations() {
        assert_eq!(human_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(human_duration(Duration::from_secs(185)), "3m 05s");
    }

    #[test]
    fn dates() {
        assert_eq!(format_unix_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_unix_time(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(format_unix_time(1_743_465_600), "2025-04-01 00:00:00");
    }

    #[test]
    fn padding_counts_chars() {
        assert_eq!(pad_right("фото", 6), "фото  ");
        assert_eq!(pad_left("фото", 6), "  фото");
        assert_eq!(pad_right("abcdef", 3), "abcdef");
    }

    #[test]
    fn russian_plurals() {
        assert_eq!(plural(1, "файл", "файла", "файлов"), "1 файл");
        assert_eq!(plural(2, "файл", "файла", "файлов"), "2 файла");
        assert_eq!(plural(5, "файл", "файла", "файлов"), "5 файлов");
        assert_eq!(plural(11, "файл", "файла", "файлов"), "11 файлов");
        assert_eq!(plural(21, "файл", "файла", "файлов"), "21 файл");
        assert_eq!(plural(112, "файл", "файла", "файлов"), "112 файлов");
        assert_eq!(plural(0, "каталог", "каталога", "каталогов"), "0 каталогов");
    }

    #[test]
    fn eta_needs_enough_samples() {
        let mut est = EtaEstimator::new();
        est.sample(1000);
        assert!(est.speed().is_none(), "по одной точке скорость неизвестна");
        assert!(est.eta(1000, 2000).is_none());
        assert!(est.eta(1000, 0).is_none(), "без общего объёма остаток неизвестен");
    }

    #[test]
    fn eta_counts_recent_speed_not_average() {
        // Время задаём явно: тест не зависит от точности sleep на CI.
        let mut est = EtaEstimator::new();
        let t0 = Instant::now();
        // Медленное начало: 1 МБ за первые 10 секунд — оно должно выпасть из окна.
        est.sample_at(t0, 0);
        est.sample_at(t0 + Duration::from_secs(10), 1_000_000);
        // Потом 20 замеров по 20 мс: ровно 50 МБ/с.
        let base = t0 + Duration::from_secs(10);
        for i in 1..=20u64 {
            est.sample_at(
                base + Duration::from_millis(20 * i),
                1_000_000 + i * 1_000_000,
            );
        }
        let speed = est.speed().expect("скорость должна посчитаться");
        assert!(
            (speed - 50_000_000.0).abs() < 1.0,
            "скорость {speed}, ожидалось 50 МБ/с по окну, а не среднее с начала"
        );
        // Осталось 20 МБ при 50 МБ/с — 0.4 секунды.
        let eta = est.eta(21_000_000, 41_000_000).expect("остаток");
        assert!((eta.as_secs_f64() - 0.4).abs() < 1e-6, "остаток {eta:?}");
    }

    #[test]
    fn progress_line_shows_volume_speed_and_remaining() {
        let mut progress = Progress::new("сжатие", 100_000_000, false);
        for i in 1..=20u64 {
            std::thread::sleep(Duration::from_millis(20));
            progress.set(i * 1_000_000);
        }
        let line = progress.line();
        assert!(line.contains("сжатие"), "{line}");
        assert!(line.contains("MiB/s"), "{line}");
        assert!(line.contains("осталось"), "{line}");
    }

    #[test]
    fn progress_reports_to_sink() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        struct Counter(Arc<AtomicU64>);
        impl ProgressSink for Counter {
            fn update(&self, done: u64, _total: u64) {
                self.0.store(done, Ordering::SeqCst);
            }
        }

        let seen = Arc::new(AtomicU64::new(0));
        let sink = Arc::new(Counter(seen.clone()));
        let mut progress = Progress::new("сжатие", 100, false).with_sink(sink);
        progress.add(40);
        progress.set(70);
        assert_eq!(seen.load(Ordering::SeqCst), 70);
    }
}
