//! Отрисовка главного окна: меню, тулбар, таблица, сводка, статусбар.

use eframe::egui::{self, Align, Color32, Layout, Sense, Stroke};
use pliczip::prelude::*;

use crate::app::{plural_ru, App, Integrity, Modal};
use crate::config::Accent;
use crate::theme::Palette;

/// Кнопка-акцент: латунная заливка, тёмный текст.
pub fn primary_button(ui: &mut egui::Ui, label: &str, p: Palette) -> egui::Response {
    let text = if p.dark {
        Color32::from_rgb(0x16, 0x18, 0x1C)
    } else {
        Color32::from_rgb(0xFF, 0xFF, 0xFF)
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(text).strong())
            .fill(p.accent)
            .corner_radius(6u8)
            .min_size(egui::vec2(92.0, 28.0)),
    )
}

/// Кнопка-действие: обычная подложка панели.
pub fn action_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(label)
            .fill(ui.visuals().widgets.inactive.weak_bg_fill)
            .corner_radius(6u8)
            .min_size(egui::vec2(92.0, 28.0)),
    )
}

/// Меню приложения.
pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("menu")
        .exact_size(26.0)
        .frame(egui::Frame::new().fill(crate::theme::current().panel))
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::menu::MenuButton::new("Файл").ui(ui, |ui| {
                    if ui.button("Открыть архив…").clicked() {
                        ui.close();
                        pick_and_open(app);
                    }
                    if ui.button("Создать архив…").clicked() {
                        ui.close();
                        app.modal = Modal::Create(crate::app::CreateDlg::from_sources(Vec::new()));
                    }
                    ui.separator();
                    if ui.button("Выход").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                egui::menu::MenuButton::new("Вид").ui(ui, |ui| {
                    egui::menu::MenuButton::new("Тема").ui(ui, |ui| {
                        if ui.selectable_label(app.cfg.dark, "Тёмная").clicked() {
                            app.cfg.dark = true;
                            app.cfg.save();
                        }
                        if ui.selectable_label(!app.cfg.dark, "Светлая").clicked() {
                            app.cfg.dark = false;
                            app.cfg.save();
                        }
                    });
                    egui::menu::MenuButton::new("Акцент").ui(ui, |ui| {
                        for accent in Accent::ALL {
                            if ui
                                .selectable_label(app.cfg.accent == accent, accent.name())
                                .clicked()
                            {
                                app.cfg.accent = accent;
                                app.cfg.save();
                            }
                        }
                    });
                });

                egui::menu::MenuButton::new("Справка").ui(ui, |ui| {
                    if ui.button("О программе").clicked() {
                        ui.close();
                        app.modal = Modal::About;
                    }
                });
            });
        });
}

/// Открывает архив через системный диалог.
fn pick_and_open(app: &mut App) {
    let picked = rfd::FileDialog::new()
        .add_filter("Архивы PlicZip", &["plz"])
        .set_title("Открыть архив")
        .pick_file();
    if let Some(path) = picked {
        app.begin_open(path);
    }
}

/// Тулбар действий.
pub fn toolbar(app: &mut App, ui: &mut egui::Ui, busy: bool) {
    let p = crate::theme::current();
    egui::Panel::top("toolbar")
        .exact_size(46.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .stroke(Stroke::new(1.0, p.line)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if primary_button(ui, "+ Добавить", p).clicked() && !busy {
                    app.modal = Modal::Create(crate::app::CreateDlg::from_sources(Vec::new()));
                }
                if action_button(ui, "Распаковать").clicked() && !busy {
                    if let Some(loaded) = &app.loaded {
                        let dir = default_extract_dir(&loaded.path);
                        app.modal = Modal::Extract(crate::app::ExtractDlg {
                            output: dir.to_string_lossy().into_owned(),
                            error: None,
                        });
                    }
                }
                ui.separator();
                if action_button(ui, "Проверить").clicked() && !busy && app.loaded.is_some() {
                    app.spawn_test();
                }
                if action_button(ui, "Изменить").clicked() && !busy {
                    // Добавление файлов в существующий архив — в планах.
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(loaded) = &mut app.loaded {
                        let mut filter = loaded.filter.clone();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut filter)
                                .hint_text("Поиск по архиву")
                                .desired_width(210.0),
                        );
                        if resp.changed() {
                            if let Some(loaded) = &mut app.loaded {
                                loaded.filter = filter;
                            }
                        }
                    }
                });
            });
        });
}

/// Каталог распаковки по умолчанию: рядом с архивом, его же именем.
fn default_extract_dir(archive: &std::path::Path) -> std::path::PathBuf {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "распаковано".into());
    archive
        .parent()
        .map(|d| d.join(stem.clone()))
        .unwrap_or_else(|| std::path::PathBuf::from(stem))
}

