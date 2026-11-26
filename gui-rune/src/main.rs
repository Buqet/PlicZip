#![forbid(unsafe_code)]

//! Первый рабочий вертикальный срез Rune-интерфейса PlicZip.
//!
//! Таблица отображает реальные записи native `.plz` архива. Создание и извлечение
//! пока остаются в основной egui-оболочке и будут перенесены отдельными этапами.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use rune::{
    col, icon, row, text, text_input, Align, AppSpec, Color, Cx, El, Family, FileDialogOptions,
    Icon, Pixels, ScrollSpec, TextEdit, TextStyle, Tx,
};
use rune_kit::{
    Button, ButtonVariant, Cell, ControlSize, IconButton, IconButtonVariant, Kit, PaletteFocus,
    PaletteSeed, SortDir, Table, TableColumn, TableState,
};

static GUI_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/Intro.otf"),
    include_bytes!("../assets/DeutschGothic.ttf"),
];
// Darkened high-frequency grain derived from the user's texture reference (1180 × 960).
static DARK_GRAIN: &[u8] = include_bytes!("../assets/grain_dark_1180x960.raw");
static SETTINGS_ICON: Icon = Icon {
    name: "settings",
    view: 64.0,
    body: r#"<path d="M47.38 27.58 L53.68 28.28 L53.68 35.72 L47.38 36.42 L46 39.75 L49.96 44.7 L44.7 49.96 L39.75 46 L36.42 47.38 L35.72 53.68 L28.28 53.68 L27.58 47.38 L24.25 46 L19.3 49.96 L14.04 44.7 L18 39.75 L16.62 36.42 L10.32 35.72 L10.32 28.28 L16.62 27.58 L18 24.25 L14.04 19.3 L19.3 14.04 L24.25 18 L27.58 16.62 L28.28 10.32 L35.72 10.32 L36.42 16.62 L39.75 18 L44.7 14.04 L49.96 19.3 L46 24.25 Z" fill="none" stroke="currentColor" stroke-width="4" stroke-linejoin="round"/><circle cx="32" cy="32" r="7" fill="none" stroke="currentColor" stroke-width="4"/>"#,
};
static BLAKE3_ICON: Icon = Icon {
    name: "shield-check",
    view: 64.0,
    body: r#"<path d="M32 6 L54 14 V30 C54 44 44 54 32 58 C20 54 10 44 10 30 V14 Z" fill="none" stroke="currentColor" stroke-width="4" stroke-linejoin="round"/>"#,
};
static BLAKE3_OK_ICON: Icon = Icon {
    name: "shield-check-success",
    view: 64.0,
    body: r#"<path d="M32 6 L54 14 V30 C54 44 44 54 32 58 C20 54 10 44 10 30 V14 Z" fill="none" stroke="currentColor" stroke-width="4" stroke-linejoin="round"/><path d="M21 32 L29 40 L43 24" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
static BLAKE3_ERROR_ICON: Icon = Icon {
    name: "shield-cross-error",
    view: 64.0,
    body: r#"<path d="M32 6 L54 14 V30 C54 44 44 54 32 58 C20 54 10 44 10 30 V14 Z" fill="none" stroke="currentColor" stroke-width="4" stroke-linejoin="round"/><path d="M24 24 L40 40 M40 24 L24 40" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round"/>"#,
};
static APP_HANDLE: std::sync::OnceLock<rune::AppHandle<State>> = std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeMode {
    Light,
    Dark,
    Cosmos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    English,
    Russian,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsSection {
    #[default]
    General,
    Appearance,
    Extraction,
    Security,
}

impl Language {
    fn text(self, ru: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Russian => ru,
            Self::English => en,
        }
    }
}

#[derive(Clone, Copy)]
struct ThemePalette {
    page: Color,
    panel: Color,
    raised: Color,
    border: Color,
    muted: Color,
    text: Color,
    field: Color,
    table: Color,
}

fn theme_palette(mode: ThemeMode) -> ThemePalette {
    let (page, panel, raised, border, muted, text, field, table) = match mode {
        ThemeMode::Dark => (
            "#0C0E10", "#141719", "#1A1D20", "#292E32", "#91989F", "#ECEFF1", "#0E1012", "#111315",
        ),
        ThemeMode::Light => (
            "#F1F2F0", "#FAFAF8", "#ECEEEB", "#D5D8D4", "#666D71", "#202427", "#FFFFFF", "#F5F6F4",
        ),
        ThemeMode::Cosmos => (
            "#0D1020", "#15192B", "#1D2238", "#343B5A", "#A2A9C4", "#E8EBFF", "#0B0E1B", "#101426",
        ),
    };
    ThemePalette {
        page: ui_color(page),
        panel: ui_color(panel),
        raised: ui_color(raised),
        border: ui_color(border),
        muted: ui_color(muted),
        text: ui_color(text),
        field: ui_color(field),
        table: ui_color(table),
    }
}

fn grain_texture(mode: ThemeMode) -> &'static Pixels {
    static GRAINS: std::sync::LazyLock<[Pixels; 3]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|index| {
            let pixels = Pixels::new();
            let mut rgba = Vec::with_capacity(1180 * 960 * 4);
            for &shade in DARK_GRAIN.iter() {
                let rgb = match index {
                    0 => {
                        let light = 255 - (shade.saturating_sub(8) / 2);
                        [light, light, light]
                    }
                    1 => [shade, shade, shade],
                    _ => [shade.saturating_sub(2), shade, shade.saturating_add(7)],
                };
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            pixels.set(1180, 960, rgba);
            pixels
        })
    });
    &GRAINS[match mode {
        ThemeMode::Light => 0,
        ThemeMode::Dark => 1,
        ThemeMode::Cosmos => 2,
    }]
}

fn settings_grain_texture(mode: ThemeMode) -> &'static Pixels {
    static GRAINS: std::sync::LazyLock<[Pixels; 3]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|index| {
            let pixels = Pixels::new();
            let mut rgba = Vec::with_capacity(1180 * 960 * 4);
            for &shade in DARK_GRAIN.iter() {
                let variance = shade.saturating_sub(8);
                let (rgb, alpha) = match index {
                    0 => ([218, 221, 218], 25 + variance.saturating_mul(2)),
                    1 => ([166, 176, 184], 24 + variance.saturating_mul(2)),
                    _ => ([145, 160, 220], 25 + variance.saturating_mul(2)),
                };
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
            pixels.set(1180, 960, rgba);
            pixels
        })
    });
    &GRAINS[match mode {
        ThemeMode::Light => 0,
        ThemeMode::Dark => 1,
        ThemeMode::Cosmos => 2,
    }]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Accent {
    Blue,
    Green,
    Orange,
}

