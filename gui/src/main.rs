//! Графическая оболочка PlicZip.
//!
//! Отдельный бинарник рядом с CLI: тот же крейт `pliczip`, общая библиотека.
//! Архив можно открыть двойным кликом — путь приходит аргументом командной
//! строки.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod dialogs;
mod icon;
mod progress;
mod theme;
mod views;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // Путь архива из аргументов: `pliczip-gui backup.plz` — открыть сразу.
    let startup: Option<PathBuf> = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PlicZip")
            .with_inner_size([1080.0, 680.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(icon::icon_data()),
        ..Default::default()
    };

    eframe::run_native(
        "PlicZip",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}