/// Правая панель: сводка и состояние целостности.
pub fn side_panel(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    egui::Panel::right("summary")
        .exact_size(268.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel2)
                .inner_margin(egui::Margin::symmetric(14, 14))
                .stroke(Stroke::new(1.0, p.line)),
        )
        .show(ui, |ui| {
            let Some(loaded) = app.loaded.as_ref() else {
                return;
            };
            let index = &loaded.meta.index;
            let (files, dirs, links) = index.counts();

            ui.label(egui::RichText::new("СВОДКА").size(11.5).color(p.dim));
            ui.add_space(6.0);
            kv(ui, "Формат", &format!("PLZ{}", index.version), p);
            kv(ui, "Режим", if index.solid { "solid" } else { "обычный" }, p);
            kv(ui, "Блоков", &index.blocks.len().to_string(), p);
            kv(
                ui,
                "Элементов",
                &plural_ru(files + dirs + links, "элемент", "элемента", "элементов"),
                p,
            );
            if loaded.meta.encrypted {
                kv(ui, "Шифрование", "ChaCha20-Poly1305", p);
            }
            if loaded.meta.volumes > 1 {
                kv(ui, "Томов", &loaded.meta.volumes.to_string(), p);
            }
            if loaded.meta.archive_bytes > 0 {
                kv(
                    ui,
                    "Файл",
                    &pliczip::util::human_bytes(loaded.meta.archive_bytes),
                    p,
                );
            }
            if index.created_at > 0 {
                kv(
                    ui,
                    "Создан",
                    &pliczip::util::format_unix_time(index.created_at),
                    p,
                );
            }

            ui.add_space(14.0);
            ui.label(egui::RichText::new("ОБЪЁМ").size(11.5).color(p.dim));
            ui.add_space(6.0);
            let raw = index.total_size();
            let stored = index.stored_size();
            kv(ui, "До сжатия", &pliczip::util::human_bytes(raw), p);
            kv(ui, "В архиве", &pliczip::util::human_bytes(stored), p);
            let saved = pliczip::util::saved_percent(raw, stored);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Экономия").color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("{saved:.1}%")).color(p.ok));
                });
            });

            ui.add_space(16.0);
            integrity_badge(ui, &loaded.integrity, p);

            ui.add_space(10.0);
            let can_test = app.op.is_none();
            if ui
                .add_enabled(
                    can_test,
                    egui::Button::new("Проверить снова").min_size(egui::vec2(240.0, 30.0)),
                )
                .clicked()
            {
                app.spawn_test();
            }
        });
}

/// Строка «параметр — значение».
fn kv(ui: &mut egui::Ui, key: &str, value: &str, p: Palette) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(egui::RichText::new(value).color(p.text).monospace());
        });
    });
}

/// Плашка целостности: три состояния — зелёный, красный, серый.
fn integrity_badge(ui: &mut egui::Ui, state: &Integrity, p: Palette) {
    let (icon, title, detail, color, soft) = match state {
        Integrity::Verified(at) => (
            "✓",
            "Целостность подтверждена",
            format!("проверено {}", ago(*at)),
            p.ok,
            p.ok_soft(),
        ),
        Integrity::Failed(msg) => (
            "✗",
            "Проверка не пройдена",
            msg.clone(),
            p.err,
            p.err_soft(),
        ),
        Integrity::Untested => (
            "—",
            "Не проверено",
            "нажмите «Проверить»".to_string(),
            p.muted,
            p.neutral_soft(),
        ),
    };

    egui::Frame::new()
        .fill(soft)
        .corner_radius(8u8)
        .inner_margin(egui::Margin::symmetric(10, 10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(icon).size(16.0).color(color));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).strong().color(color));
                    ui.label(egui::RichText::new(detail).size(11.5).color(p.muted));
                });
            });
        });
}

/// «2 мин назад» — человекочитаемо.
fn ago(at: std::time::Instant) -> String {
    let secs = at.elapsed().as_secs();
    if secs < 60 {
        "только что".to_string()
    } else if secs < 3600 {
        format!("{} мин назад", secs / 60)
    } else {
        format!("{} ч назад", secs / 3600)
    }
}

/// Дата из unix-времени в виде `14.06.2026`.
fn fmt_date(ts: i64) -> String {
    let text = pliczip::util::format_unix_time(ts);
    if text.len() < 10 {
        return String::new();
    }
    let d = &text[0..10];
    format!("{}.{}.{}", &d[8..10], &d[5..7], &d[0..4])
}