impl Accent {
    fn color(self) -> Color {
        let value = match self {
            Self::Blue => "#76A9FF",
            Self::Green => "#83C98B",
            Self::Orange => "#F2A45D",
        };
        rune::color::hex(value).expect("valid hard-coded accent color")
    }

    fn label(self, language: Language) -> &'static str {
        match self {
            Self::Blue => language.text("Синий", "Blue"),
            Self::Green => language.text("Зелёный", "Green"),
            Self::Orange => language.text("Оранжевый", "Orange"),
        }
    }
}

fn theme(accent: Accent, mode: ThemeMode) -> Kit {
    let palette = theme_palette(mode);
    let mut kit = Kit::from_palette_focus(
        PaletteSeed {
            bg: palette.page,
            fg: palette.text,
            accent: accent.color(),
        },
        PaletteFocus::Accent,
    );
    kit.c.bg_icon_button_contrast = accent.color();
    kit.c.submit_fg = ui_color("#101214");
    kit.c.submit_icon = ui_color("#101214");
    kit
}

struct ArchiveEntry {
    id: String,
    name: String,
    kind: EntryKind,
    size: u64,
}

#[derive(Clone, Copy)]
enum EntryKind {
    File,
    Directory,
    Link,
    Other,
}

struct State {
    entries: Vec<ArchiveEntry>,
    visible: Vec<usize>,
    order: Option<Vec<usize>>,
    table: TableState,
    accent: Accent,
    theme_mode: ThemeMode,
    language: Language,
    auto_verify: bool,
    animations_enabled: bool,
    settings_section: SettingsSection,
    archive_path: Option<PathBuf>,
    archive_encrypted: bool,
    path_input: TextEdit,
    output_input: TextEdit,
    filter_input: TextEdit,
    status: String,
    settings_open: bool,
    verify_running: bool,
    verify_success: bool,
    verify_failed: bool,
    verify_generation: u64,
    shield_hover: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            visible: Vec::new(),
            order: None,
            table: TableState::new().multi_select(true),
            accent: Accent::Blue,
            theme_mode: ThemeMode::Dark,
            language: Language::Russian,
            auto_verify: false,
            animations_enabled: true,
            settings_section: SettingsSection::General,
            archive_path: None,
            archive_encrypted: false,
            path_input: TextEdit::new(""),
            output_input: TextEdit::new(""),
            filter_input: TextEdit::new(""),
            status: "Выберите архив .plz для просмотра его содержимого".to_owned(),
            settings_open: false,
            verify_running: false,
            verify_success: false,
            verify_failed: false,
            verify_generation: 0,
            shield_hover: false,
        }
    }
}

impl State {
    fn open_path_input(&mut self) {
        let value = self.path_input.text.trim().to_owned();
        if value.is_empty() {
            self.status = "Укажите путь к архиву .plz".to_owned();
            return;
        }
        self.open_archive(PathBuf::from(value));
    }

    fn extract_all(&mut self) {
        let Some(archive_path) = self.archive_path.clone() else {
            self.status = "Сначала откройте архив .plz".to_owned();
            return;
        };
        let output = self.output_input.text.trim().to_owned();
        if output.is_empty() {
            self.status = "Укажите папку для распаковки".to_owned();
            return;
        }

        match extract_archive(&archive_path, Path::new(&output)) {
            Ok(stats) => {
                self.status = format!(
                    "Распаковка завершена · файлов: {} · каталогов: {} · ссылок: {} · пропущено: {} · {}",
                    stats.files, stats.dirs, stats.links, stats.skipped, output
                );
            }
            Err(error) => self.status = format!("Ошибка распаковки: {error}"),
        }
    }

    fn verify_blake3(&mut self) {
        if self.verify_running {
            return;
        }
        let Some(path) = self.archive_path.clone() else {
            self.status = "Сначала откройте архив .plz".to_owned();
            return;
        };
        let Some(handle) = APP_HANDLE.get().cloned() else {
            self.status = "Не удалось запустить проверку BLAKE3".to_owned();
            return;
        };

        self.verify_running = true;
        self.verify_success = false;
        self.verify_failed = false;
        self.verify_generation = self.verify_generation.wrapping_add(1);
        let generation = self.verify_generation;
        self.status = "Проверка BLAKE3 файлов и блоков…".to_owned();
        std::thread::spawn(move || {
            let result = (|| -> Result<pliczip::reader::TestReport, String> {
                let mut archive = pliczip::reader::Archive::open_with(&path, None)
                    .map_err(|error| format!("{error:#}"))?;
                archive
                    .test(false, 100_000, None)
                    .map_err(|error| format!("{error:#}"))
            })();
            handle.update(move |state, tx| {
                if state.verify_generation != generation
                    || state.archive_path.as_deref() != Some(path.as_path())
                {
                    return;
                }
                state.verify_running = false;
                state.status = match result {
                    Ok(report) => {
                        state.verify_success = true;
                        state.verify_failed = false;
                        format!(
                            "Проверка BLAKE3 пройдена · {} файлов · {} блоков",
                            report.files, report.blocks
                        )
                    }
                    Err(error) => {
                        state.verify_success = false;
                        state.verify_failed = true;
                        format!("Ошибка проверки BLAKE3: {error}")
                    }
                };
                tx.color_motion(rune::Spring::new(0.32, 0.0));
            });
        });
    }

    fn refresh_visible(&mut self) {
        let query = self.filter_input.text.trim().to_lowercase();
        self.visible.clear();
        if let Some(order) = &self.order {
            for &index in order {
                let entry = &self.entries[index];
                if query.is_empty() || entry.name.to_lowercase().contains(query.as_str()) {
                    self.visible.push(index);
                }
            }
        } else {
            for (index, entry) in self.entries.iter().enumerate() {
                if query.is_empty() || entry.name.to_lowercase().contains(query.as_str()) {
                    self.visible.push(index);
                }
            }
        }
    }

