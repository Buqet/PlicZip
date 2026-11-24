#![forbid(unsafe_code)]

//! Первый рабочий вертикальный срез Rune-интерфейса PlicZip.
//!
//! Таблица отображает реальные записи native `.plz` архива. Создание и извлечение
//! пока остаются в основной egui-оболочке и будут перенесены отдельными этапами.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use rune::{
    col, row, text, text_input, Align, AppSpec, Color, Cx, El, FileDialogOptions, ScrollSpec,
    TextEdit, TextStyle, Tx,
};
use rune_kit::{
    Button, ButtonVariant, Cell, Kit, PaletteFocus, PaletteSeed, SortDir, Table, TableColumn,
    TableState,
};

static GUI_FONTS: &[&[u8]] = &[include_bytes!("../assets/Intro.otf")];

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

    fn label(self) -> &'static str {
        match self {
            Self::Blue => "Синий",
            Self::Green => "Зелёный",
            Self::Orange => "Оранжевый",
        }
    }
}

fn theme(accent: Accent) -> Kit {
    let background = rune::color::hex("#101214").expect("valid hard-coded background color");
    let foreground = rune::color::hex("#F1F2F3").expect("valid hard-coded foreground color");
    let mut kit = Kit::from_palette_focus(
        PaletteSeed {
            bg: background,
            fg: foreground,
            accent: accent.color(),
        },
        PaletteFocus::Accent,
    );
    kit.c.bg_icon_button_contrast = accent.color();
    kit.c.submit_fg = background;
    kit.c.submit_icon = background;
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
    table: TableState,
    accent: Accent,
    archive_path: Option<PathBuf>,
    archive_encrypted: bool,
    path_input: TextEdit,
    output_input: TextEdit,
    filter_input: TextEdit,
    status: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            visible: Vec::new(),
            table: TableState::new().multi_select(true),
            accent: Accent::Blue,
            archive_path: None,
            archive_encrypted: false,
            path_input: TextEdit::new(""),
            output_input: TextEdit::new(""),
            filter_input: TextEdit::new(""),
            status: "Выберите архив .plz для просмотра его содержимого".to_owned(),
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

    fn refresh_visible(&mut self) {
        let query = self.filter_input.text.trim().to_lowercase();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                query.is_empty() || entry.name.to_lowercase().contains(query.as_str())
            })
            .map(|(index, _)| index)
            .collect();
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
                self.filter_input = TextEdit::new("");
                self.output_input =
                    TextEdit::new(suggested_output_path(&path).display().to_string());
                self.table = TableState::new().multi_select(true);
                self.archive_path = Some(path.clone());
                self.archive_encrypted = encrypted;
                self.refresh_visible();
                self.status = format!("Открыт {} · {count} объектов", path.display());
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
    let compare = |left: &ArchiveEntry, right: &ArchiveEntry| match column {
        "name" => rune_kit::natural_cmp(&left.name, &right.name),
        "type" => kind_name(left.kind).cmp(kind_name(right.kind)),
        "size" => left.size.cmp(&right.size),
        // Protection is an archive-level property, so every row has the same value.
        "encrypted" => std::cmp::Ordering::Equal,
        _ => std::cmp::Ordering::Equal,
    };

    state
        .entries
        .sort_by(|left, right| direction.apply(compare(left, right)));
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

fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "Файл",
        EntryKind::Directory => "Каталог",
        EntryKind::Link => "Ссылка",
        EntryKind::Other => "Другое",
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
    } else if status.starts_with("Открыт") || status.starts_with("Распаковка завершена")
    {
        StatusKind::Success
    } else {
        StatusKind::Neutral
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} МиБ", bytes as f64 / (1024 * 1024) as f64)
    } else if bytes >= 1024 {
        format!("{:.1} КиБ", bytes as f64 / 1024.0)
    } else if bytes == 0 {
        "—".to_owned()
    } else {
        format!("{bytes} Б")
    }
}

