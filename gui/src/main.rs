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
mod widgets;
mod windows;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--install-context-menu") {
        return finish_command(windows::install(), "Контекстное меню PlicZip установлено.");
    }
    if args.iter().any(|arg| arg == "--uninstall-context-menu") {
        return finish_command(windows::uninstall(), "Контекстное меню PlicZip удалено.");
    }
    // Эти флаги вызываются командами Windows Explorer. Обычный запуск с
    // одним путём архива по-прежнему открывает архив напрямую.
    let context_add = value_after(&args, "--context-add").map(PathBuf::from);
    let startup = if context_add.is_some() {
        None
    } else {
        value_after(&args, "--context-open")
            .or_else(|| {
                args.iter()
                    .find(|a| !a.starts_with('-'))
                    .map(String::as_str)
            })
            .map(PathBuf::from)
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PlicZip")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([860.0, 540.0])
            .with_icon(icon::icon_data()),
        ..Default::default()
    };

    eframe::run_native(
        "PlicZip",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup, context_add)))),
    )
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn finish_command(result: Result<(), String>, success: &str) -> eframe::Result {
    match result {
        Ok(()) => {
            println!("{success}");
            windows::refresh();
        }
        Err(err) => eprintln!("Ошибка: {err}"),
    }
    Ok(())
}