    fn open_archive(&mut self, path: PathBuf) {
        self.path_input = TextEdit::new(path.to_string_lossy().into_owned());
        let result = (|| -> Result<(Vec<ArchiveEntry>, bool), String> {
            // Inspect before opening: Archive::open may request a password on stdin, which must
            // never block the Rune event loop. Password entry is a follow-up UI milestone.
            let (source, _) = pliczip::source::open(&path).map_err(|error| format!("{error:#}"))?;
            let layout =
                pliczip::format::inspect(source.as_ref()).map_err(|error| format!("{error:#}"))?;
            if layout.encrypted {
                return Err("Зашифрованные архивы пока не поддерживаются в Rune preview".to_owned());
            }

            let archive = pliczip::reader::Archive::open_with(&path, None)
                .map_err(|error| format!("{error:#}"))?;
            let entries = archive
                .entries()
                .iter()
                .map(|entry| ArchiveEntry {
                    id: entry.path.clone(),
                    name: entry.path.clone(),
                    kind: match entry.kind {
                        pliczip::format::Kind::File => EntryKind::File,
                        pliczip::format::Kind::Dir => EntryKind::Directory,
                        pliczip::format::Kind::Symlink => EntryKind::Link,
                        pliczip::format::Kind::Other => EntryKind::Other,
                    },
                    size: entry.size,
                })
                .collect();
            Ok((entries, archive.is_encrypted()))
        })();

        match result {
            Ok((entries, encrypted)) => {
                let count = entries.len();
                self.entries = entries;
                self.order = None;
                self.filter_input = TextEdit::new("");
                self.output_input =
                    TextEdit::new(suggested_output_path(&path).display().to_string());
                self.table = TableState::new().multi_select(true);
                self.archive_path = Some(path.clone());
                self.archive_encrypted = encrypted;
                self.verify_success = false;
                self.verify_failed = false;
                self.verify_running = false;
                self.verify_generation = self.verify_generation.wrapping_add(1);
                self.refresh_visible();
                self.status = format!("Открыт {} · {count} объектов", path.display());
                if self.auto_verify {
                    self.verify_blake3();
                }
            }
            Err(error) => {
                // Keep the currently opened archive visible if the new selection is invalid.
                self.status = format!("Не удалось открыть {}: {error}", path.display());
            }
        }
    }
}

fn suggested_output_path(archive_path: &Path) -> PathBuf {
    let parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = archive_path
        .file_stem()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| Cow::Borrowed("archive"));
    parent.join(format!("{stem}_extracted"))
}

fn extract_archive(
    archive_path: &Path,
    output: &Path,
) -> Result<pliczip::reader::ExtractStats, String> {
    let (source, _) = pliczip::source::open(archive_path).map_err(|error| format!("{error:#}"))?;
    let layout = pliczip::format::inspect(source.as_ref()).map_err(|error| format!("{error:#}"))?;
    if layout.encrypted {
        return Err("зашифрованные архивы пока не поддерживаются в Rune preview".to_owned());
    }

    let mut archive = pliczip::reader::Archive::open_with(archive_path, None)
        .map_err(|error| format!("{error:#}"))?;
    archive
        .extract(&pliczip::reader::ExtractOptions {
            output: output.to_path_buf(),
            skip_existing: true,
            show_progress: false,
            ..Default::default()
        })
        .map_err(|error| format!("{error:#}"))
}

fn sort_entries(state: &mut State, column: &str, direction: SortDir) {
    let entries = &state.entries;
    let order = state
        .order
        .get_or_insert_with(|| (0..entries.len()).collect());
    order.sort_by(|&left, &right| {
        let left = &entries[left];
        let right = &entries[right];
        let comparison = match column {
            "name" => rune_kit::natural_cmp(&left.name, &right.name),
            "type" => kind_name(left.kind).cmp(kind_name(right.kind)),
            "size" => left.size.cmp(&right.size),
            // Protection is an archive-level property, so every row has the same value.
            "encrypted" => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Equal,
        };
        direction.apply(comparison)
    });
    state.refresh_visible();
}

fn kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Directory => "directory",
        EntryKind::Link => "link",
        EntryKind::Other => "other",
    }
}

