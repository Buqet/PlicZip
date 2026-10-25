//! Модальные окна: создание, распаковка, пароль, ошибка, прогресс.

use eframe::egui::{self, vec2, Align, Align2, Color32, FontId, Layout, Sense, Stroke};

use crate::app::{App, CreateDlg, ExtractDlg, Modal, ModalKind, PwDlg};
use crate::theme::Palette;
use crate::views::{action_button, primary_button};
use crate::widgets::{self, mix, Icon, Kind as Btn};

/// Модальное окно: затемнение фона, карточка с иконкой и заголовком.
fn dialog(
    ctx: &egui::Context,
    title: impl Into<String>,
    icon: Icon,
    color: Color32,
    p: Palette,
    add: impl FnOnce(&mut egui::Ui),
) {
    let title = title.into();
    egui::Modal::new(egui::Id::unique(("pliczip_dialog", icon as u8)))
        .backdrop_color(Color32::from_black_alpha(if p.dark { 160 } else { 90 }))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .corner_radius(16u8)
                .stroke(Stroke::new(1.0, p.line))
                .inner_margin(egui::Margin::same(22))
                .shadow(egui::Shadow {
                    offset: [0, 14],
                    blur: 40,
                    spread: 0,
                    color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }),
                }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                let fill = mix(p.panel, color, 0.15);
                ui.painter().rect_filled(r, 10.0, fill);
                widgets::paint_icon(ui.painter(), r.shrink(9.0), icon, color, fill);
                ui.add_space(4.0);
                ui.label(egui::RichText::new(title).size(17.0).strong().color(p.text));
            });
            ui.add_space(14.0);
            add(ui);
        });
}

/// Рисует текущее модальное окно.
pub fn draw(app: &mut App, ctx: &egui::Context, p: Palette) {
    if let Some(op) = &app.op {
        progress_window(ctx, op, p);
        return;
    }
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    match app.modal_kind() {
        ModalKind::None => {}
        ModalKind::Create => {
            let mut request = CreateRequest::None;
            dialog(ctx, "Создать архив", Icon::Plus, p.accent, p, |ui| {
                    if let Modal::Create(dlg) = &mut app.modal {
                        request = create_dialog(ui, dlg, p);
                    }
                });
            if escape {
                request = CreateRequest::Cancel;
            }
            match request {
                CreateRequest::Create => {
                    let dlg = match std::mem::replace(&mut app.modal, Modal::None) {
                        Modal::Create(dlg) => dlg,
                        _ => return,
                    };
                    // При ошибке диалог возвращается как был: введённое не теряется.
                    if let Err(err) = app.spawn_create(&dlg) {
                        app.modal = Modal::Create(CreateDlg {
                            error: Some(err),
                            ..dlg
                        });
                    }
                }
                CreateRequest::Cancel => app.modal = Modal::None,
                CreateRequest::None => {}
            }
        }
        ModalKind::Extract => {
            let mut submit = false;
            let mut cancel = false;
            dialog(ctx, "Распаковать архив", Icon::Extract, p.accent, p, |ui| {
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
            if cancel || escape {
                app.modal = Modal::None;
            }
        }
        ModalKind::Password => {
            let mut submit = false;
            let mut cancel = false;
            dialog(ctx, "Пароль архива", Icon::Lock, p.accent, p, |ui| {
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
            if cancel || escape {
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
            dialog(ctx, title, Icon::Cross, p.err, p, |ui| {
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
            if close || escape {
                app.modal = Modal::None;
            }
        }
        ModalKind::About => {
            let mut close = false;
            dialog(ctx, "О программе", Icon::Archive, p.accent, p, |ui| {
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
            if close || escape {
                app.modal = Modal::None;
            }
        }
    }
}

/// Что диалог создания просит сделать дальше.
enum CreateRequest {
    None,
    Create,
    Cancel,
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

    ui.set_width(470.0);
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let zone = egui::Frame::new()
        .fill(mix(p.panel, p.accent, if hovering { 0.12 } else { 0.04 }))
        .corner_radius(12u8)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if dlg.sources.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    widgets::icon(ui, Icon::Folder, 28.0, p.accent);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Перетащите файлы или папки сюда").strong().color(p.text));
                    ui.label(egui::RichText::new("или выберите их на диске кнопкой ниже").size(12.0).color(p.muted));
                    ui.add_space(6.0);
                });
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
                            if widgets::button(ui, Some(Icon::Close), "", Btn::Ghost, true)
                                .on_hover_text("Убрать из списка")
                                .clicked()
                            {
                                dlg.sources.remove(i);
                            }
                        });
                    }
                });
            }
        });
    widgets::dashed_rect(
        ui.painter(),
        zone.response.rect,
        12.0,
        Stroke::new(1.2, mix(p.line, p.accent, if hovering { 1.0 } else { 0.35 })),
    );

    ui.add_space(10.0);
    if widgets::button(ui, Some(Icon::Plus), "Выбрать на диске…", Btn::Normal, true).clicked() {
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
                .desired_width(120.0),
        );
        ui.label(egui::RichText::new("— пусто: один файл").size(11.5).color(p.dim));
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Комментарий").color(p.muted));
    ui.add(egui::TextEdit::singleline(&mut dlg.comment).desired_width(f32::INFINITY));

    if let Some(err) = &dlg.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if action_button(ui, "Отмена").clicked() {
            request = CreateRequest::Cancel;
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
        if action_button(ui, "Отмена").clicked() {
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
        if action_button(ui, "Отмена").clicked() {
            *cancel = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, "Открыть", p).clicked() {
                *submit = true;
            }
        });
    });
}

/// Окно прогресса: кольцо с процентом, скорость и честный остаток.
fn progress_window(ctx: &egui::Context, op: &crate::progress::OpState, p: Palette) {
    let mut chars = op.label.chars();
    let title = match chars.next() {
        Some(first) => format!("{}{}…", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    };
    dialog(ctx, title, Icon::Archive, p.accent, p, |ui| {
        ui.set_width(380.0);
        let percent = op.percent();
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(150.0, 150.0), Sense::hover());
            let painter = ui.painter();
            let frac = (percent / 100.0) as f32;
            widgets::ring(painter, rect.center(), 62.0, 10.0, frac, mix(p.panel, p.text, 0.08), p.accent);
            // Бегущая точка, пока объём ещё неизвестен.
            if op.total == 0 {
                let t = ui.input(|i| i.time) as f32;
                let a = t * 3.0 - std::f32::consts::FRAC_PI_2;
                let dot = rect.center() + vec2(a.cos(), a.sin()) * 62.0;
                painter.circle_filled(dot, 6.0, p.accent);
            }
            painter.text(
                rect.center() - vec2(0.0, 4.0),
                Align2::CENTER_CENTER,
                format!("{percent:.0}%"),
                FontId::proportional(34.0),
                p.text,
            );
            let done = pliczip::util::human_bytes(op.done);
            let total = if op.total > 0 { pliczip::util::human_bytes(op.total) } else { "?".into() };
            painter.text(
                rect.center() + vec2(0.0, 26.0),
                Align2::CENTER_CENTER,
                format!("{done} / {total}"),
                FontId::monospace(11.0),
                p.dim,
            );
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let speed = match op.speed() {
                Some(v) => format!("{:.1} МиБ/с", v / (1024.0 * 1024.0)),
                None => "— МиБ/с".to_string(),
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
        ui.label(
            egui::RichText::new(format!(
                "прошло {}",
                pliczip::util::human_duration(op.started.elapsed())
            ))
            .size(11.5)
            .color(p.dim)
            .monospace(),
        );
    });
}