fn accent_chip(accent: Accent, selected: Accent, kit: &Kit) -> El {
    let color = accent.color();
    let label = accent.label();
    Button::new(format!("Акцент {label}"))
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

fn ui_color(value: &str) -> Color {
    rune::color::hex(value).expect("valid hard-coded UI color")
}

fn metric_card(label: &str, value: String, kit: &Kit, highlighted: bool) -> El {
    col()
        .grow(1.0)
        .fill(ui_color("#171A1D"))
        .border(1.0, ui_color("#282D31"))
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

fn archive_table(state: &State, kit: &Kit) -> El {
    let warning = kit.c.warning.fg;

    let table = Table::new(
        |state: &State| &state.table,
        |state: &mut State| &mut state.table,
        |state: &State| state.visible.len(),
        |state: &State, index| Cow::Borrowed(state.entries[state.visible[index]].id.as_str()),
    )
    .name("Файлы архива")
    .column(
        TableColumn::new("name", "Имя")
            .flex(2.0)
            .min(120.0)
            .sortable(),
    )
    .column(
        TableColumn::new("type", "Тип")
            .width(110.0)
            .min(90.0)
            .sortable(),
    )
    .column(
        TableColumn::new("size", "Размер")
            .width(110.0)
            .min(80.0)
            .align(Align::End)
            .sortable(),
    )
    .column(
        TableColumn::new("encrypted", "Шифрование")
            .width(155.0)
            .resizable(false)
            .sortable(),
    )
    .cell(move |state: &State, index, column| {
        let entry = &state.entries[state.visible[index]];
        match column {
            "name" => Cell::text(entry.name.as_str()),
            "type" => Cell::text(kind_label(entry.kind)).muted(),
            "size" => Cell::text(format_size(entry.size)).mono(),
            "encrypted" => {
                if state.archive_encrypted {
                    Cell::text("Зашифрован").color(warning)
                } else {
                    Cell::text("Не зашифрован").muted()
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
        .fill(ui_color("#111315"))
        .border(1.0, ui_color("#282D31"))
        .radius(10.0)
        .padding(6.0)
        .child(table)
}

fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let mut kit = theme(state.accent);
    let page = ui_color("#0C0E10");
    let panel = ui_color("#141719");
    let raised = ui_color("#1A1D20");
    let border = ui_color("#292E32");
    let muted = ui_color("#91989F");
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
    kit.c.text_base = ui_color("#ECEFF1");
    kit.c.text_muted = muted;
    kit.c.text_accent = state.accent.color();
    let selected = state.table.selected_len();
    let accent_controls = row()
        .key("accent-controls")
        .center_y()
        .gap(3.0)
        .child(accent_chip(Accent::Blue, state.accent, &kit))
        .child(accent_chip(Accent::Green, state.accent, &kit))
        .child(accent_chip(Accent::Orange, state.accent, &kit));

    let open_button = Button::new("Открыть архив")
        .variant(ButtonVariant::Contrast)
        .on_press_tx(|_state: &mut State, tx: &mut Tx| {
            tx.open_file(
                FileDialogOptions {
                    title: "Открыть архив PlicZip".to_owned(),
                    ..Default::default()
                }
                .filter("Архив PlicZip", &["plz"]),
                |state: &mut State, result, _| match result {
                    Ok(Some(path)) => state.open_archive(path),
                    Ok(None) => {}
                    Err(error) => state.status = format!("Не удалось открыть диалог: {error}"),
                },
            );
        })
        .build(&kit);
    let load_button = Button::new("Загрузить")
        .variant(ButtonVariant::Neutral)
        .on_press(|state: &mut State| state.open_path_input())
        .build(&kit);
    let path_field = col()
        .grow(1.0)
        .h(38.0)
        .center_y()
        .fill(ui_color("#0E1012"))
        .border(1.0, border)
        .radius(8.0)
        .padding(8.0)
        .child(
            text_input(&state.path_input, TextStyle::new(13.0, 400.0))
                .key("archive-path")
                .label_static("Путь к архиву .plz")
                .w_full()
                .h(26.0)
                .single_line()
                .placeholder("Путь к архиву .plz", kit.c.text_muted)
                .on_edit(|state: &mut State| &mut state.path_input)
                .on_submit(|state: &mut State, _| state.open_path_input()),
        );

    let status_tone = status_kind(&state.status);
    let status_color = match status_tone {
        StatusKind::Neutral => muted,
        StatusKind::Success => state.accent.color(),
        StatusKind::Error => ui_color("#FF7777"),
    };
    let status_line =
        row()
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
            .child(text(&state.status).font(kit.ty.small).color(
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
                    text("ИСТОЧНИК АРХИВА")
                        .font(kit.ty.small)
                        .color(kit.c.text_muted),
                )
                .child(
                    text(state.archive_path.as_ref().map_or(
                        "ФОРМАТ .PLZ".to_owned(),
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
            "ЗАПИСЕЙ В АРХИВЕ",
            state.entries.len().to_string(),
            &kit,
            false,
        ))
        .child(metric_card(
            "ПОСЛЕ ФИЛЬТРА",
            format!("{} / {}", state.visible.len(), state.entries.len()),
            &kit,
            false,
        ))
        .child(metric_card(
            "ВЫБРАНО",
            selected.to_string(),
            &kit,
            selected > 0,
        ))
        .child(metric_card(
            "СОСТОЯНИЕ ЗАЩИТЫ",
            if state.archive_path.is_none() {
                "—".to_owned()
            } else if state.archive_encrypted {
                "Включена".to_owned()
            } else {
                "Не зашифрован".to_owned()
            },
            &kit,
            state.archive_encrypted,
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
                text("РАСПАКОВКА")
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
                            .fill(ui_color("#0E1012"))
                            .border(1.0, border)
                            .radius(8.0)
                            .padding(8.0)
                            .child(
                                text_input(&state.output_input, TextStyle::new(13.0, 400.0))
                                    .key("output-path")
                                    .label_static("Папка для распаковки")
                                    .w_full()
                                    .h(26.0)
                                    .single_line()
                                    .placeholder("Папка назначения", kit.c.text_muted)
                                    .on_edit(|state: &mut State| &mut state.output_input)
                                    .on_submit(|state: &mut State, _| state.extract_all()),
                            ),
                    )
                    .child(
                        Button::new("Извлечь всё")
                            .variant(ButtonVariant::Neutral)
                            .on_press(|state: &mut State| state.extract_all())
                            .build(&kit),
                    ),
            )
    } else {
        text("")
    };

    let table_area = if state.archive_path.is_some() && !state.visible.is_empty() {
        archive_table(state, &kit)
    } else {
        let (symbol, hint, detail) = if state.archive_path.is_some() && state.entries.is_empty() {
            ("□", "Архив пуст", "В выбранном архиве нет записей")
        } else if state.archive_path.is_some() {
            (
                "⌕",
                "Ничего не найдено",
                "Измените запрос поиска, чтобы найти нужную запись",
            )
        } else {
            (
                "P",
                "Архив не открыт",
                "Выберите файл .plz — его содержимое появится здесь",
            )
        };
        col()
            .key("empty-state")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(8.0)
            .child(
                text(symbol)
                    .font(TextStyle::new(34.0, 600.0))
                    .color(state.accent.color()),
            )
            .child(
                text(hint)
                    .font(TextStyle::new(17.0, 600.0))
                    .color(kit.c.text_base),
            )
            .child(text(detail).font(kit.ty.small).color(kit.c.text_muted))
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
                            text("Содержимое архива")
                                .font(kit.ty.body_medium)
                                .color(kit.c.text_base),
                        )
                        .child(
                            text("Просмотр структуры и свойств записей")
                                .font(kit.ty.small)
                                .color(kit.c.text_muted),
                        ),
                )
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .gap(7.0)
                        .child(text("ПОИСК").font(kit.ty.small).color(kit.c.text_muted))
                        .child(
                            col()
                                .w(190.0)
                                .min_w(0.0)
                                .h(38.0)
                                .center_y()
                                .fill(ui_color("#0E1012"))
                                .border(1.0, border)
                                .radius(8.0)
                                .padding(8.0)
                                .child(
                                    text_input(&state.filter_input, TextStyle::new(13.0, 400.0))
                                        .key("archive-filter")
                                        .label_static("Поиск по имени или пути")
                                        .w_full()
                                        .h(26.0)
                                        .single_line()
                                        .placeholder("Имя или часть пути", kit.c.text_muted)
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
                        "ИМЯ · ТИП · РАЗМЕР · ШИФРОВАНИЕ"
                    } else {
                        "ГОТОВО К ПРОСМОТРУ"
                    })
                    .font(kit.ty.small)
                    .color(kit.c.text_muted),
                )
                .child(
                    text(format!("{} записей", state.visible.len()))
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
                .fill(ui_color("#111315"))
                .radius(10.0)
                .border(1.0, border)
                .child(table_area),
        );

    let footer = row()
        .key("status-footer")
        .w_full()
        .center_y()
        .justify_between()
        .child(
            text("PLICZIP · РАБОЧЕЕ ПРОСТРАНСТВО АРХИВОВ")
                .font(kit.ty.small)
                .color(kit.c.text_muted),
        )
        .child(
            text(format!(
                "{} из {} · {} выбрано",
                state.visible.len(),
                state.entries.len(),
                selected
            ))
            .font(kit.ty.small)
            .color(kit.c.text_muted),
        );

    col()
        .key("root")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .fill(page)
        .child(
            col()
                .key("main-content")
                .w_full()
                .grow(1.0)
                .h_full()
                .padding(16.0)
                .gap(10.0)
                .child(
                    row()
                        .key("app-brand")
                        .w_full()
                        .center_y()
                        .gap(10.0)
                        .child(
                            col()
                                .size(34.0, 34.0)
                                .center()
                                .fill(state.accent.color())
                                .radius(10.0)
                                .child(text("P").font(TextStyle::new(17.0, 700.0)).color(page)),
                        )
                        .child(
                            col()
                                .gap(1.0)
                                .child(
                                    text("PlicZip")
                                        .font(TextStyle::new(16.0, 650.0))
                                        .color(kit.c.text_base),
                                )
                                .child(
                                    text("АРХИВНАЯ СТУДИЯ · ФОРМАТ .PLZ")
                                        .font(kit.ty.small)
                                        .color(kit.c.text_muted),
                                ),
                        ),
                )
                .child(
                    col()
                        .key("main-header")
                        .w_full()
                        .gap(8.0)
                        .child(
                            col()
                                .gap(3.0)
                                .child(
                                    text("Библиотека архивов")
                                        .font(TextStyle::new(25.0, 650.0))
                                        .color(kit.c.text_base),
                                )
                                .child(
                                    text("Управляйте содержимым PlicZip в одном месте")
                                        .font(kit.ty.small)
                                        .color(kit.c.text_muted),
                                ),
                        )
                        .child(
                            col()
                                .gap(5.0)
                                .child(
                                    text("ЦВЕТОВОЙ АКЦЕНТ")
                                        .font(kit.ty.small)
                                        .color(kit.c.text_muted),
                                )
                                .child(accent_controls),
                        ),
                )
                .child(source_card)
                .child(metrics)
                .child(extract_card)
                .child(library_card)
                .child(footer),
        )
}

fn main() {
    let mut spec = AppSpec::new(
        "PlicZip · Предпросмотр Rune",
        (1180.0, 960.0),
        State::default,
        view,
    );
    spec.min_size = Some((700.0, 760.0));
    spec.fonts = GUI_FONTS;
    spec.families = ("Intro", "ui-monospace");
    rune::run(spec);
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
}
