#![forbid(unsafe_code)]

//! Первый вертикальный срез Rune-интерфейса PlicZip.
//!
//! Здесь намеренно нет дублирования архивных операций: это preview-экран,
//! который проверяет таблицу, сортировку и multi-select Rune. Подключение
//! реального AppModel будет следующим этапом миграции.

use std::borrow::Cow;

use rune::{col, row, text, Align, AppSpec, Cx, El};
use rune_kit::{Cell, Kit, SortDir, Table, TableColumn, TableState};

struct ArchiveEntry {
    id: String,
    name: String,
    kind: EntryKind,
    size: u64,
    encrypted: bool,
}

enum EntryKind {
    File,
    Directory,
}

struct State {
    entries: Vec<ArchiveEntry>,
    table: TableState,
}

fn demo_state() -> State {
    State {
        entries: vec![
            ArchiveEntry {
                id: "1".to_owned(),
                name: "documents".to_owned(),
                kind: EntryKind::Directory,
                size: 0,
                encrypted: false,
            },
            ArchiveEntry {
                id: "2".to_owned(),
                name: "documents/readme.txt".to_owned(),
                kind: EntryKind::File,
                size: 12_480,
                encrypted: false,
            },
            ArchiveEntry {
                id: "3".to_owned(),
                name: "documents/report.pdf".to_owned(),
                kind: EntryKind::File,
                size: 8_589_934,
                encrypted: true,
            },
            ArchiveEntry {
                id: "4".to_owned(),
                name: "images/logo.svg".to_owned(),
                kind: EntryKind::File,
                size: 96_460,
                encrypted: false,
            },
        ],
        table: TableState::new().multi_select(true),
    }
}

fn sort_entries(state: &mut State, column: &str, direction: SortDir) {
    let compare = |left: &ArchiveEntry, right: &ArchiveEntry| match column {
        "name" => rune_kit::natural_cmp(&left.name, &right.name),
        "type" => kind_name(&left.kind).cmp(kind_name(&right.kind)),
        "size" => left.size.cmp(&right.size),
        "encrypted" => left.encrypted.cmp(&right.encrypted),
        _ => std::cmp::Ordering::Equal,
    };

    state
        .entries
        .sort_by(|left, right| direction.apply(compare(left, right)));
}

fn kind_name(kind: &EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Directory => "directory",
    }
}

fn kind_label(kind: &EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "Файл",
        EntryKind::Directory => "Каталог",
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
        |state: &State| state.entries.len(),
        |state: &State, index| Cow::Borrowed(state.entries[index].id.as_str()),
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
        let entry = &state.entries[index];
        match column {
            "name" => Cell::el(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(text(if matches!(entry.kind, EntryKind::Directory) {
                        "[dir]"
                    } else {
                        "[file]"
                    }))
                    .child(text(&entry.name)),
            ),
            "type" => Cell::text(kind_label(&entry.kind)).muted(),
            "size" => Cell::text(format_size(entry.size)).mono(),
            "encrypted" => {
                if entry.encrypted {
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

    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(kit.c.page)
        .padding(12.0)
        .gap(8.0)
        .child(
            text("PlicZip — Rune preview")
                .font(kit.ty.ui_medium)
                .color(kit.c.text_base),
        )
        .child(text("Таблица файлов архива").color(kit.c.text_muted))
        .child(
            col()
                .key("table-frame")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(archive_table(state, cx)),
        )
        .child(
            row().key("status").w_full().child(
                text(format!(
                    "{} объектов · {} выбрано",
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
        demo_state,
        view,
    ));
}
