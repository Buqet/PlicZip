//! Модальные окна: создание, распаковка, пароль, ошибка, прогресс.

use eframe::egui::{self, vec2, Align, Color32, Layout, Sense, Stroke};

use crate::app::{App, CreateDlg, ExtractDlg, Modal, ModalKind, PwDlg};
use crate::config::{Accent, Language};
use crate::locale::Locale;
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
    if app.op.is_some() {
        return;
    }
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    match app.modal_kind() {
        ModalKind::None => {}
        ModalKind::Create => {
            let mut request = CreateRequest::None;
            dialog(
                ctx,
                app.locale.text("dialog-create"),
                Icon::Plus,
                p.accent,
                p,
                |ui| {
                    if let Modal::Create(dlg) = &mut app.modal {
                        request = create_dialog(ui, dlg, p, &app.locale);
                    }
                },
            );
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
            dialog(
                ctx,
                app.locale.text("dialog-extract"),
                Icon::Extract,
                p.accent,
                p,
                |ui| {
                    if let Modal::Extract(dlg) = &mut app.modal {
                        extract_dialog(ui, dlg, p, &app.locale, &mut submit, &mut cancel);
                    }
                },
            );
            if submit {
                let output = match &mut app.modal {
                    Modal::Extract(dlg) => std::path::PathBuf::from(dlg.output.trim().to_string()),
                    _ => return,
                };
                if output.as_os_str().is_empty() {
                    if let Modal::Extract(dlg) = &mut app.modal {
                        dlg.error = Some(app.locale.text("choose-folder-error"));
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
            dialog(
                ctx,
                app.locale.text("dialog-password"),
                Icon::Lock,
                p.accent,
                p,
                |ui| {
                    if let Modal::Password(dlg) = &mut app.modal {
                        password_dialog(ui, dlg, p, &app.locale, &mut submit, &mut cancel);
                    }
                },
            );
            if submit {
                let (path, password) = match &mut app.modal {
                    Modal::Password(dlg) => (dlg.path.clone(), dlg.password.clone()),
                    _ => return,
                };
                if password.is_empty() {
                    if let Modal::Password(dlg) = &mut app.modal {
                        dlg.error = Some(app.locale.text("password-required"));
                    }
                } else {
                    app.open_now(path, Some(zeroize::Zeroizing::new(password)));
                }
            }
            if cancel || escape {
                app.modal = Modal::None;
            }
        }
        ModalKind::Rename => {
            let mut submit = false;
            let mut cancel = false;
            dialog(
                ctx,
                app.locale.text("rename-title"),
                Icon::File,
                p.accent,
                p,
                |ui| {
                    if let Modal::Rename { value, .. } = &mut app.modal {
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .hint_text(app.locale.text("name-placeholder")),
                        );
                        ui.horizontal(|ui| {
                            submit =
                                primary_button(ui, app.locale.text("apply-action"), p).clicked();
                            cancel = action_button(ui, app.locale.text("cancel-action")).clicked();
                        });
                    }
                },
            );
            if escape {
                cancel = true;
            }
            if submit {
                if let Modal::Rename { index, value } =
                    std::mem::replace(&mut app.modal, Modal::None)
                {
                    if !value.trim().is_empty() {
                        app.spawn_rebuild_entry(Some(index), Some(value.trim().to_owned()), None);
                    }
                }
            } else if cancel {
                app.modal = Modal::None;
            }
        }
        ModalKind::NewFile => {
            let mut submit = false;
            let mut cancel = false;
            dialog(
                ctx,
                app.locale.text("new-file-title"),
                Icon::File,
                p.accent,
                p,
                |ui| {
                    if let Modal::NewFile { value } = &mut app.modal {
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .hint_text(app.locale.text("name-placeholder")),
                        );
                        ui.horizontal(|ui| {
                            submit =
                                primary_button(ui, app.locale.text("apply-action"), p).clicked();
                            cancel = action_button(ui, app.locale.text("cancel-action")).clicked();
                        });
                    }
                },
            );
            if escape {
                cancel = true;
            }
            if submit {
                if let Modal::NewFile { value } = std::mem::replace(&mut app.modal, Modal::None) {
                    if !value.trim().is_empty() {
                        app.spawn_rebuild_entry(None, None, Some(value.trim().to_owned()));
                    }
                }
            } else if cancel {
                app.modal = Modal::None;
            }
        }
        ModalKind::Recovery => {
            let mut close = false;
            let mut repair = false;
            dialog(
                ctx,
                app.locale.text("recovery-title"),
                Icon::Shield,
                p.accent,
                p,
                |ui| {
                    ui.set_min_width(470.0);
                    ui.label(
                        egui::RichText::new(app.locale.text("recovery-step-diagnosis"))
                            .strong()
                            .color(p.text),
                    );
                    ui.label(
                        egui::RichText::new(app.locale.text("recovery-password-note"))
                            .color(p.muted),
                    );
                    ui.add_space(10.0);
                    let report = match &app.modal {
                        Modal::Recovery(dlg) => dlg.report.as_ref(),
                        _ => None,
                    };
                    match report {
                        None => {
                            ui.spinner();
                            ui.label(app.locale.text("recovery-checking"));
                        }
                        Some(Err(err)) => {
                            ui.colored_label(p.err, format!("Диагностика не удалась: {err}"));
                        }
                        Some(Ok(report)) => {
                            let status = if report.is_clean() {
                                "Повреждений не обнаружено. Восстановление не требуется."
                            } else if report.recoverable() {
                                "Повреждения обнаружены, но их можно восстановить."
                            } else {
                                "Повреждения обнаружены, но parity недостаточно."
                            };
                            ui.colored_label(
                                if report.is_clean() { p.ok } else { p.accent },
                                status,
                            );
                            ui.label(format!(
                                "Data-кусков: {} · Parity-кусков: {}",
                                report.data_shards, report.parity_shards
                            ));
                            ui.label(format!(
                                "Повреждено data: {} · parity: {}",
                                report.bad_data.len(),
                                report.bad_parity.len()
                            ));
                            if !report.unrecoverable_groups.is_empty() {
                                ui.colored_label(p.err, app.locale.text("recovery-groups-error"));
                            }
                            ui.add_space(8.0);
                            if report.recoverable() && !report.is_clean() {
                                ui.label(
                                    egui::RichText::new(app.locale.text("recovery-step-repair"))
                                        .strong()
                                        .color(p.text),
                                );
                                ui.label(app.locale.text("recovery-recheck"));
                                repair =
                                    primary_button(ui, app.locale.text("recover"), p).clicked();
                            }
                        }
                    }
                    ui.add_space(12.0);
                    if action_button(ui, app.locale.text("close-dialog")).clicked() {
                        close = true;
                    }
                },
            );
            if repair {
                app.modal = Modal::None;
                app.spawn_repair();
            } else if close || escape {
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
                if primary_button(ui, app.locale.text("close"), p).clicked() {
                    close = true;
                }
            });
            if close || escape {
                app.modal = Modal::None;
            }
        }
        ModalKind::About => {
            let mut close = false;
            dialog(
                ctx,
                &app.locale.menu_about,
                Icon::Archive,
                p.accent,
                p,
                |ui| {
                    ui.set_max_width(420.0);
                    ui.label(egui::RichText::new("PlicZip").size(24.0).color(p.accent));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(app.locale.text("archive-motto")).color(p.text));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "версия {}\nформат PLZ1 · BLAKE3 · ChaCha20-Poly1305",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .color(p.muted)
                        .monospace(),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(&app.locale.about_changes)
                            .strong()
                            .color(p.text),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(190.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for line in CHANGELOG_RU.lines() {
                                ui.label(egui::RichText::new(line).size(12.0).color(p.muted));
                            }
                        });
                    ui.add_space(10.0);
                    if primary_button(ui, &app.locale.about_close, p).clicked() {
                        close = true;
                    }
                },
            );
            if close || escape {
                app.modal = Modal::None;
            }
        }
        ModalKind::Settings => {
            let mut close = false;
            let mut selected_language = app.cfg.language;
            let mut selected_accent = app.cfg.accent;
            dialog(
                ctx,
                &app.locale.settings_title,
                Icon::Archive,
                p.accent,
                p,
                |ui| {
                    ui.set_min_width(420.0);
                    ui.horizontal(|ui| {
                        let _ = ui.selectable_label(false, &app.locale.settings_general);
                        let _ = ui.selectable_label(true, &app.locale.settings_language);
                    });
                    ui.separator();
                    ui.label(
                        egui::RichText::new(&app.locale.settings_language)
                            .strong()
                            .color(p.text),
                    );
                    egui::Frame::new()
                        .fill(p.panel)
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            for language in Language::ALL {
                                let (name, selected) = match language {
                                    Language::Russian => (
                                        &app.locale.language_russian,
                                        selected_language == language,
                                    ),
                                    Language::English => (
                                        &app.locale.language_english,
                                        selected_language == language,
                                    ),
                                };
                                let response = ui.selectable_label(
                                    selected,
                                    egui::RichText::new(format!(
                                        "{}  {}                                      {}%",
                                        language.flag(),
                                        name,
                                        language.completion()
                                    ))
                                    .size(13.0),
                                );
                                if response.clicked() {
                                    selected_language = language;
                                }
                            }
                        });
                    ui.label(
                        egui::RichText::new(app.locale.text("localization-format"))
                            .size(11.0)
                            .color(p.dim),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(app.locale.text("accent"))
                            .strong()
                            .color(p.text),
                    );
                    egui::Frame::new()
                        .fill(p.panel)
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            for accent in Accent::ALL {
                                let key = match accent {
                                    Accent::Brass => "accent-brass",
                                    Accent::Steel => "accent-steel",
                                    Accent::Green => "accent-green",
                                };
                                if ui
                                    .selectable_label(
                                        selected_accent == accent,
                                        app.locale.text(key),
                                    )
                                    .clicked()
                                {
                                    selected_accent = accent;
                                }
                            }
                        });
                    ui.add_space(14.0);
                    if primary_button(ui, &app.locale.about_close, p).clicked() {
                        close = true;
                    }
                },
            );
            if selected_language != app.cfg.language {
                app.set_language(selected_language);
            }
            if selected_accent != app.cfg.accent {
                app.cfg.accent = selected_accent;
                app.cfg.save();
                crate::theme::apply(ctx, &app.cfg);
            }
            if close || escape {
                app.modal = Modal::None;
            }
        }
    }
}

