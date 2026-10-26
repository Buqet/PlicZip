//! Графическая оболочка PlicZip.
//!
//! Отдельный бинарник рядом с CLI: тот же крейт `pliczip`, общая библиотека.
//! Архив можно открыть двойным кликом — путь приходит аргументом командной
//! строки; пункты контекстного меню Проводника передают ключ действия
//! (`--extract-here`, `--add` и т. д., см. `shell::args`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod dialogs;
mod icon;
// Кодировщик ICO и значок архива из `mark` использует только build.rs.
#[allow(dead_code)]
mod mark;
mod progress;
mod shell;
mod theme;
mod views;
mod widgets;

use shell::Launch;

fn main() -> eframe::Result {
    // `pliczip-gui backup.plz` — открыть сразу; ключи — действия из Проводника.
    let launch = match shell::args::parse(std::env::args_os().skip(1)) {
        // Для установщика: встроить/убрать и выйти без окна. Код возврата 1 — ошибка.
        Launch::Register => exit_with(shell::register()),
        Launch::Unregister => exit_with(shell::unregister()),
        // Проводник запускает по процессу на каждый выделенный элемент —
        // собираем их в одно окно, лишние процессы выходят.
        Launch::Add(paths) => match shell::gather::collect("add", paths) {
            Some(all) => Launch::Add(all),
            None => return Ok(()),
        },
        Launch::Compress(paths) => match shell::gather::collect("compress", paths) {
            Some(all) => Launch::Compress(all),
            None => return Ok(()),
        },
        other => other,
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
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, launch)))),
    )
}

fn exit_with(result: Result<(), String>) -> ! {
    match result {
        Ok(()) => std::process::exit(0),
        Err(err) => {
            eprintln!("pliczip-gui: {err}");
            std::process::exit(1)
        }
    }
}
