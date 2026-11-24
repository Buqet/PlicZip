#![forbid(unsafe_code)]

//! Первый рабочий вертикальный срез Rune-интерфейса PlicZip.
//!
//! Таблица отображает реальные записи native `.plz` архива. Создание и извлечение
//! пока остаются в основной egui-оболочке и будут перенесены отдельными этапами.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use rune::{
    col, row, text, text_input, Align, AppSpec, Cx, El, FileDialogOptions, TextEdit, TextStyle, Tx,
};
use rune_kit::{Button, ButtonVariant, Cell, Kit, SortDir, Table, TableColumn, TableState};

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

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024 * 1024) as f64)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes == 0 {
        "—".to_owned()
    } else {
        format!("{bytes} B")
    }
}

fn archive_table(state: &State, cx: &mut Cx<State>) -> El {
    let kit = Kit::neutral(cx.system_dark());
    let warning = kit.c.warning.fg;

    let table = Table::new(
        |state: &State| &state.table,
        |state: &mut State| &mut state.table,
        |state: &State| state.visible.len(),
        |state: &State, index| Cow::Borrowed(state.entries[state.visible[index]].id.as_str()),
    )
    .name("Archive files")
    .column(
        TableColumn::new("name", "Имя")
            .flex(2.0)
            .min(220.0)
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
        TableColumn::new("encrypted", "Защита")
            .width(100.0)
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
                    Cell::text("Да").color(warning)
                } else {
                    Cell::text("Нет").muted()
                }
            }
            _ => Cell::text(""),
        }
    })
    .on_sort(|state: &mut State, column, direction, _| {
        sort_entries(state, column, direction);
    })
    .zebra(true)
    .build(state, &kit)
    .grow(1.0)
    .min_h(0.0);

    col().key("archive-table").w_full().h_full().child(table)
}

fn view(state: &State, cx: &mut Cx<State>) -> El {
    let kit = Kit::neutral(cx.system_dark());
    let selected = state.table.selected_len();
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
    let path_field = text_input(&state.path_input, TextStyle::new(13.0, 400.0))
        .key("archive-path")
        .size(350.0, 32.0)
        .single_line()
        .placeholder("Путь к архиву .plz", kit.c.text_muted)
        .on_edit(|state: &mut State| &mut state.path_input)
        .on_submit(|state: &mut State, _| state.open_path_input());

    let extract_row = if state.archive_path.is_some() && !state.archive_encrypted {
        row()
            .key("extract-row")
            .w_full()
            .center_y()
            .gap(8.0)
            .child(text("Распаковать в").color(kit.c.text_muted))
            .child(
                text_input(&state.output_input, TextStyle::new(13.0, 400.0))
                    .key("output-path")
                    .size(440.0, 30.0)
                    .single_line()
                    .placeholder("Папка назначения", kit.c.text_muted)
                    .on_edit(|state: &mut State| &mut state.output_input)
                    .on_submit(|state: &mut State, _| state.extract_all()),
            )
            .child(
                Button::new("Извлечь всё")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|state: &mut State| state.extract_all())
                    .build(&kit),
            )
    } else {
        text("")
    };

    let filter_row = if state.archive_path.is_some() {
        row()
            .key("filter-row")
            .w_full()
            .center_y()
            .gap(8.0)
            .child(text("Фильтр").color(kit.c.text_muted))
            .child(
                text_input(&state.filter_input, TextStyle::new(13.0, 400.0))
                    .key("archive-filter")
                    .w_full()
                    .size(400.0, 30.0)
                    .single_line()
                    .placeholder("Имя или часть пути", kit.c.text_muted)
                    .on_edit(|state: &mut State| &mut state.filter_input)
                    .on_input_change(|state: &mut State, _| state.refresh_visible())
                    .on_submit(|state: &mut State, _| state.refresh_visible()),
            )
    } else {
        text("")
    };

    let table_area = if state.archive_path.is_some() && !state.visible.is_empty() {
        archive_table(state, cx)
    } else {
        let hint = if state.archive_path.is_some() {
            if state.entries.is_empty() {
                "Архив не содержит записей"
            } else {
                "По запросу ничего не найдено"
            }
        } else {
            "Содержимое архива появится здесь"
        };
        col()
            .key("empty-state")
            .w_full()
            .grow(1.0)
            .center_y()
            .child(text(hint))
    };

    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(kit.c.page)
        .padding(12.0)
        .gap(8.0)
        .child(
            row()
                .key("toolbar")
                .w_full()
                .center_y()
                .gap(8.0)
                .child(open_button)
                .child(path_field)
                .child(load_button)
                .child(
                    text(state.archive_path.as_ref().map_or(
                        "PlicZip Rune preview".to_owned(),
                        |p| {
                            p.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| p.display().to_string())
                        },
                    ))
                    .font(kit.ty.ui_medium)
                    .color(kit.c.text_base),
                ),
        )
        .child(text(&state.status).color(kit.c.text_muted))
        .child(extract_row)
        .child(filter_row)
        .child(
            col()
                .key("table-frame")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(table_area),
        )
        .child(
            row().key("status").w_full().child(
                text(format!(
                    "{} из {} объектов · {} выбрано",
                    state.visible.len(),
                    state.entries.len(),
                    selected
                ))
                .font(kit.ty.text_small)
                .color(kit.c.text_muted),
            ),
        )
}

fn main() {
    rune::run(AppSpec::new(
        "PlicZip Rune Preview",
        (900.0, 600.0),
        State::default,
        view,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliczip::prelude::CreateOptions;
    use std::time::{SystemTime, UNIX_EPOCH};

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