const CHANGELOG_RU: &str = "• TAR: чтение, просмотр, проверка и распаковка\n• Контекстное меню файлов: переименование, удаление и выбор\n• Создание пустых файлов через ПКМ по свободному месту\n• Настройка акцента интерфейса\n• ZIP, 7z и RAR: режим чтения\n• Создание архивов только в формате .plz\n• Recovery records и Windows Explorer";

/// Что диалог создания просит сделать дальше.
enum CreateRequest {
    None,
    Create,
    Cancel,
}

/// Диалог создания архива.
fn create_dialog(
    ui: &mut egui::Ui,
    dlg: &mut CreateDlg,
    p: Palette,
    locale: &Locale,
) -> CreateRequest {
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
                    ui.label(
                        egui::RichText::new(locale.text("drop-files"))
                            .strong()
                            .color(p.text),
                    );
                    ui.label(
                        egui::RichText::new(locale.text("choose-files-hint"))
                            .size(12.0)
                            .color(p.muted),
                    );
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
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for i in (0..dlg.sources.len()).rev() {
                            let path = dlg.sources[i].clone();
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(path.display().to_string()).color(p.text),
                                );
                                if widgets::button(ui, Some(Icon::Close), "", Btn::Ghost, true)
                                    .on_hover_text(locale.text("remove-from-list"))
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
        Stroke::new(
            1.2,
            mix(p.line, p.accent, if hovering { 1.0 } else { 0.35 }),
        ),
    );

    ui.add_space(10.0);
    if widgets::button(
        ui,
        Some(Icon::Plus),
        &locale.text("choose-disk"),
        Btn::Normal,
        true,
    )
    .clicked()
    {
        if let Some(picked) = rfd::FileDialog::new().pick_files() {
            for path in picked {
                if !dlg.sources.contains(&path) {
                    dlg.sources.push(path);
                }
            }
        }
    }

    ui.add_space(10.0);
    ui.label(egui::RichText::new(locale.text("save-as")).color(p.muted));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut dlg.output);
        if action_button(ui, locale.text("browse")).clicked() {
            if let Some(picked) = rfd::FileDialog::new()
                .set_file_name(locale.text("archive-default-file"))
                .set_title(locale.text("save-archive"))
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
        ui.label(egui::RichText::new(locale.text("compression")).color(p.muted));
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
    ui.checkbox(&mut dlg.density, locale.text("density"));
    if dlg.density {
        ui.label(
            egui::RichText::new(locale.text("density-help"))
                .size(12.0)
                .color(p.muted),
        );
    }

    ui.add_space(8.0);
    ui.checkbox(&mut dlg.solid, locale.text("solid"));

    ui.add_space(8.0);
    ui.checkbox(&mut dlg.encrypt, locale.text("encrypt"));
    if dlg.encrypt {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(locale.text("password")).color(p.muted));
            ui.add(
                egui::TextEdit::singleline(&mut dlg.password)
                    .password(true)
                    .desired_width(180.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(locale.text("password-repeat")).color(p.muted));
            ui.add(
                egui::TextEdit::singleline(&mut dlg.password2)
                    .password(true)
                    .desired_width(180.0),
            );
        });
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(locale.text("volume")).color(p.muted));
        ui.add(
            egui::TextEdit::singleline(&mut dlg.volume)
                .hint_text(locale.text("volume-example"))
                .desired_width(120.0),
        );
        ui.label(
            egui::RichText::new(locale.text("volume-empty"))
                .size(11.5)
                .color(p.dim),
        );
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new(locale.text("comment")).color(p.muted));
    ui.add(egui::TextEdit::singleline(&mut dlg.comment).desired_width(f32::INFINITY));

    if let Some(err) = &dlg.error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if action_button(ui, locale.text("cancel-action")).clicked() {
            request = CreateRequest::Cancel;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, locale.text("create-button"), p).clicked() {
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
    locale: &Locale,
    submit: &mut bool,
    cancel: &mut bool,
) {
    ui.set_min_width(420.0);
    ui.label(egui::RichText::new(locale.text("extract-to")).color(p.muted));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut dlg.output);
        if action_button(ui, locale.text("browse")).clicked() {
            if let Some(picked) = rfd::FileDialog::new()
                .set_title(locale.text("extract-folder"))
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
        if action_button(ui, locale.text("cancel-action")).clicked() {
            *cancel = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, locale.text("extract-button"), p).clicked() {
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
    locale: &Locale,
    submit: &mut bool,
    cancel: &mut bool,
) {
    ui.set_min_width(360.0);
    ui.label(egui::RichText::new(format!("Архив {}", dlg.path.display())).color(p.muted));
    ui.add_space(8.0);
    ui.add(
        egui::TextEdit::singleline(&mut dlg.password)
            .password(true)
            .hint_text(locale.text("password"))
            .desired_width(340.0),
    );
    if let Some(err) = &dlg.error {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(err).color(p.err));
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if action_button(ui, locale.text("cancel-action")).clicked() {
            *cancel = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if primary_button(ui, locale.text("open-button"), p).clicked() {
                *submit = true;
            }
        });
    });
}
