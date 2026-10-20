//! Модальные окна: создание, распаковка, пароль, ошибка, прогресс.

use eframe::egui::{self, Align, Layout, Stroke};

use crate::app::{App, CreateDlg, ExtractDlg, Modal, ModalKind, PwDlg};
use crate::theme::Palette;
use crate::views::{action_button, primary_button};

/// Рисует текущее модальное окно.
pub fn draw(app: &mut App, ctx: &egui::Context, p: Palette) {
    if let Some(op) = &app.op {
        progress_window(ctx, op, p);
    }

    match app.modal_kind() {
        ModalKind::None => {}
        ModalKind::Create => {
            let mut request = CreateRequest::None;
            egui::Window::new("Создать архив")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Modal::Create(dlg) = &mut app.modal {
                        request = create_dialog(ui, dlg, p);
                    }
                });
            if let CreateRequest::Create = request {
                let dlg = match std::mem::replace(&mut app.modal, Modal::None) {
                    Modal::Create(dlg) => dlg,
                    _ => return,
                };
                if let Err(err) = app.spawn_create(dlg) {
                    app.modal = Modal::Create(CreateDlg {
                        error: Some(err),
                        ..CreateDlg::from_sources(Vec::new())
                    });
                }
            }
        }
        ModalKind::Extract => {
            let mut submit = false;
            let mut cancel = false;
            egui::Window::new("Распаковать архив")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Modal::Extract(dlg) = &mut app.modal {
                        extract_dialog(ui, dlg, p, &mut submit, &mut cancel);
                    }
                });
            if submit {
                let output = match &mut app.modal {
                    Modal::Extract(dlg) => std::path::PathBuf::from(dlg.output.trim().to_string()),
                    _ => return,
                };
                if output.as_os_str().is_empty() {
                    if let Modal::Extract(dlg) = &mut app.modal {
                        dlg.error = Some("укажи каталог для распаковки".into());
                    }
                } else {
                    app.spawn_extract(output);
                }
            }
            if cancel {
                app.modal = Modal::None;
            }
        }
        ModalKind::Password => {
            let mut submit = false;
            let mut cancel = false;
            egui::Window::new("Пароль архива")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Modal::Password(dlg) = &mut app.modal {
                        password_dialog(ui, dlg, p, &mut submit, &mut cancel);
                    }
                });
            if submit {
                let (path, password) = match &mut app.modal {
                    Modal::Password(dlg) => (dlg.path.clone(), dlg.password.clone()),
                    _ => return,
                };
                if password.is_empty() {
                    if let Modal::Password(dlg) = &mut app.modal {
                        dlg.error = Some("пароль не может быть пустым".into());
                    }
                } else {
                    app.open_now(path, Some(zeroize::Zeroizing::new(password)));
                }
            }
            if cancel {
                app.modal = Modal::None;
            }
        }
        ModalKind::Error => {
            let (title, message, hint) = match &app.modal {
                Modal::Error {
                    title,
                    message,
                    hint,
                } => (title.clone(), message.clone(), hint.clone()),
                _ => return,
            };
            let mut close = false;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(egui::RichText::new(&message).color(p.text));
                    if !hint.is_empty() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(&hint).color(p.muted));
                    }
                    ui.add_space(12.0);
                    if primary_button(ui, "Закрыть", p).clicked() {
                        close = true;
                    }
                });
            if close {
                app.modal = Modal::None;
            }
        }
        ModalKind::About => {
            let mut close = false;
            egui::Window::new("О программе")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(egui::RichText::new("PlicZip").size(24.0).color(p.accent));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Архиватор, который не врёт о целостности.")
                            .color(p.text),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "версия {}\nформат PLZ1 · BLAKE3 · ChaCha20-Poly1305",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .color(p.muted)
                        .monospace(),
                    );
                    ui.add_space(14.0);
                    if primary_button(ui, "Закрыть", p).clicked() {
                        close = true;
                    }
                });
            if close {
                app.modal = Modal::None;
            }
        }
    }
}