fn kind_label(kind: EntryKind, language: Language) -> &'static str {
    match kind {
        EntryKind::File => language.text("Файл", "File"),
        EntryKind::Directory => language.text("Каталог", "Folder"),
        EntryKind::Link => language.text("Ссылка", "Link"),
        EntryKind::Other => language.text("Другое", "Other"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusKind {
    Neutral,
    Success,
    Error,
}

fn status_kind(status: &str) -> StatusKind {
    if status.starts_with("Ошибка")
        || status.starts_with("Не удалось")
        || status.starts_with("Укажите")
        || status.starts_with("Сначала")
    {
        StatusKind::Error
    } else if status.starts_with("Открыт")
        || status.starts_with("Распаковка завершена")
        || status.starts_with("Проверка BLAKE3 пройдена")
    {
        StatusKind::Success
    } else {
        StatusKind::Neutral
    }
}

fn localized_status(status: &str, language: Language) -> String {
    if language == Language::Russian {
        return status.to_owned();
    }
    if status == "Выберите архив .plz для просмотра его содержимого"
    {
        return "Choose a .plz archive to view its contents".to_owned();
    }
    if status.starts_with("Сначала откройте архив") {
        return "Open a .plz archive first".to_owned();
    }
    if status.starts_with("Укажите путь к архиву") {
        return "Enter a path to a .plz archive".to_owned();
    }
    if status.starts_with("Укажите папку") {
        return "Choose a destination folder".to_owned();
    }
    if status.starts_with("Проверка BLAKE3 файлов и блоков") {
        return "Checking BLAKE3 files and blocks…".to_owned();
    }
    if status.starts_with("Проверка BLAKE3 пройдена") {
        return status
            .replace("Проверка BLAKE3 пройдена", "BLAKE3 check passed")
            .replace("файлов", "files")
            .replace("блоков", "blocks");
    }
    if let Some(rest) = status.strip_prefix("Открыт ") {
        return format!("Opened {}", rest.replace("объектов", "items"));
    }
    if let Some(rest) = status.strip_prefix("Ошибка проверки BLAKE3: ") {
        return format!("BLAKE3 verification error: {rest}");
    }
    if let Some(rest) = status.strip_prefix("Не удалось открыть ") {
        return format!("Could not open {}", rest.replace(": ", ": "));
    }
    if status.starts_with("Ошибка") || status.starts_with("Не удалось") {
        return format!(
            "Error: {}",
            status.split_once(": ").map_or(status, |(_, d)| d)
        );
    }
    if status.starts_with("Распаковка завершена") {
        return status
            .replace("Распаковка завершена", "Extraction complete")
            .replace("файлов", "files")
            .replace("каталогов", "folders")
            .replace("ссылок", "links")
            .replace("пропущено", "skipped");
    }
    status.to_owned()
}

fn format_size(bytes: u64, language: Language) -> String {
    if bytes >= 1024 * 1024 {
        format!(
            "{:.1} {}",
            bytes as f64 / (1024 * 1024) as f64,
            language.text("МиБ", "MiB")
        )
    } else if bytes >= 1024 {
        format!(
            "{:.1} {}",
            bytes as f64 / 1024.0,
            language.text("КиБ", "KiB")
        )
    } else if bytes == 0 {
        "—".to_owned()
    } else {
        format!("{bytes} {}", language.text("Б", "B"))
    }
}

fn accent_chip(accent: Accent, selected: Accent, kit: &Kit, language: Language) -> El {
    let color = accent.color();
    let label = accent.label(language);
    Button::new(format!("{} {label}", language.text("Акцент", "Accent")))
        .variant(if accent == selected {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::GhostMuted
        })
        .label_with(move |font, ink| {
            row()
                .center_y()
                .gap(4.0)
                .child(text("●").font(font).color(color))
                .child(text(label).font(font).color(ink))
        })
        .on_press(move |state: &mut State| state.accent = accent)
        .build(kit)
}

fn setting_option(
    label: &'static str,
    selected: bool,
    action: impl Fn(&mut State) + 'static,
    kit: &Kit,
) -> El {
    Button::new(label)
        .control_size(ControlSize::Small)
        .variant(if selected {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::GhostMuted
        })
        .on_press(action)
        .build(kit)
}

fn settings_tab(label: &'static str, section: SettingsSection, state: &State, kit: &Kit) -> El {
    Button::new(label)
        .control_size(ControlSize::Large)
        .variant(if state.settings_section == section {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::GhostMuted
        })
        .on_press(move |state: &mut State| state.settings_section = section)
        .build(kit)
        .grow(1.0)
}

fn setting_card(title: &str, detail: &str, content: El, kit: &Kit, palette: ThemePalette) -> El {
    col()
        .grow(1.0)
        .fill(palette.panel)
        .border(1.0, palette.border)
        .radius(0.0)
        .padding(22.0)
        .gap(14.0)
        .child(text(title).font(kit.ty.body_medium).color(palette.text))
        .child(text(detail).font(kit.ty.small).color(palette.muted))
        .child(content)
}

fn ui_color(value: &str) -> Color {
    rune::color::hex(value).expect("valid hard-coded UI color")
}

fn metric_card(
    label: &str,
    value: String,
    kit: &Kit,
    highlighted: bool,
    palette: ThemePalette,
) -> El {
    col()
        .grow(1.0)
        .fill(palette.panel)
        .border(1.0, palette.border)
        .radius(10.0)
        .padding(12.0)
        .gap(5.0)
        .child(text(label).font(kit.ty.small).color(kit.c.text_muted))
        .child(
            text(value)
                .font(TextStyle::new(19.0, 600.0))
                .color(if highlighted {
                    kit.c.text_accent
                } else {
                    kit.c.text_base
                }),
        )
}

fn choose_archive(state: &mut State, tx: &mut Tx) {
    tx.open_file(
        FileDialogOptions {
            title: state
                .language
                .text("Открыть архив PlicZip", "Open a PlicZip archive")
                .to_owned(),
            ..Default::default()
        }
        .filter(
            state.language.text("Архив PlicZip", "PlicZip archive"),
            &["plz"],
        ),
        |state: &mut State, result, _| match result {
            Ok(Some(path)) => state.open_archive(path),
            Ok(None) => {}
            Err(error) => state.status = format!("Не удалось открыть диалог: {error}"),
        },
    );
}

fn archive_table(state: &State, kit: &Kit, palette: ThemePalette, language: Language) -> El {
    let warning = kit.c.warning.fg;

    let table = Table::new(
        |state: &State| &state.table,
        |state: &mut State| &mut state.table,
        |state: &State| state.visible.len(),
        |state: &State, index| Cow::Borrowed(state.entries[state.visible[index]].id.as_str()),
    )
    .name(language.text("Файлы архива", "Archive files"))
    .column(
        TableColumn::new("name", language.text("Имя", "Name"))
            .flex(2.0)
            .min(120.0)
            .sortable(),
    )
    .column(
        TableColumn::new("type", language.text("Тип", "Type"))
            .width(110.0)
            .min(90.0)
            .sortable(),
    )
    .column(
        TableColumn::new("size", language.text("Размер", "Size"))
            .width(110.0)
            .min(80.0)
            .align(Align::End)
            .sortable(),
    )
    .column(
        TableColumn::new("encrypted", language.text("Шифрование", "Encryption"))
            .width(155.0)
            .resizable(false)
            .sortable(),
    )
    .cell(move |state: &State, index, column| {
        let entry = &state.entries[state.visible[index]];
        match column {
            "name" => Cell::text(entry.name.as_str()),
            "type" => Cell::text(kind_label(entry.kind, language)).muted(),
            "size" => Cell::text(format_size(entry.size, language)).mono(),
            "encrypted" => {
                if state.archive_encrypted {
                    Cell::text(language.text("Зашифрован", "Encrypted")).color(warning)
                } else {
                    Cell::text(language.text("Не зашифрован", "Not encrypted")).muted()
                }
            }
            _ => Cell::text(""),
        }
    })
    .on_sort(|state: &mut State, column, direction, _| {
        sort_entries(state, column, direction);
    })
    .zebra(true)
    .build(state, kit)
    .grow(1.0)
    .min_h(0.0);

    col()
        .key("archive-table")
        .w_full()
        .h_full()
        .fill(palette.table)
        .border(1.0, palette.border)
        .radius(10.0)
        .padding(6.0)
        .child(table)
}

fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let palette = theme_palette(state.theme_mode);
    let language = state.language;
    let mut kit = theme(state.accent, state.theme_mode);
    let page = palette.page;
    let panel = palette.panel;
    let raised = palette.raised;
    let border = palette.border;
    let muted = palette.muted;
    kit.c.page = page;
    kit.c.bg_base = panel;
    kit.c.layer_01 = panel;
    kit.c.layer_02 = raised;
    kit.c.bg_button_neutral = raised;
    kit.c.surface_base = panel;
    kit.c.surface_weak = panel;
    kit.c.surface_raised = raised;
    kit.c.border_base = border;
    kit.c.border_muted = border;
    kit.c.text_base = palette.text;
    kit.c.text_muted = muted;
    kit.c.text_accent = state.accent.color();
    let selected = state.table.selected_len();

    let open_button = Button::new(language.text("Открыть архив", "Open archive"))
        .variant(ButtonVariant::Contrast)
        .on_press_tx(choose_archive)
        .build(&kit);
    let load_button = Button::new(language.text("Загрузить", "Load"))
        .variant(ButtonVariant::Neutral)
        .on_press(|state: &mut State| state.open_path_input())
        .build(&kit);
    let path_field = col()
        .grow(1.0)
        .h(38.0)
        .center_y()
        .fill(palette.field)
        .border(1.0, border)
        .radius(8.0)
        .padding(8.0)
        .child(
            text_input(&state.path_input, TextStyle::new(13.0, 400.0))
                .key("archive-path")
                .label_static(language.text("Путь к архиву .plz", "Path to .plz archive"))
                .w_full()
                .h(26.0)
                .single_line()
                .placeholder(
                    language.text("Путь к архиву .plz", "Path to .plz archive"),
                    kit.c.text_muted,
                )
                .on_edit(|state: &mut State| &mut state.path_input)
                .on_submit(|state: &mut State, _| state.open_path_input()),
        );

    let status_tone = status_kind(&state.status);
    let status_color = match status_tone {
        StatusKind::Neutral => muted,
        StatusKind::Success => state.accent.color(),
        StatusKind::Error => ui_color("#FF7777"),
    };
    let displayed_status = localized_status(&state.status, language);
    let status_line = row()
        .key("status-line")
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            text(if status_tone == StatusKind::Error {
                "!"
            } else {
                "●"
            })
            .color(status_color),
        )
        .child(text(displayed_status).font(kit.ty.small).color(
            if status_tone == StatusKind::Error {
                status_color
            } else {
                kit.c.text_muted
            },
        ));

    let source_card = col()
        .key("source-card")
        .w_full()
        .fill(panel)
        .border(1.0, border)
        .radius(12.0)
        .padding(14.0)
        .gap(10.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text(language.text("ИСТОЧНИК АРХИВА", "ARCHIVE SOURCE"))
                        .font(kit.ty.small)
                        .color(kit.c.text_muted),
                )
                .child(
                    text(state.archive_path.as_ref().map_or(
                        language.text("ФОРМАТ .PLZ", "FORMAT .PLZ").to_owned(),
                        |path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string())
                        },
                    ))
                    .font(kit.ty.mono)
                    .color(kit.c.text_base),
                ),
        )
        .child(
            row()
                .key("toolbar")
                .w_full()
                .center_y()
                .gap(9.0)
                .child(open_button)
                .child(path_field)
                .child(load_button),
        )
        .child(status_line);

    let metrics = row()
        .key("archive-metrics")
        .w_full()
        .gap(9.0)
        .child(metric_card(
            language.text("ЗАПИСЕЙ В АРХИВЕ", "ARCHIVE ENTRIES"),
            state.entries.len().to_string(),
            &kit,
            false,
            palette,
        ))
        .child(metric_card(
            language.text("ПОСЛЕ ФИЛЬТРА", "AFTER FILTER"),
            format!("{} / {}", state.visible.len(), state.entries.len()),
            &kit,
            false,
            palette,
        ))
        .child(metric_card(
            language.text("ВЫБРАНО", "SELECTED"),
            selected.to_string(),
            &kit,
            selected > 0,
            palette,
        ))
        .child(metric_card(
            language.text("СОСТОЯНИЕ ЗАЩИТЫ", "PROTECTION"),
            if state.archive_path.is_none() {
                "—".to_owned()
            } else if state.archive_encrypted {
                language.text("Включена", "Enabled").to_owned()
            } else {
                language.text("Не зашифрован", "Not encrypted").to_owned()
            },
            &kit,
            state.archive_encrypted,
            palette,
        ));

    let extract_card = if state.archive_path.is_some() && !state.archive_encrypted {
        col()
            .key("extract-card")
            .w_full()
            .fill(panel)
            .border(1.0, border)
            .radius(12.0)
            .padding(13.0)
            .gap(9.0)
            .child(
                text(language.text("РАСПАКОВКА", "EXTRACTION"))
                    .font(kit.ty.small)
                    .color(kit.c.text_muted),
            )
            .child(
                row()
                    .key("extract-row")
                    .w_full()
                    .center_y()
                    .gap(9.0)
                    .child(
                        col()
                            .grow(1.0)
                            .h(38.0)
                            .center_y()
                            .fill(palette.field)
                            .border(1.0, border)
                            .radius(8.0)
                            .padding(8.0)
                            .child(
                                text_input(&state.output_input, TextStyle::new(13.0, 400.0))
                                    .key("output-path")
                                    .label_static(
                                        language.text("Папка для распаковки", "Extraction folder"),
                                    )
                                    .w_full()
                                    .h(26.0)
                                    .single_line()
                                    .placeholder(
                                        language.text("Папка назначения", "Destination folder"),
                                        kit.c.text_muted,
                                    )
                                    .on_edit(|state: &mut State| &mut state.output_input)
                                    .on_submit(|state: &mut State, _| state.extract_all()),
                            ),
                    )
                    .child(
                        Button::new(language.text("Извлечь всё", "Extract all"))
                            .variant(ButtonVariant::Neutral)
                            .on_press(|state: &mut State| state.extract_all())
                            .build(&kit),
                    ),
            )
    } else {
        text("")
    };

    let table_area = if state.archive_path.is_some() && !state.visible.is_empty() {
        archive_table(state, &kit, palette, language)
    } else {
        let (symbol, hint, detail) = if state.archive_path.is_some() && state.entries.is_empty() {
            (
                "□",
                language.text("Архив пуст", "Archive is empty"),
                language.text(
                    "В выбранном архиве нет записей",
                    "The selected archive has no entries",
                ),
            )
        } else if state.archive_path.is_some() {
            (
                "⌕",
                language.text("Ничего не найдено", "No matches found"),
                language.text(
                    "Измените запрос поиска, чтобы найти нужную запись",
                    "Change your search to find an entry",
                ),
            )
        } else {
            (
                "P",
                language.text("Откройте архив PlicZip", "Open a PlicZip archive"),
                language.text(
                    "Выберите файл .plz, чтобы увидеть его содержимое",
                    "Choose a .plz file to browse its contents",
                ),
            )
        };
        let empty_icon = if state.archive_path.is_none() {
            col()
                .size(66.0, 66.0)
                .center()
                .fill(raised)
                .border(1.0, border)
                .radius(18.0)
                .child(
                    text(symbol)
                        .font(TextStyle::new(38.0, 400.0).family(Family::Named("Deutsch Gothic")))
                        .color(state.accent.color()),
                )
        } else {
            col().size(54.0, 54.0).center().child(
                text(symbol)
                    .font(TextStyle::new(34.0, 600.0))
                    .color(state.accent.color()),
            )
        };
        let empty_state = col()
            .key("empty-state")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(10.0)
            .child(empty_icon)
            .child(
                text(hint)
                    .font(TextStyle::new(17.0, 600.0))
                    .color(kit.c.text_base),
            )
            .child(text(detail).font(kit.ty.small).color(kit.c.text_muted));
        if state.archive_path.is_none() {
            empty_state.child(
                Button::new(language.text("Выбрать архив", "Choose archive"))
                    .variant(ButtonVariant::Contrast)
                    .on_press_tx(choose_archive)
                    .build(&kit),
            )
        } else {
            empty_state
        }
    };

    let library_card = col()
        .key("library-card")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(panel)
        .border(1.0, border)
        .radius(12.0)
        .padding(12.0)
        .gap(10.0)
        .child(
            col()
                .w_full()
                .gap(9.0)
                .child(
                    col()
                        .gap(2.0)
                        .child(
                            text(language.text("Содержимое архива", "Archive contents"))
                                .font(kit.ty.body_medium)
                                .color(kit.c.text_base),
                        )
                        .child(
                            text(language.text(
                                "Просмотр структуры и свойств записей",
                                "Browse entries and metadata",
                            ))
                            .font(kit.ty.small)
                            .color(kit.c.text_muted),
                        ),
                )
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .gap(7.0)
                        .child(
                            text(language.text("ПОИСК", "SEARCH"))
                                .font(kit.ty.small)
                                .color(kit.c.text_muted),
                        )
                        .child(
                            col()
                                .w(190.0)
                                .min_w(0.0)
                                .h(38.0)
                                .center_y()
                                .fill(palette.field)
                                .border(1.0, border)
                                .radius(8.0)
                                .padding(8.0)
                                .child(
                                    text_input(&state.filter_input, TextStyle::new(13.0, 400.0))
                                        .key("archive-filter")
                                        .label_static(language.text(
                                            "Поиск по имени или пути",
                                            "Search by name or path",
                                        ))
                                        .w_full()
                                        .h(26.0)
                                        .single_line()
                                        .placeholder(
                                            language.text("Имя или часть пути", "Name or path"),
                                            kit.c.text_muted,
                                        )
                                        .on_edit(|state: &mut State| &mut state.filter_input)
                                        .on_input_change(|state: &mut State, _| {
                                            state.refresh_visible()
                                        })
                                        .on_submit(|state: &mut State, _| state.refresh_visible()),
                                ),
                        ),
                ),
        )
        .child(
            row()
                .key("table-meta")
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    text(if state.archive_path.is_some() {
                        language.text(
                            "ИМЯ · ТИП · РАЗМЕР · ШИФРОВАНИЕ",
                            "NAME · TYPE · SIZE · ENCRYPTION",
                        )
                    } else {
                        language.text("ГОТОВО К ПРОСМОТРУ", "READY TO BROWSE")
                    })
                    .font(kit.ty.small)
                    .color(kit.c.text_muted),
                )
                .child(
                    text(format!(
                        "{} {}",
                        state.visible.len(),
                        language.text("записей", "items")
                    ))
                    .font(kit.ty.small)
                    .color(kit.c.text_muted),
                ),
        )
        .child(
            col()
                .key("table-frame")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .fill(palette.table)
                .radius(10.0)
                .border(1.0, border)
                .child(table_area),
        );

    let settings_overlay = if state.settings_open {
        let language_options = row()
            .gap(10.0)
            .child(setting_option(
                "Русский",
                state.language == Language::Russian,
                |state| state.language = Language::Russian,
                &kit,
            ))
            .child(setting_option(
                "English",
                state.language == Language::English,
                |state| state.language = Language::English,
                &kit,
            ));
        let theme_options = row()
            .gap(10.0)
            .child(setting_option(
                language.text("Светлая", "Light"),
                state.theme_mode == ThemeMode::Light,
                |state| state.theme_mode = ThemeMode::Light,
                &kit,
            ))
            .child(setting_option(
                language.text("Тёмная", "Dark"),
                state.theme_mode == ThemeMode::Dark,
                |state| state.theme_mode = ThemeMode::Dark,
                &kit,
            ))
            .child(setting_option(
                language.text("Космос", "Cosmos"),
                state.theme_mode == ThemeMode::Cosmos,
                |state| state.theme_mode = ThemeMode::Cosmos,
                &kit,
            ));
        let accent_options = row()
            .gap(8.0)
            .child(accent_chip(Accent::Blue, state.accent, &kit, language))
            .child(accent_chip(Accent::Green, state.accent, &kit, language))
            .child(accent_chip(Accent::Orange, state.accent, &kit, language));
        let animations_option = row()
            .w_full()
            .center_y()
            .justify_between()
            .child(text(
                language.text("Анимации интерфейса", "Interface animations"),
            ))
            .child(setting_option(
                if state.animations_enabled {
                    language.text("Вкл.", "On")
                } else {
                    language.text("Выкл.", "Off")
                },
                state.animations_enabled,
                |state| state.animations_enabled = !state.animations_enabled,
                &kit,
            ));
        let verify_option = row()
            .w_full()
            .center_y()
            .justify_between()
            .child(text(language.text(
                "Проверять BLAKE3 при открытии архива",
                "Verify BLAKE3 when opening an archive",
            )))
            .child(setting_option(
                if state.auto_verify {
                    language.text("Вкл.", "On")
                } else {
                    language.text("Выкл.", "Off")
                },
                state.auto_verify,
                |state| state.auto_verify = !state.auto_verify,
                &kit,
            ));

        let destination_field = col()
            .w_full()
            .h(52.0)
            .center_y()
            .fill(palette.field)
            .border(1.0, border)
            .padding(12.0)
            .child(
                text_input(&state.output_input, TextStyle::new(15.0, 400.0))
                    .key("settings-extraction-path")
                    .label_static(language.text("Папка назначения", "Destination folder"))
                    .w_full()
                    .h(30.0)
                    .single_line()
                    .placeholder(
                        language.text("Например: ./extracted", "For example: ./extracted"),
                        kit.c.text_muted,
                    )
                    .on_edit(|state: &mut State| &mut state.output_input),
            );
        let destination_section = row()
            .w_full()
            .grow(1.0)
            .gap(16.0)
            .child(setting_card(
                language.text("ПАПКА РАСПАКОВКИ", "EXTRACTION FOLDER"),
                language.text(
                    "Путь, который будет подставлен для распаковки текущего архива",
                    "Default destination path for extracting the current archive",
                ),
                destination_field,
                &kit,
                palette,
            ))
            .child(setting_card(
                language.text("ТЕКУЩИЙ АРХИВ", "CURRENT ARCHIVE"),
                language.text(
                    "Предложенная папка назначения зависит от расположения архива",
                    "The suggested destination follows the archive location",
                ),
                text(state.archive_path.as_ref().map_or_else(
                    || {
                        language
                            .text("Архив ещё не открыт", "No archive is open")
                            .to_owned()
                    },
                    |path| path.display().to_string(),
                ))
                .color(palette.text),
                &kit,
                palette,
            ));

        let section_content = match state.settings_section {
            SettingsSection::General => row()
                .w_full()
                .grow(1.0)
                .gap(16.0)
                .child(setting_card(
                    language.text("ЯЗЫК ИНТЕРФЕЙСА", "INTERFACE LANGUAGE"),
                    language.text(
                        "Выберите язык приложения",
                        "Choose the application language",
                    ),
                    language_options,
                    &kit,
                    palette,
                ))
                .child(setting_card(
                    language.text("АНИМАЦИИ", "ANIMATIONS"),
                    language.text(
                        "Включайте движения и переходы интерфейса",
                        "Enable interface motion and transitions",
                    ),
                    animations_option,
                    &kit,
                    palette,
                )),
            SettingsSection::Appearance => row()
                .w_full()
                .grow(1.0)
                .gap(16.0)
                .child(setting_card(
                    language.text("ТЕМА", "THEME"),
                    language.text(
                        "Зернистость сохраняется в каждой теме",
                        "Grain remains present in every theme",
                    ),
                    theme_options,
                    &kit,
                    palette,
                ))
                .child(setting_card(
                    language.text("АКЦЕНТНЫЙ ЦВЕТ", "ACCENT COLOR"),
                    language.text(
                        "Цвет отметок и важных состояний",
                        "Highlights and status color",
                    ),
                    accent_options,
                    &kit,
                    palette,
                )),
            SettingsSection::Extraction => destination_section,
            SettingsSection::Security => row()
                .w_full()
                .grow(1.0)
                .gap(16.0)
                .child(setting_card(
                    language.text("ПРОВЕРКА BLAKE3", "BLAKE3 VERIFICATION"),
                    language.text(
                        "Автоматическая проверка целостности при открытии архива",
                        "Automatically verify archive integrity when opening",
                    ),
                    col().w_full().gap(16.0).child(verify_option).child(
                        Button::new(language.text("Проверить сейчас", "Verify now"))
                            .variant(ButtonVariant::Neutral)
                            .on_press(|state: &mut State| state.verify_blake3())
                            .build(&kit),
                    ),
                    &kit,
                    palette,
                ))
                .child(setting_card(
                    language.text("СОСТОЯНИЕ АРХИВА", "ARCHIVE STATUS"),
                    language.text("Текущая проверка", "Current verification"),
                    text(localized_status(&state.status, language)).color(palette.text),
                    &kit,
                    palette,
                )),
        };
        let tabs = row()
            .w_full()
            .gap(8.0)
            .child(settings_tab(
                language.text("Общие", "General"),
                SettingsSection::General,
                state,
                &kit,
            ))
            .child(settings_tab(
                language.text("Внешний вид", "Appearance"),
                SettingsSection::Appearance,
                state,
                &kit,
            ))
            .child(settings_tab(
                language.text("Распаковка", "Extraction"),
                SettingsSection::Extraction,
                state,
                &kit,
            ))
            .child(settings_tab(
                language.text("Безопасность", "Security"),
                SettingsSection::Security,
                state,
                &kit,
            ));

        Some(
            col()
                .key("settings-overlay")
                .absolute(0.0, 0.0)
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .fill(ui_color("#000000B8"))
                .pixels(settings_grain_texture(state.theme_mode))
                .padding(28.0)
                .gap(20.0)
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .justify_between()
                        .child(
                            col()
                                .gap(5.0)
                                .child(
                                    text(language.text("НАСТРОЙКИ", "SETTINGS"))
                                        .font(TextStyle::new(28.0, 600.0))
                                        .color(palette.text),
                                )
                                .child(
                                    text(language.text(
                                        "Настройте PlicZip под себя",
                                        "Customize PlicZip to your preferences",
                                    ))
                                    .font(kit.ty.body)
                                    .color(palette.muted),
                                ),
                        )
                        .child(
                            Button::new(language.text("Закрыть  ×", "Close  ×"))
                                .variant(ButtonVariant::GhostMuted)
                                .on_press(|state: &mut State| state.settings_open = false)
                                .build(&kit),
                        ),
                )
                .child(tabs)
                .child(section_content),
        )
    } else {
        None
    };
    let settings_button = IconButton::new(&SETTINGS_ICON)
        .variant(IconButtonVariant::GhostMuted)
        .label(language.text("Настройки", "Settings"))
        .help(language.text("Открыть настройки", "Open settings"))
        .expanded(state.settings_open)
        .on_press(|state: &mut State| state.settings_open = !state.settings_open)
        .build(&kit)
        .key("settings-button");
    let (shield_icon, shield_color, shield_key) = if state.verify_success {
        (&BLAKE3_OK_ICON, ui_color("#83C98B"), "shield-ok")
    } else if state.verify_failed {
        (&BLAKE3_ERROR_ICON, ui_color("#E56B6F"), "shield-error")
    } else {
        (&BLAKE3_ICON, kit.c.text_muted, "shield-idle")
    };
    let animations_enabled = state.animations_enabled;
    let shield_rotation = if state.animations_enabled && state.shield_hover {
        0.16
    } else {
        0.0
    };
    let shield_button =
        Button::new(language.text("Проверить целостность BLAKE3", "Verify BLAKE3 integrity"))
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label_with(move |_font, _ink| {
                row().center_y().child(
                    icon(shield_icon, 14.0)
                        .key(shield_key)
                        .color(shield_color)
                        .rotate(shield_rotation)
                        .rotate_motion(rune::Spring::new(0.32, 0.0))
                        .transition(rune::Transition::scale(if animations_enabled {
                            0.82
                        } else {
                            1.0
                        })),
                )
            })
            .on_press(|state: &mut State| state.verify_blake3())
            .build(&kit)
            .size(20.0, 20.0)
            .key("blake3-shield-button")
            .on_hover_change(|state: &mut State, hovering, tx| {
                if state.shield_hover != hovering {
                    state.shield_hover = hovering;
                    tx.color_motion(rune::Spring::new(0.24, 0.0));
                }
            });
    let footer_actions = row()
        .center_y()
        .gap(5.0)
        .child(shield_button)
        .child(text("|").font(kit.ty.small).color(kit.c.text_muted))
        .child(settings_button);
    let footer = row()
        .key("status-footer")
        .w_full()
        .center_y()
        .justify_between()
        .child(
            text(format!(
                "{} {} · {} {}",
                state.visible.len(),
                language.text("из", "of"),
                state.entries.len(),
                language.text("выбрано", "selected")
            ))
            .font(kit.ty.small)
            .color(kit.c.text_muted),
        )
        .child(footer_actions);

    let main_content = col()
        .key("main-content")
        .w_full()
        .grow(1.0)
        .h_full()
        .padding(16.0)
        .gap(10.0)
        .child(source_card)
        .child(metrics)
        .child(extract_card)
        .child(library_card)
        .child(footer);
    col()
        .key("root")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .fill(page)
        .pixels(grain_texture(state.theme_mode))
        .child(main_content)
        .children(settings_overlay)
}