/// Центральная область: таблица содержимого.
pub fn central(app: &mut App, ui: &mut egui::Ui, p: Palette, busy: bool) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.panel))
        .show(ui, |ui| {
            let Some(loaded) = app.loaded.as_ref() else {
                empty_state(ui, p);
                return;
            };
            let index = &loaded.meta.index;
            let filter = loaded.filter.to_lowercase();
            let selected = loaded.selected;

            // Заголовок колонок.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("ИМЯ").size(11.5).color(p.dim));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new("ИЗМЕНЁН").size(11.5).color(p.dim));
                    ui.label(egui::RichText::new("МЕТОД").size(11.5).color(p.dim));
                    ui.label(egui::RichText::new("В АРХИВЕ").size(11.5).color(p.dim));
                    ui.label(egui::RichText::new("РАЗМЕР").size(11.5).color(p.dim));
                });
            });
            ui.separator();

            let mut clicked: Option<usize> = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for (i, entry) in index.entries.iter().enumerate() {
                    if !filter.is_empty() && !entry.path.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let block = entry.block.and_then(|b| index.blocks.get(b as usize));
                    let is_dir = entry.kind == Kind::Dir;
                    let is_link = entry.kind == Kind::Symlink;

                    let size_text = if is_dir {
                        "—".to_string()
                    } else {
                        pliczip::util::human_bytes(entry.size)
                    };
                    let method_text = block.map(|b| b.method.name()).unwrap_or("—");
                    // Доля файла в сжатом блоке: в обычном режиме точная, в solid —
                    // пропорциональная, иначе сумма по файлам не сойдётся с блоком.
                    let packed_text = match (block, is_dir) {
                        (Some(b), false) if b.size > 0 => pliczip::util::human_bytes(
                            b.stored_size.saturating_mul(entry.size) / b.size,
                        ),
                        _ => "—".to_string(),
                    };
                    let date_text = if entry.mtime > 0 {
                        fmt_date(entry.mtime)
                    } else {
                        "—".to_string()
                    };
                    let depth = entry.path.matches('/').count();

                    let row = ui.horizontal(|ui| {
                        let icon = if is_dir {
                            "📁"
                        } else if is_link {
                            "🔗"
                        } else {
                            "📄"
                        };
                        let indent = "  ".repeat(depth);
                        let mut text = egui::RichText::new(format!("{icon} {indent}{}", entry.path))
                            .color(if is_dir { p.accent } else { p.text });
                        if is_dir {
                            text = text.strong();
                        }
                        let name = ui.add(egui::Label::new(text).sense(Sense::click()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(egui::RichText::new(date_text).color(p.dim).monospace());
                            ui.label(
                                egui::RichText::new(method_text)
                                    .color(if method_text == "brotli" {
                                        p.accent
                                    } else {
                                        p.dim
                                    })
                                    .monospace(),
                            );
                            ui.label(egui::RichText::new(packed_text).color(p.muted).monospace());
                            ui.label(egui::RichText::new(size_text).color(p.muted).monospace());
                        });
                        name
                    });

                    if selected == Some(i) {
                        let rect = row.response.rect.expand(2.0);
                        ui.painter().rect_filled(rect, 2.0, p.accent_soft());
                    }
                    if row.response.clicked() {
                        clicked = Some(i);
                    }
                }
            });

            if let Some(i) = clicked {
                if let Some(loaded) = &mut app.loaded {
                    loaded.selected = Some(i);
                }
            }
            if busy {
                ui.spinner();
            }
        });
}

/// Пустое состояние: подсказка про перетаскивание.
fn empty_state(ui: &mut egui::Ui, p: Palette) {
    egui::Frame::new()
        .fill(p.bg)
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                ui.label(egui::RichText::new("PlicZip").size(30.0).color(p.accent));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Перетащите архив .plz в окно или откройте его из меню «Файл».",
                    )
                    .color(p.muted),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Создать новый архив: «Файл» → «Создать архив…»")
                        .size(12.5)
                        .color(p.dim),
                );
            });
        });
}

/// Статусбар.
pub fn status_bar(app: &mut App, ui: &mut egui::Ui) {
    let p = crate::theme::current();
    egui::Panel::bottom("status")
        .exact_size(26.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel2)
                .inner_margin(egui::Margin::symmetric(12, 4))
                .stroke(Stroke::new(1.0, p.line)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(op) = &app.op {
                    ui.label(egui::RichText::new(format!("{}…", op.label)).color(p.accent));
                } else if let Some(text) = &app.last_action {
                    ui.label(egui::RichText::new(text).color(p.ok));
                } else {
                    ui.label(egui::RichText::new("Готово").color(p.dim));
                }

                if let Some(loaded) = &app.loaded {
                    let index = &loaded.meta.index;
                    let methods: Vec<&str> = {
                        let mut v: Vec<&str> = index.blocks.iter().map(|b| b.method.name()).collect();
                        v.sort_unstable();
                        v.dedup();
                        v
                    };
                    let left = if methods.is_empty() {
                        format!("{} блоков", index.blocks.len())
                    } else {
                        format!(
                            "{} блоков · {} · {}",
                            index.blocks.len(),
                            methods.join("/"),
                            if index.solid { "solid" } else { "обычный" }
                        )
                    };
                    ui.label(egui::RichText::new(left).color(p.dim));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let raw = index.total_size();
                        let stored = index.stored_size();
                        let saved = pliczip::util::saved_percent(raw, stored);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} → {} · {saved:.1}%",
                                pliczip::util::human_bytes(raw),
                                pliczip::util::human_bytes(stored)
                            ))
                            .color(p.dim)
                            .monospace(),
                        );
                    });
                }
            });
        });
}