/// Что диалог создания просит сделать дальше.
enum CreateRequest {
    None,
    Create,
}

/// Диалог создания архива.
fn create_dialog(ui: &mut egui::Ui, dlg: &mut CreateDlg, p: Palette) -> CreateRequest {
    let mut request = CreateRequest::None;

    // Перетаскивание файлов в диалог.
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    for path in dropped {
        if !dlg.sources.contains(&path) {
            dlg.sources.push(path);
        }
    }

    egui::Frame::new()
        .fill(p.panel2)
        .corner_radius(8u8)
        .stroke(Stroke::new(1.0, p.line))
        .show(ui, |ui| {
            ui.set_min_width(440.0);
            if dlg.sources.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Перетащите файлы или папки").strong().color(p.text));
                ui.label(egui::RichText::new("или выберите на диске").color(p.accent));
                ui.add_space(10.0);
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "В архив: {}",
                        crate::app::plural_ru(dlg.sources.len(), "файл", "файла", "файлов")
                    ))
                    .color(p.muted),
                );
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for i in (0..dlg.sources.len()).rev() {
                        let path = dlg.sources[i].clone();
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(path.display().to_string()).color(p.text));
                            if ui.small_button("✕").clicked() {
                                dlg.sources.remove(i);
                            }
                        });
                    }
                });
            }
        });

    ui.add_space(10.0);
    if action_button(ui, "Выбрать на диске…").clicked() {
        if let Some(picked) = rfd::FileDialog::new().pick_files() {
            for path in picked {
                if !dlg.sources.contains(&path) {
                    dlg.sources.push(path);
                }
            }
        }
    }

    ui.add_space(10.0);
    ui.label(egui::RichText::new("Сохранить как").color(p.muted));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut dlg.output);
        if action_button(ui, "Обзор…").clicked() {
            if let Some(picked) = rfd::FileDialog::new()
                .set_file_name("архив.plz")
                .set_title("Сохранить архив")
                .save_file()
            {
                dlg.output = picked.to_string_lossy().into_owned();
            }
        }
    });

    ui.add_space(12.0);
    // Ползунок подписан следствием, а не уровнем: пользователь не обязан
    // знать, что такое q9.
    let label = match dlg.level {
        0 => "без сжатия".to_string(),
        1..=4 => format!("deflate · {}", dlg.level),
        5 => "brotli q5 · баланс".to_string(),
        6..=8 => format!("brotli q{} · сильнее", dlg.level),
        9 => "brotli q9 · максимум для текста".to_string(),
        _ => format!("brotli q{} · максимум", dlg.level),
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Сжатие").color(p.muted));
        ui.add(egui::Slider::new(&mut dlg.level, 0..=11).show_value(false));
        ui.label(egui::RichText::new(&label).color(p.accent).monospace());
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("быстро").size(11.5).color(p.dim));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(egui::RichText::new("максимум").size(11.5).color(p.dim));
        });
    });
    ui.label(
        egui::RichText::new(match dlg.level {
            0 => "уже сжатые файлы: видео, jpg, другие архивы",
            1..=4 => "быстро и умеренно: обычные файлы",
            5 => "разумный баланс — по умолчанию",
            6..=8 => "дольше, но заметно меньше: текст и код",
            9 => "долго, но текст и код жмутся сильно",
            _ => "максимум качества, очень долго",
        })
        .size(12.0)
        .color(p.muted),
    );

    ui.add_space(8.0);
    ui.checkbox(&mut dlg.solid, "Склеивать мелкие файлы в solid-блоки");

    ui.add_space(8.0);
    ui.checkbox(&mut dlg.encrypt, "Защитить паролем");
    if dlg.encrypt {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Пароль").color(p.muted));
            ui.add(
                egui::TextEdit::singleline(&mut dlg.password)
                    .password(true)
                    .desired_width(180.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Повторите").color(p.muted));
            ui.add(
                egui::TextEdit::singleline(&mut dlg.password2)
                    .password(true)
                    .desired_width(180.0),
            );
        });
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Том").color(p.muted));
        ui.add(
            egui::TextEdit::singleline(&mut dlg.volume)
                .hint_text("например 700M")
                .desired_width(90.0),
        );
        ui.label(egui::RichText::new("— пусто: один файл").size(11.5).color(p.dim));
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Комментарий").color(p.muted));
    ui.add(egui::TextEdit::singleline(&mut dlg.comment).desired_width(420.0));

    if let Some(err) = &dlg.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui.button("Отмена").clicked() {
            *dlg = CreateDlg::from_sources(Vec::new());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, "Создать", p).clicked() {
                request = CreateRequest::Create;
            }
        });
    });

    request
}