fn main() {
    rune::set_app_icon(include_bytes!("../../assets/pliczip-icon.png").to_vec())
        .expect("embedded PlicZip icon must be a valid PNG");
    let mut spec = AppSpec::new(
        "PlicZip · Предпросмотр Rune",
        (1180.0, 960.0),
        State::default,
        view,
    );
    spec.min_size = Some((700.0, 760.0));
    spec.fonts = GUI_FONTS;
    spec.families = ("Intro", "ui-monospace");
    let app = rune::App::new(spec);
    let _ = APP_HANDLE.set(app.handle());
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliczip::prelude::CreateOptions;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn status_tone_distinguishes_errors_and_success() {
        assert_eq!(status_kind("Не удалось открыть архив"), StatusKind::Error);
        assert_eq!(status_kind("Ошибка распаковки"), StatusKind::Error);
        assert_eq!(
            status_kind("Открыт sample.plz · 3 объектов"),
            StatusKind::Success
        );
        assert_eq!(
            status_kind("Выберите архив .plz для просмотра его содержимого"),
            StatusKind::Neutral
        );
    }

    #[test]
    fn opens_native_archive_and_populates_table_rows() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("pliczip-rune-test-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("notes.txt");
        let archive_path = root.join("sample.plz");
        std::fs::write(&input, b"Rune reads the real PlicZip index").unwrap();
        pliczip::writer::create(
            &archive_path,
            std::slice::from_ref(&input),
            &CreateOptions {
                show_progress: false,
                ..Default::default()
            },
        )
        .unwrap();

        let mut state = State::default();
        state.path_input = TextEdit::new(archive_path.display().to_string());
        state.open_path_input();

        assert_eq!(state.entries.len(), 1, "{}", state.status);
        assert_eq!(state.entries[0].name, "notes.txt");
        assert_eq!(state.entries[0].size, 33);
        assert_eq!(state.archive_path, Some(archive_path));
        assert_eq!(state.visible, vec![0]);

        state.filter_input = TextEdit::new("NOTES.TXT");
        state.refresh_visible();
        assert_eq!(state.visible, vec![0]);
        state.filter_input = TextEdit::new("missing");
        state.refresh_visible();
        assert!(state.visible.is_empty());

        let output = root.join("out");
        state.output_input = TextEdit::new(output.display().to_string());
        state.extract_all();
        assert_eq!(
            std::fs::read(output.join("notes.txt")).unwrap(),
            b"Rune reads the real PlicZip index"
        );
        assert!(state.status.contains("файлов: 1"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sorting_changes_only_indices_and_preserves_filter_order() {
        let mut state = State::default();
        state.entries = vec![
            ArchiveEntry {
                id: "0".to_owned(),
                name: "file-10".to_owned(),
                kind: EntryKind::File,
                size: 100,
            },
            ArchiveEntry {
                id: "1".to_owned(),
                name: "file-2".to_owned(),
                kind: EntryKind::File,
                size: 10,
            },
            ArchiveEntry {
                id: "2".to_owned(),
                name: "file-1".to_owned(),
                kind: EntryKind::File,
                size: 10,
            },
        ];
        state.refresh_visible();

        sort_entries(&mut state, "size", SortDir::Ascending);
        assert_eq!(state.order.as_deref(), Some(&[1, 2, 0][..]));
        assert_eq!(state.visible, vec![1, 2, 0]);
        assert_eq!(
            state
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["0", "1", "2"]
        );

        state.filter_input = TextEdit::new("file-1");
        state.refresh_visible();
        assert_eq!(state.visible, vec![2, 0]);
        state.filter_input = TextEdit::new("");
        state.refresh_visible();
        assert_eq!(state.visible, vec![1, 2, 0]);

        sort_entries(&mut state, "name", SortDir::Ascending);
        assert_eq!(state.order.as_deref(), Some(&[2, 1, 0][..]));
        assert_eq!(state.entries[0].id, "0");
    }
}