/// Диалог распаковки.
fn extract_dialog(
    ui: &mut egui::Ui,
    dlg: &mut ExtractDlg,
    p: Palette,
    submit: &mut bool,
    cancel: &mut bool,
) {
    ui.set_min_width(420.0);
    ui.label(egui::RichText::new("Куда распаковать").color(p.muted));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut dlg.output);
        if action_button(ui, "Обзор…").clicked() {
            if let Some(picked) = rfd::FileDialog::new()
                .set_title("Каталог для распаковки")
                .pick_folder()
            {
                dlg.output = picked.to_string_lossy().into_owned();
            }
        }
    });
    if let Some(err) = &dlg.error {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui.button("Отмена").clicked() {
            *cancel = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, "Распаковать", p).clicked() {
                *submit = true;
            }
        });
    });
}

/// Диалог ввода пароля.
fn password_dialog(
    ui: &mut egui::Ui,
    dlg: &mut PwDlg,
    p: Palette,
    submit: &mut bool,
    cancel: &mut bool,
) {
    ui.set_min_width(360.0);
    ui.label(
        egui::RichText::new(format!("Архив {}", dlg.path.display())).color(p.muted),
    );
    ui.add_space(8.0);
    ui.add(
        egui::TextEdit::singleline(&mut dlg.password)
            .password(true)
            .hint_text("Пароль")
            .desired_width(340.0),
    );
    if let Some(err) = &dlg.error {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui.button("Отмена").clicked() {
            *cancel = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, "Открыть", p).clicked() {
                *submit = true;
            }
        });
    });
}

/// Окно прогресса: процент, полоса, скорость и честный остаток.
fn progress_window(ctx: &egui::Context, op: &crate::progress::OpState, p: Palette) {
    egui::Window::new(format!("{}…", op.label))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            let percent = op.percent();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{percent:.0}"))
                        .size(42.0)
                        .color(p.accent)
                        .monospace(),
                );
                ui.label(egui::RichText::new("%").size(18.0).color(p.dim));
            });

            ui.add_space(6.0);
            ui.add(
                egui::ProgressBar::new((percent / 100.0) as f32)
                    .fill(p.accent)
                    .desired_height(8.0),
            );

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let speed = match op.speed() {
                    Some(v) => format!("{:.0} МиБ/с", v / (1024.0 * 1024.0)),
                    None => "—".to_string(),
                };
                ui.label(egui::RichText::new(speed).color(p.muted).monospace());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let eta = match op.eta() {
                        Some(d) => format!("осталось {}", pliczip::util::human_duration(d)),
                        None => "осталось —".to_string(),
                    };
                    ui.label(egui::RichText::new(eta).color(p.muted).monospace());
                });
            });

            ui.add_space(4.0);
            let done = pliczip::util::human_bytes(op.done);
            let total = pliczip::util::human_bytes(op.total);
            ui.label(
                egui::RichText::new(format!(
                    "{done} / {total} · прошло {}",
                    pliczip::util::human_duration(op.started.elapsed())
                ))
                .size(11.5)
                .color(p.dim)
                .monospace(),
            );
        });
}
