//! Отрисовка главного окна: меню, тулбар, шапка архива, таблица, сводка,
//! статусбар, оверлей перетаскивания и уведомления.

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CursorIcon, FontId, Layout, Rect, Sense, Stroke,
    StrokeKind,
};
use pliczip::prelude::*;

use crate::app::{packed_size, plural_ru, saved_fraction, App, CreateDlg, Integrity, Modal, Sort};
use crate::config::Accent;
use crate::locale::Locale;
use crate::theme::Palette;
use crate::widgets::{self, mix, Icon, Kind as Btn};

/// Кнопка-акцент (для диалогов).
pub fn primary_button<L: AsRef<str>>(ui: &mut egui::Ui, label: L, _p: Palette) -> egui::Response {
    widgets::button(ui, None, label.as_ref(), Btn::Primary, true)
}

/// Обычная кнопка (для диалогов).
pub fn action_button<L: AsRef<str>>(ui: &mut egui::Ui, label: L) -> egui::Response {
    widgets::button(ui, None, label.as_ref(), Btn::Normal, true)
}

/// Меню приложения.
pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) {
    let p = crate::theme::current();
    let busy = app.op.is_some();
    egui::Panel::top("menu")
        .exact_size(26.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel2)
                .inner_margin(egui::Margin::symmetric(6, 0)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::menu::MenuButton::new(app.locale.text("file")).ui(ui, |ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new(app.locale.text("open-archive")))
                        .clicked()
                    {
                        ui.close();
                        app.pick_and_open();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new(app.locale.text("create-archive")))
                        .clicked()
                    {
                        ui.close();
                        app.modal = Modal::Create(CreateDlg::from_sources(Vec::new()));
                    }
                    if ui
                        .add_enabled(
                            !busy && app.loaded.is_some(),
                            egui::Button::new(app.locale.text("close-archive")),
                        )
                        .clicked()
                    {
                        ui.close();
                        app.loaded = None;
                        app.last_action = None;
                    }
                    ui.separator();
                    if ui.button(app.locale.text("exit")).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                egui::menu::MenuButton::new(app.locale.text("archive")).ui(ui, |ui| {
                    let ready = !busy && app.loaded.is_some();
                    if ui
                        .add_enabled(ready, egui::Button::new(app.locale.text("extract")))
                        .clicked()
                    {
                        ui.close();
                        app.open_extract_dialog();
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new(app.locale.text("verify")))
                        .clicked()
                    {
                        ui.close();
                        app.spawn_test();
                    }
                    if ui
                        .add_enabled(
                            ready
                                && app
                                    .loaded
                                    .as_ref()
                                    .is_some_and(|a| a.meta.recovery.is_some()),
                            egui::Button::new(app.locale.text("repair")),
                        )
                        .clicked()
                    {
                        ui.close();
                        app.open_recovery_wizard();
                    }
                    if ui
                        .add_enabled(
                            app.loaded.is_some(),
                            egui::Button::new(app.locale.text("find")),
                        )
                        .clicked()
                    {
                        ui.close();
                        app.focus_search = true;
                    }
                });

                egui::menu::MenuButton::new(&app.locale.menu_view).ui(ui, |ui| {
                    egui::menu::MenuButton::new(app.locale.text("theme")).ui(ui, |ui| {
                        if ui
                            .selectable_label(app.cfg.dark, app.locale.text("dark"))
                            .clicked()
                        {
                            app.cfg.dark = true;
                            app.cfg.save();
                        }
                        if ui
                            .selectable_label(!app.cfg.dark, app.locale.text("light"))
                            .clicked()
                        {
                            app.cfg.dark = false;
                            app.cfg.save();
                        }
                    });
                    egui::menu::MenuButton::new(app.locale.text("accent")).ui(ui, |ui| {
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
                    ui.separator();
                    if ui
                        .add_enabled(!busy, egui::Button::new(&app.locale.menu_settings))
                        .clicked()
                    {
                        ui.close();
                        app.modal = Modal::Settings;
                    }
                });

                egui::menu::MenuButton::new(&app.locale.menu_help).ui(ui, |ui| {
                    let update_label = match &app.update {
                        crate::app::UpdateState::Checking => &app.locale.update_checking,
                        crate::app::UpdateState::Available { latest, .. } => {
                            let _ = latest;
                            &app.locale.update_available
                        }
                        crate::app::UpdateState::NotFound => &app.locale.update_none,
                        crate::app::UpdateState::Error | crate::app::UpdateState::Idle => {
                            &app.locale.update_check
                        }
                    };
                    if ui
                        .add_enabled(
                            !busy && !matches!(&app.update, crate::app::UpdateState::Checking),
                            egui::Button::new(update_label),
                        )
                        .clicked()
                    {
                        ui.close();
                        if let crate::app::UpdateState::Available { url, .. } = &app.update {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                        } else {
                            app.spawn_update_check();
                        }
                    }
                    ui.separator();
                    if ui.button(&app.locale.menu_about).clicked() {
                        ui.close();
                        app.modal = Modal::About;
                    }
                });
            });
        });
}

/// Тулбар действий.
pub fn toolbar(app: &mut App, ui: &mut egui::Ui, busy: bool) {
    let p = crate::theme::current();
    egui::Panel::top("toolbar")
        .exact_size(56.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(14, 12)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let has = app.loaded.is_some();
                if widgets::button(
                    ui,
                    Some(Icon::Plus),
                    &app.locale.text("create"),
                    Btn::Primary,
                    !busy,
                )
                .on_hover_text(app.locale.text("new-archive-hint"))
                .clicked()
                {
                    app.modal = Modal::Create(CreateDlg::from_sources(Vec::new()));
                }
                if widgets::button(
                    ui,
                    Some(Icon::Folder),
                    &app.locale.text("open"),
                    Btn::Normal,
                    !busy,
                )
                .on_hover_text(app.locale.text("open-archive-hint"))
                .clicked()
                {
                    app.pick_and_open();
                }

                // Тонкий разделитель групп.
                let (r, _) = ui.allocate_exact_size(vec2(9.0, 24.0), Sense::hover());
                ui.painter().line_segment(
                    [r.center_top(), r.center_bottom()],
                    Stroke::new(1.0, p.line),
                );

                if widgets::button(
                    ui,
                    Some(Icon::Extract),
                    &app.locale.text("unpack"),
                    Btn::Normal,
                    has && !busy,
                )
                .on_hover_text("Ctrl+E")
                .clicked()
                {
                    app.open_extract_dialog();
                }
                if widgets::button(
                    ui,
                    Some(Icon::Shield),
                    &app.locale.text("verify-short"),
                    Btn::Normal,
                    has && !busy,
                )
                .on_hover_text(app.locale.text("verify-hint"))
                .clicked()
                {
                    app.spawn_test();
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let theme_icon = if app.cfg.dark { Icon::Sun } else { Icon::Moon };
                    let hint = if app.cfg.dark {
                        app.locale.text("light")
                    } else {
                        app.locale.text("dark")
                    };
                    if widgets::button(ui, Some(theme_icon), "", Btn::Ghost, true)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        app.cfg.dark = !app.cfg.dark;
                        app.cfg.save();
                    }
                    let update_label = match &app.update {
                        crate::app::UpdateState::Checking => &app.locale.update_checking,
                        crate::app::UpdateState::Available { latest, .. } => {
                            let _ = latest;
                            &app.locale.update_available
                        }
                        crate::app::UpdateState::NotFound => &app.locale.update_none,
                        crate::app::UpdateState::Error | crate::app::UpdateState::Idle => {
                            &app.locale.update_check
                        }
                    };
                    if widgets::button(
                        ui,
                        None,
                        update_label,
                        Btn::Ghost,
                        !busy && !matches!(&app.update, crate::app::UpdateState::Checking),
                    )
                    .clicked()
                    {
                        if let crate::app::UpdateState::Available { url, .. } = &app.update {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                        } else {
                            app.spawn_update_check();
                        }
                    }
                    if has {
                        search_box(app, ui, p);
                    }
                });
            });
        });
}

/// Поле поиска с иконкой.
fn search_box(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &mut app.loaded else {
        return;
    };
    let focus = std::mem::take(&mut app.focus_search);
    egui::Frame::new()
        .fill(p.bg)
        .corner_radius(8u8)
        .stroke(Stroke::new(1.0, p.line))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                vec2(262.0, 22.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    widgets::icon(ui, Icon::Search, 14.0, p.dim);
                    let width = if loaded.filter.is_empty() {
                        230.0
                    } else {
                        196.0
                    };
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut loaded.filter)
                            .id_salt("archive_search")
                            .frame(egui::Frame::NONE)
                            .hint_text(app.locale.text("search-hint"))
                            .align(egui::Align2::LEFT_CENTER)
                            .desired_width(width),
                    );
                    if focus {
                        resp.request_focus();
                    }
                    if !loaded.filter.is_empty() {
                        let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                        let color = if resp.hovered() { p.text } else { p.dim };
                        widgets::paint_icon(ui.painter(), r.shrink(3.0), Icon::Close, color, p.bg);
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            loaded.filter.clear();
                        }
                    }
                },
            );
        });
}

/// Шапка открытого архива: имя, путь, признаки и статус проверки.
pub fn archive_header(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &app.loaded else { return };
    egui::Panel::top("archive_header")
        .exact_size(78.0)
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 14,
            bottom: 6,
        }))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Плитка с иконкой архива.
                let (tile, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                let tile_fill = mix(p.bg, p.accent, 0.16);
                ui.painter().rect(
                    tile,
                    12.0,
                    tile_fill,
                    Stroke::new(1.0, p.accent.gamma_multiply(0.4)),
                    StrokeKind::Inside,
                );
                widgets::paint_icon(
                    ui.painter(),
                    tile.shrink(12.0),
                    Icon::Archive,
                    p.accent,
                    tile_fill,
                );
                ui.add_space(6.0);

                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    let name = loaded
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(egui::RichText::new(name).size(19.0).strong().color(p.text));
                    let full =
                        std::path::absolute(&loaded.path).unwrap_or_else(|_| loaded.path.clone());
                    let dir = full
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    ui.add(
                        egui::Label::new(egui::RichText::new(dir).size(12.0).color(p.dim))
                            .truncate(),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let index = &loaded.meta.index;
                    let (icon, text, color) = match &loaded.integrity {
                        Integrity::Verified(_) => {
                            (Icon::Check, app.locale.text("integrity-verified"), p.ok)
                        }
                        Integrity::Failed(_) => {
                            (Icon::Cross, app.locale.text("integrity-failed"), p.err)
                        }
                        Integrity::Untested => {
                            (Icon::Shield, app.locale.text("integrity-untested"), p.muted)
                        }
                    };
                    widgets::chip(ui, Some(icon), &text, color);
                    if loaded.meta.encrypted {
                        widgets::chip(
                            ui,
                            Some(Icon::Lock),
                            &app.locale.text("encrypted"),
                            p.accent,
                        );
                    }
                    if loaded.meta.volumes > 1 {
                        let vols = plural_ru(loaded.meta.volumes, "том", "тома", "томов");
                        widgets::chip(ui, Some(Icon::Layers), &vols, p.muted);
                    }
                    if let Some((percent, _, parity)) = loaded.meta.recovery {
                        widgets::chip(
                            ui,
                            Some(Icon::Shield),
                            &format!("recovery {percent}% · {parity} parity"),
                            p.ok,
                        );
                    }
                    if let Some(kind) = loaded.external_kind {
                        widgets::chip(
                            ui,
                            None,
                            &format!("{} · только чтение", kind.name()),
                            p.accent,
                        );
                    } else if index.solid {
                        widgets::chip(ui, None, "solid", p.muted);
                        let mut methods: Vec<&str> =
                            index.blocks.iter().map(|b| b.method.name()).collect();
                        methods.sort_unstable();
                        methods.dedup();
                        if !methods.is_empty() {
                            widgets::chip(ui, None, &methods.join(" · "), p.muted);
                        }
                        widgets::chip(ui, None, &format!("PLZ{}", index.version), p.muted);
                    }
                });
            });
        });
}

/// Правая панель: сжатие, целостность, свойства архива и выбранного файла.
pub fn side_panel(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    egui::Panel::right("summary")
        .exact_size(300.0)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
            left: 0,
            right: 16,
            top: 6,
            bottom: 14,
        }))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    compression_card(app, ui, p);
                    integrity_card(app, ui, p);
                    selection_card(app, ui, p);
                    archive_card(app, ui, p);
                });
        });
}

/// Карточка «Сжатие»: кольцо экономии и объёмы.
fn compression_card(app: &App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &app.loaded else { return };
    if let Some(kind) = loaded.external_kind {
        widgets::card(ui, p, |ui| {
            widgets::section(ui, &app.locale.text("external-section"), p);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                "Формат: {}\nСоздание внешних архивов выполняется только сторонними программами.",
                kind.name()
            ))
                .color(p.text),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(app.locale.text("external-readonly"))
                    .size(11.5)
                    .color(p.dim),
            );
        });
        return;
    }
    let index = &loaded.meta.index;
    let raw = index.total_size();
    let stored = index.stored_size();
    let saved = pliczip::util::saved_percent(raw, stored);
    widgets::card(ui, p, |ui| {
        widgets::section(ui, &app.locale.text("compression-section"), p);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(104.0, 104.0), Sense::hover());
            let anim = ui.ctx().animate_value_with_time(
                egui::Id::unique(("ring", &loaded.path)),
                saved as f32,
                0.6,
            );
            let painter = ui.painter();
            widgets::ring(
                painter,
                rect.center(),
                44.0,
                9.0,
                anim / 100.0,
                mix(p.panel, p.text, 0.08),
                p.accent,
            );
            painter.text(
                rect.center() - vec2(0.0, 6.0),
                Align2::CENTER_CENTER,
                format!("{anim:.0}%"),
                FontId::proportional(24.0),
                p.text,
            );
            painter.text(
                rect.center() + vec2(0.0, 15.0),
                Align2::CENTER_CENTER,
                app.locale.text("saved-label"),
                FontId::proportional(11.0),
                p.dim,
            );

            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                stat(
                    ui,
                    &app.locale.text("before-compression"),
                    &pliczip::util::human_bytes(raw),
                    p,
                );
                ui.add_space(6.0);
                stat(
                    ui,
                    &app.locale.text("in-archive"),
                    &pliczip::util::human_bytes(stored),
                    p,
                );
            });
        });
        // Полоса: какая доля исходного объёма осталась.
        let frac = if raw == 0 {
            0.0
        } else {
            (stored as f64 / raw as f64).min(1.0) as f32
        };
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        ui.painter()
            .rect_filled(bar, 3.0, mix(p.panel, p.text, 0.08));
        let mut fill = bar;
        fill.set_width((bar.width() * frac).max(6.0));
        ui.painter().rect_filled(fill, 3.0, p.accent);
        ui.label(
            egui::RichText::new(
                app.locale
                    .text("archive-ratio")
                    .replace("{:.1}", &format!("{:.1}", frac * 100.0)),
            )
            .size(11.0)
            .color(p.dim),
        );
    });
}

/// Подпись и крупное значение.
fn stat(ui: &mut egui::Ui, key: &str, value: &str, p: Palette) {
    ui.label(egui::RichText::new(key).size(11.5).color(p.muted));
    ui.label(
        egui::RichText::new(value)
            .size(16.0)
            .color(p.text)
            .monospace(),
    );
}

/// Карточка целостности: три состояния — зелёный, красный, серый.
fn integrity_card(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &app.loaded else { return };
    let (icon, title, detail, color) = match &loaded.integrity {
        Integrity::Verified(at) => (
            Icon::Check,
            app.locale.text("integrity-verified"),
            format!("BLAKE3 сошёлся · {}", ago(*at)),
            p.ok,
        ),
        Integrity::Failed(msg) => (
            Icon::Cross,
            app.locale.text("integrity-failed"),
            msg.clone(),
            p.err,
        ),
        Integrity::Untested => (
            Icon::Shield,
            app.locale.text("integrity-untested"),
            app.locale.text("integrity-detail"),
            p.muted,
        ),
    };
    let busy = app.op.is_some();
    let fill = mix(p.panel, color, 0.08);
    let mut run = false;
    egui::Frame::new()
        .fill(fill)
        .corner_radius(12u8)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                widgets::paint_icon(ui.painter(), r, icon, color, fill);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).strong().color(color));
                    ui.add(
                        egui::Label::new(egui::RichText::new(detail).size(11.5).color(p.muted))
                            .wrap(),
                    );
                });
            });
            ui.add_space(8.0);
            let label = match loaded.integrity {
                Integrity::Untested => app.locale.text("verify"),
                _ => app.locale.text("verify-again"),
            };
            if widgets::button(ui, Some(Icon::Shield), &label, Btn::Normal, !busy).clicked() {
                run = true;
            }
        });
    if run {
        app.spawn_test();
    }
}

/// Карточка «Архив»: свойства.
fn archive_card(app: &App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &app.loaded else { return };
    let index = &loaded.meta.index;
    let (files, dirs, links) = index.counts();
    widgets::card(ui, p, |ui| {
        widgets::section(ui, &app.locale.text("archive-section"), p);
        ui.add_space(2.0);
        kv(ui, &app.locale.text("files-label"), &files.to_string(), p);
        kv(ui, &app.locale.text("dirs-label"), &dirs.to_string(), p);
        if let Some(kind) = loaded.external_kind {
            kv(ui, &app.locale.text("format-label"), kind.name(), p);
            kv(
                ui,
                &app.locale.text("mode-label"),
                &app.locale.text("readonly-label"),
                p,
            );
        }
        if links > 0 {
            kv(ui, &app.locale.text("links-label"), &links.to_string(), p);
        }
        kv(
            ui,
            &app.locale.text("blocks-label"),
            &index.blocks.len().to_string(),
            p,
        );
        if loaded.external_kind.is_none() {
            let mode = if index.solid {
                app.locale.text("mode-solid")
            } else {
                app.locale.text("mode-normal")
            };
            kv(ui, &app.locale.text("mode-label"), &mode, p);
        }
        if loaded.meta.encrypted {
            kv(ui, &app.locale.text("cipher-label"), "ChaCha20-Poly1305", p);
        }
        if let Some((percent, data, parity)) = loaded.meta.recovery {
            kv(
                ui,
                &app.locale.text("recovery-label"),
                &format!("{percent}% · {data}+{parity} кусков"),
                p,
            );
        }
        if loaded.meta.archive_bytes > 0 {
            kv(
                ui,
                &app.locale.text("archive-size-label"),
                &pliczip::util::human_bytes(loaded.meta.archive_bytes),
                p,
            );
        }
        if index.created_at > 0 {
            kv(
                ui,
                &app.locale.text("created-label"),
                &pliczip::util::format_unix_time(index.created_at),
                p,
            );
        }
    });
}

/// Карточка выбранного элемента.
fn selection_card(app: &App, ui: &mut egui::Ui, p: Palette) {
    let Some(loaded) = &app.loaded else { return };
    let index = &loaded.meta.index;
    let Some(entry) = loaded.selected.and_then(|i| index.entries.get(i)) else {
        widgets::card(ui, p, |ui| {
            widgets::section(ui, &app.locale.text("selected-section"), p);
            ui.label(
                egui::RichText::new(app.locale.text("select-row-hint"))
                    .size(12.0)
                    .color(p.dim),
            );
        });
        return;
    };
    widgets::card(ui, p, |ui| {
        widgets::section(ui, &app.locale.text("selected-section"), p);
        ui.horizontal(|ui| {
            let icon = entry_icon(entry.kind);
            widgets::icon(ui, icon, 18.0, entry_color(entry.kind, p));
            let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
            ui.add(egui::Label::new(egui::RichText::new(name).strong().color(p.text)).truncate());
        });
        ui.add(
            egui::Label::new(
                egui::RichText::new(&entry.path)
                    .size(11.5)
                    .color(p.dim)
                    .monospace(),
            )
            .wrap(),
        );
        ui.add_space(2.0);
        match entry.kind {
            Kind::Dir => kv(ui, "Тип", &app.locale.text("folder-type"), p),
            Kind::Symlink => {
                kv(ui, "Тип", &app.locale.text("link-type"), p);
                kv(ui, &app.locale.text("target-label"), &entry.link, p);
            }
            _ => {
                kv(
                    ui,
                    &app.locale.text("size-label"),
                    &pliczip::util::human_bytes(entry.size),
                    p,
                );
                if let Some(packed) = packed_size(index, entry) {
                    kv(
                        ui,
                        &app.locale.text("in-archive"),
                        &pliczip::util::human_bytes(packed),
                        p,
                    );
                }
                if let Some(f) = saved_fraction(index, entry) {
                    kv(
                        ui,
                        &app.locale.text("savings-label"),
                        &format!("{:.1}%", f * 100.0),
                        p,
                    );
                }
                if let Some(b) = entry.block.and_then(|b| index.blocks.get(b as usize)) {
                    kv(ui, &app.locale.text("method-label"), b.method.name(), p);
                }
            }
        }
        if entry.mtime > 0 {
            kv(
                ui,
                &app.locale.text("modified-label"),
                &pliczip::util::format_unix_time(entry.mtime),
                p,
            );
        }
        if entry.kind == Kind::File {
            let hex: String = entry
                .hash
                .iter()
                .take(10)
                .map(|b| format!("{b:02x}"))
                .collect();
            kv(ui, "BLAKE3", &format!("{hex}…"), p);
        }
    });
}

/// Строка «параметр — значение».
fn kv(ui: &mut egui::Ui, key: &str, value: &str, p: Palette) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(value).color(p.text).monospace()).truncate(),
            );
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

fn entry_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Dir => Icon::Folder,
        Kind::Symlink => Icon::Link,
        _ => Icon::File,
    }
}

fn entry_color(kind: Kind, p: Palette) -> Color32 {
    match kind {
        Kind::Dir => p.accent,
        Kind::Symlink => p.muted,
        _ => p.dim,
    }
}

/// Высота строки таблицы.
const ROW_H: f32 = 30.0;
/// Колонки справа налево: ключ сортировки, заголовок, ширина.
const COLUMNS: [(Sort, &str, f32); 5] = [
    (Sort::Date, "ИЗМЕНЁН", 96.0),
    (Sort::Method, "МЕТОД", 74.0),
    (Sort::Ratio, "ЭКОНОМИЯ", 116.0),
    (Sort::Packed, "В АРХИВЕ", 86.0),
    (Sort::Size, "РАЗМЕР", 86.0),
];

/// Раскладка колонок для строки шириной `row`.
fn column_rects(row: Rect) -> (Rect, [Rect; 5]) {
    let mut right = row.right() - 14.0;
    let mut cols = [Rect::NOTHING; 5];
    for (i, (_, _, w)) in COLUMNS.iter().enumerate() {
        cols[i] = Rect::from_min_max(pos2(right - w, row.top()), pos2(right, row.bottom()));
        right -= w + 8.0;
    }
    let name = Rect::from_min_max(
        pos2(row.left() + 14.0, row.top()),
        pos2(right, row.bottom()),
    );
    (name, cols)
}

/// Центральная область: таблица содержимого или пустое состояние.
pub fn central(app: &mut App, ui: &mut egui::Ui, p: Palette, busy: bool) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
            left: 16,
            right: 12,
            top: 6,
            bottom: 14,
        }))
        .show(ui, |ui| {
            if app.loaded.is_none() {
                empty_state(app, ui, p, busy);
                return;
            }
            egui::Frame::new()
                .fill(p.panel)
                .corner_radius(12u8)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    table(app, ui, p);
                });
        });
}

/// Таблица файлов: сортировка по клику на заголовок, дерево или плоский список.
fn table(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    let sort = app.sort;
    let desc = app.sort_desc;
    let mut new_sort: Option<Sort> = None;

    // Заголовок колонок.
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let (name_rect, cols) = column_rects(head);
    let header_font = FontId::proportional(11.0);
    let painter = ui.painter().clone();
    let mut header_cell = |rect: Rect, key: Sort, title: &str, left: bool| {
        let resp = ui
            .interact(rect, ui.scope_id().with(("sort", title)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let active = sort == key;
        let color = if active || resp.hovered() {
            p.text
        } else {
            p.dim
        };
        let (pos, align) = if left {
            (pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER)
        } else {
            (pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER)
        };
        let text_rect = painter.text(pos, align, title, header_font.clone(), color);
        if active {
            let x = if left {
                text_rect.right() + 8.0
            } else {
                text_rect.left() - 8.0
            };
            widgets::sort_arrow(&painter, pos2(x, rect.center().y), desc, p.accent);
        }
        if resp.clicked() {
            new_sort = Some(key);
        }
    };
    header_cell(name_rect, Sort::Path, "ИМЯ", true);
    for (i, (key, title, _)) in COLUMNS.iter().enumerate() {
        header_cell(cols[i], *key, title, false);
    }

    if let Some(key) = new_sort {
        if app.sort == key {
            app.sort_desc = !app.sort_desc;
        } else {
            app.sort = key;
            // Размеры и даты удобнее смотреть от больших к маленьким.
            app.sort_desc = !matches!(key, Sort::Path | Sort::Method);
        }
    }

    // Под модальным окном подсказки строк не показываем.
    let interactive = app.modal_kind() == crate::app::ModalKind::None && app.op.is_none();
    let Some(loaded) = &mut app.loaded else {
        return;
    };
    let flat = loaded.is_flat(sort);
    let index = &loaded.meta.index;
    let rows = loaded.view.len();
    let mut clicked: Option<usize> = None;
    let mut toggle_dir: Option<String> = None;
    let mut context_action: Option<(usize, u8)> = None;
    let locale = app.locale.clone();

    if rows == 0 {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            widgets::icon(ui, Icon::Search, 28.0, p.dim);
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("Ничего не найдено по «{}»", loaded.filter))
                    .color(p.muted),
            );
        });
        return;
    }

    let mono = FontId::monospace(12.5);
    let text_font = FontId::proportional(13.5);
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show_rows(ui, ROW_H, rows, |ui, range| {
            for row_i in range {
                let i = loaded.view[row_i];
                let entry = &index.entries[i];
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                if !ui.is_rect_visible(row) {
                    continue;
                }
                let painter = ui.painter();
                let selected = loaded.selected == Some(i);
                let bg = if selected {
                    Some(mix(p.panel, p.accent, 0.16))
                } else if resp.hovered() {
                    Some(mix(p.panel, p.text, 0.05))
                } else if row_i % 2 == 1 {
                    Some(mix(p.panel, p.text, 0.018))
                } else {
                    None
                };
                if let Some(bg) = bg {
                    painter.rect_filled(row.shrink2(vec2(4.0, 1.0)), 6.0, bg);
                }
                if selected {
                    let bar = Rect::from_min_size(
                        row.left_top() + vec2(4.0, 6.0),
                        vec2(3.0, ROW_H - 12.0),
                    );
                    painter.rect_filled(bar, 1.5, p.accent);
                }
                let (name_rect, cols) = column_rects(row);
                let cy = row.center().y;

                // Имя: в дереве — отступ и только имя, в плоском списке — путь.
                let depth = entry.path.matches('/').count() as f32;
                let mut x = name_rect.left() + if flat { 0.0 } else { depth * 18.0 };
                if !flat && entry.kind == Kind::Dir {
                    let expanded = loaded.expanded.contains(&entry.path);
                    let arrow_rect = Rect::from_center_size(pos2(x + 5.0, cy), vec2(14.0, ROW_H));
                    let arrow = ui.interact(
                        arrow_rect,
                        ui.scope_id().with(("folder", &entry.path)),
                        Sense::click(),
                    );
                    painter.text(
                        pos2(x + 5.0, cy),
                        Align2::CENTER_CENTER,
                        if expanded { "▾" } else { "▸" },
                        FontId::proportional(13.0),
                        if arrow.hovered() { p.accent } else { p.dim },
                    );
                    if arrow.clicked() {
                        toggle_dir = Some(entry.path.clone());
                    }
                    x += 14.0;
                }
                let icon_rect = Rect::from_center_size(pos2(x + 8.0, cy), vec2(16.0, 16.0));
                let row_bg = bg.unwrap_or(p.panel);
                widgets::paint_icon(
                    painter,
                    icon_rect,
                    entry_icon(entry.kind),
                    entry_color(entry.kind, p),
                    row_bg,
                );
                x += 26.0;
                let clip = painter.with_clip_rect(Rect::from_min_max(
                    pos2(x, row.top()),
                    pos2(name_rect.right(), row.bottom()),
                ));
                let (parent, name) = match entry.path.rsplit_once('/') {
                    Some((parent, name)) => (parent, name),
                    None => ("", entry.path.as_str()),
                };
                if flat && !parent.is_empty() {
                    let r = clip.text(
                        pos2(x, cy),
                        Align2::LEFT_CENTER,
                        format!("{parent}/"),
                        text_font.clone(),
                        p.dim,
                    );
                    x = r.right();
                }
                let name_color = if entry.kind == Kind::Dir {
                    p.text
                } else {
                    mix(p.text, p.muted, 0.15)
                };
                clip.text(
                    pos2(x, cy),
                    Align2::LEFT_CENTER,
                    name,
                    text_font.clone(),
                    name_color,
                );

                let right_text = |rect: Rect, text: String, color: Color32| {
                    painter.text(
                        pos2(rect.right(), cy),
                        Align2::RIGHT_CENTER,
                        text,
                        mono.clone(),
                        color,
                    );
                };
                let block = entry.block.and_then(|b| index.blocks.get(b as usize));

                // Дата.
                right_text(
                    cols[0],
                    if entry.mtime > 0 {
                        fmt_date(entry.mtime)
                    } else {
                        "—".into()
                    },
                    p.dim,
                );
                // Метод.
                let method = block.map(|b| b.method.name()).unwrap_or("—");
                let method_color = match method {
                    "brotli" => p.accent,
                    "deflate" => p.muted,
                    _ => p.dim,
                };
                right_text(cols[1], method.to_string(), method_color);
                // Экономия: мини-полоса и процент.
                if let Some(f) = saved_fraction(index, entry) {
                    let c = cols[2];
                    let track = Rect::from_min_size(pos2(c.left(), cy - 3.0), vec2(58.0, 6.0));
                    painter.rect_filled(track, 3.0, mix(p.panel, p.text, 0.08));
                    if f > 0.0 {
                        let mut fill = track;
                        fill.set_width((track.width() * f).max(6.0));
                        painter.rect_filled(fill, 3.0, p.accent.gamma_multiply(0.85));
                    }
                    right_text(c, format!("{:.0}%", f * 100.0), p.muted);
                } else {
                    right_text(cols[2], "—".into(), p.dim);
                }
                // В архиве и размер.
                right_text(
                    cols[3],
                    packed_size(index, entry)
                        .map(pliczip::util::human_bytes)
                        .unwrap_or_else(|| "—".into()),
                    p.muted,
                );
                right_text(
                    cols[4],
                    if entry.kind == Kind::Dir {
                        "—".into()
                    } else {
                        pliczip::util::human_bytes(entry.size)
                    },
                    p.text,
                );

                let resp = if interactive {
                    resp.on_hover_text_at_pointer(&entry.path)
                } else {
                    resp
                };
                if interactive {
                    resp.context_menu(|ui| {
                        if ui
                            .add_enabled(
                                loaded.external_kind.is_none(),
                                egui::Button::new(locale.text("context-rename")),
                            )
                            .clicked()
                        {
                            context_action = Some((i, 1));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                loaded.external_kind.is_none(),
                                egui::Button::new(locale.text("context-delete")),
                            )
                            .clicked()
                        {
                            context_action = Some((i, 2));
                            ui.close();
                        }
                        if ui.button(locale.text("context-select")).clicked() {
                            context_action = Some((i, 3));
                            ui.close();
                        }
                    });
                }
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
        });

    if let Some(i) = clicked {
        loaded.selected = if loaded.selected == Some(i) {
            None
        } else {
            Some(i)
        };
    }
    if let Some(path) = toggle_dir {
        loaded.toggle_dir(&path);
    }
    // Контекстное меню пустого места: создание файла и сортировка.
    let blank = ui.allocate_response(vec2(ui.available_width().max(1.0), 28.0), Sense::click());
    let mut blank_action = 0u8;
    blank.context_menu(|ui| {
        if ui.button(locale.text("context-new-file")).clicked() {
            blank_action = 1;
            ui.close();
        }
        ui.menu_button(locale.text("context-sort"), |ui| {
            if ui.button(locale.text("context-sort-name")).clicked() {
                blank_action = 10;
                ui.close();
            }
            if ui.button(locale.text("context-sort-size")).clicked() {
                blank_action = 11;
                ui.close();
            }
            if ui.button(locale.text("context-sort-date")).clicked() {
                blank_action = 12;
                ui.close();
            }
        });
    });
    if blank_action == 1 {
        app.modal = Modal::NewFile {
            value: locale.text("name-placeholder"),
        };
    } else if blank_action >= 10 {
        app.sort = match blank_action {
            11 => Sort::Size,
            12 => Sort::Date,
            _ => Sort::Path,
        };
        app.sort_desc = false;
    }
    if let Some((i, action)) = context_action {
        match action {
            1 => {
                let value = loaded
                    .meta
                    .index
                    .entries
                    .get(i)
                    .map(|e| e.path.clone())
                    .unwrap_or_default();
                app.modal = Modal::Rename { index: i, value };
            }
            2 => app.spawn_rebuild_entry(Some(i), None, None),
            3 => loaded.selected = Some(i),
            _ => {}
        }
    }
}

/// Пустое состояние: зона перетаскивания и быстрые действия.
fn empty_state(app: &mut App, ui: &mut egui::Ui, p: Palette, busy: bool) {
    let avail = ui.available_rect_before_wrap();
    let size = vec2(
        620.0_f32.min(avail.width() - 40.0),
        500.0_f32.min(avail.height() - 40.0),
    );
    let zone = Rect::from_center_size(avail.center(), size);
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let t = ui
        .ctx()
        .animate_bool_with_time(egui::Id::unique("empty_drop"), hovering, 0.2);

    let painter = ui.painter();
    painter.rect_filled(zone, 20.0, mix(p.panel, p.accent, 0.04 + 0.08 * t));
    widgets::dashed_rect(
        painter,
        zone.shrink(1.0),
        20.0,
        Stroke::new(1.5, mix(p.line, p.accent, 0.35 + 0.65 * t)),
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(zone.shrink(24.0))
            .layout(Layout::top_down(Align::Center)),
    );
    let ui = &mut child;
    ui.add_space(18.0);
    // Иконка в круге с мягким свечением.
    let (r, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(r.center(), 42.0, mix(p.panel, p.accent, 0.10 + 0.08 * t));
    painter.circle_stroke(
        r.center(),
        42.0,
        Stroke::new(1.0, p.accent.gamma_multiply(0.35)),
    );
    let fill = mix(p.panel, p.accent, 0.10);
    widgets::paint_icon(
        painter,
        Rect::from_center_size(r.center(), vec2(38.0, 38.0)),
        Icon::Archive,
        p.accent,
        fill,
    );
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("PlicZip")
            .size(28.0)
            .strong()
            .color(p.text),
    );
    ui.add_space(2.0);
    ui.label(egui::RichText::new(app.locale.text("drop-open")).color(p.muted));
    ui.add_space(18.0);
    let buttons_w = 300.0;
    ui.allocate_ui_with_layout(
        vec2(buttons_w, 34.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            if widgets::button(
                ui,
                Some(Icon::Folder),
                &app.locale.text("open-archive-button"),
                Btn::Primary,
                !busy,
            )
            .clicked()
            {
                app.pick_and_open();
            }
            if widgets::button(
                ui,
                Some(Icon::Plus),
                &app.locale.text("create-archive-button"),
                Btn::Normal,
                !busy,
            )
            .clicked()
            {
                app.modal = Modal::Create(CreateDlg::from_sources(Vec::new()));
            }
        },
    );
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new(app.locale.text("drop-features"))
            .size(11.5)
            .color(p.dim),
    );

    let recent = app.cfg.recent_archives.clone();
    if !recent.is_empty() {
        ui.add_space(18.0);
        ui.allocate_ui_with_layout(
            vec2(440.0, 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(app.locale.text("recent-title"))
                        .strong()
                        .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(app.locale.text("clear-recent")).clicked() {
                        app.clear_recent();
                    }
                });
            },
        );
        egui::ScrollArea::vertical()
            .id_salt("recent_archives")
            .max_height(115.0)
            .show(ui, |ui| {
                for value in recent {
                    let path = std::path::PathBuf::from(&value);
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&value);
                    ui.horizontal(|ui| {
                        let response = ui
                            .add_sized(
                                [330.0, 27.0],
                                egui::Button::new(
                                    egui::RichText::new(format!("▸  {name}")).color(p.text),
                                )
                                .fill(p.panel2),
                            )
                            .on_hover_text(&value);
                        if response.clicked() && !busy {
                            app.open_recent(path.clone());
                        }
                        let ctx = ui.ctx().clone();
                        if ui
                            .small_button(app.locale.text("copy-path"))
                            .on_hover_text(app.locale.text("copy-path-hint"))
                            .clicked()
                        {
                            app.copy_path_to_clipboard(&ctx, &path);
                        }
                        if ui
                            .small_button(app.locale.text("open-location"))
                            .on_hover_text(app.locale.text("open-location-hint"))
                            .clicked()
                        {
                            app.open_recent_location(&ctx, &path);
                        }
                        if ui
                            .small_button(app.locale.text("remove-recent"))
                            .on_hover_text(app.locale.text("remove-recent-hint"))
                            .clicked()
                        {
                            app.remove_recent(&path);
                        }
                    });
                }
            });
    }
}

/// Оверлей при перетаскивании файлов над окном.
pub fn drop_overlay(ctx: &egui::Context, p: Palette, locale: &Locale) {
    let (hovering, plz) = ctx.input(|i| {
        let files = &i.raw.hovered_files;
        let plz = files
            .iter()
            .any(|f| f.path.as_deref().map(crate::app::is_plz).unwrap_or(false));
        (!files.is_empty(), plz)
    });
    let t = ctx.animate_bool_with_time(egui::Id::unique("drop_overlay"), hovering, 0.15);
    if t <= 0.0 {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop_layer"),
    ));
    let screen = ctx.content_rect();
    painter.rect_filled(screen, 0.0, p.bg.gamma_multiply(0.82 * t));
    let zone = screen.shrink(24.0);
    widgets::dashed_rect(
        &painter,
        zone,
        22.0,
        Stroke::new(2.0, p.accent.gamma_multiply(t)),
    );
    let c = zone.center();
    let fill = mix(p.bg, p.accent, 0.15);
    painter.circle_filled(c - vec2(0.0, 40.0), 38.0, fill.gamma_multiply(t));
    let icon = if plz { Icon::Archive } else { Icon::Plus };
    widgets::paint_icon(
        &painter,
        Rect::from_center_size(c - vec2(0.0, 40.0), vec2(34.0, 34.0)),
        icon,
        p.accent.gamma_multiply(t),
        fill,
    );
    let title = if plz {
        locale.text("drop-open-release")
    } else {
        locale.text("drop-create-release")
    };
    painter.text(
        c + vec2(0.0, 22.0),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(22.0),
        p.text.gamma_multiply(t),
    );
    let sub = if plz {
        locale.text("drop-open-hint")
    } else {
        locale.text("drop-create-hint")
    };
    painter.text(
        c + vec2(0.0, 50.0),
        Align2::CENTER_CENTER,
        sub,
        FontId::proportional(13.0),
        p.muted.gamma_multiply(t),
    );
}

/// Уведомление в правом нижнем углу: появляется и тает за 4 секунды.
pub fn toast(app: &mut App, ctx: &egui::Context, p: Palette) {
    let Some(toast) = &app.toast else { return };
    let age = toast.at.elapsed().as_secs_f32();
    const LIFE: f32 = 4.0;
    if age > LIFE {
        app.toast = None;
        return;
    }
    let alpha = (age / 0.2).min(1.0) * ((LIFE - age) / 0.5).min(1.0);
    let slide = (1.0 - (age / 0.25).min(1.0)) * 12.0;
    ctx.request_repaint();

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("toast"),
    ));
    let font = FontId::proportional(13.5);
    let color = if toast.ok { p.ok } else { p.err };
    let galley = painter.layout_no_wrap(toast.text.clone(), font, p.text.gamma_multiply(alpha));
    let size = vec2(galley.size().x + 58.0, 44.0);
    let screen = ctx.content_rect();
    let rect = Rect::from_min_size(
        pos2(
            screen.right() - size.x - 18.0,
            screen.bottom() - size.y - 40.0 + slide,
        ),
        size,
    );
    let fill = mix(p.panel, color, 0.10);
    painter.rect(
        rect,
        12.0,
        fill.gamma_multiply(alpha),
        Stroke::new(1.0, color.gamma_multiply(0.5 * alpha)),
        StrokeKind::Inside,
    );
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(18.0, 18.0));
    let icon = if toast.ok { Icon::Check } else { Icon::Cross };
    widgets::paint_icon(&painter, icon_rect, icon, color.gamma_multiply(alpha), fill);
    painter.galley(
        pos2(rect.left() + 42.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
}

/// Плавающий центр операций: активный прогресс и история завершённых задач.
pub fn operation_center(app: &mut App, ctx: &egui::Context, p: Palette) {
    if !app.operation_center_open {
        return;
    }
    let active = app
        .op
        .as_ref()
        .map(|op| (op.label.clone(), op.percent(), op.speed(), op.eta()));
    egui::Window::new(app.locale.text("operation-center"))
        .id(egui::Id::unique("operation_center"))
        .anchor(Align2::RIGHT_BOTTOM, [-16.0, -42.0])
        .default_width(360.0)
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .corner_radius(12u8)
                .stroke(Stroke::new(1.0, p.line))
                .inner_margin(egui::Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(app.locale.text("operation-center"))
                        .strong()
                        .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(app.locale.text("close")).clicked() {
                        app.operation_center_open = false;
                    }
                });
            });
            ui.separator();
            if let Some((label, percent, speed, eta)) = &active {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.accent));
                    ui.label(egui::RichText::new(label).strong().color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{percent:.0}%"))
                                .color(p.accent)
                                .monospace(),
                        );
                    });
                });
                let (bar, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar, 3.5, mix(p.panel2, p.text, 0.12));
                let mut fill = bar;
                fill.set_width(bar.width() * (*percent / 100.0) as f32);
                ui.painter().rect_filled(fill, 3.5, p.accent);
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if let Some(speed) = speed {
                        ui.label(
                            egui::RichText::new(format_speed(&app.locale, *speed)).color(p.muted),
                        );
                    }
                    if let Some(eta) = eta {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format_eta(&app.locale, *eta)).color(p.muted),
                            );
                        });
                    }
                });
                ui.separator();
            }

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(app.locale.text("operation-history"))
                        .strong()
                        .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !app.operation_history.is_empty()
                        && ui.small_button(app.locale.text("clear-history")).clicked()
                    {
                        app.clear_operation_history();
                    }
                });
            });
            if app.operation_history.is_empty() {
                ui.label(
                    egui::RichText::new(app.locale.text("operation-history-empty")).color(p.dim),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .show(ui, |ui| {
                        for item in &app.operation_history {
                            ui.horizontal(|ui| {
                                let color = if item.ok { p.ok } else { p.err };
                                let icon = if item.ok { Icon::Check } else { Icon::Cross };
                                let (icon_rect, _) =
                                    ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                widgets::paint_icon(ui.painter(), icon_rect, icon, color, p.panel);
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(&item.label).color(p.text));
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} · {}",
                                            item.detail,
                                            format_duration(&app.locale, item.elapsed)
                                        ))
                                        .size(11.0)
                                        .color(p.muted),
                                    );
                                });
                            });
                            ui.add_space(5.0);
                        }
                    });
            }
        });
}

fn format_speed(locale: &Locale, bytes_per_second: f64) -> String {
    let speed = pliczip::util::human_bytes(bytes_per_second.max(0.0) as u64);
    locale.format("operation-speed", &[&speed])
}

fn format_eta(locale: &Locale, duration: std::time::Duration) -> String {
    locale.format("operation-eta", &[&format_duration(locale, duration)])
}

fn format_duration(locale: &Locale, duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        let value = seconds.to_string();
        locale.format("operation-seconds", &[&value])
    } else {
        let value = (seconds / 60).to_string();
        locale.format("operation-minutes", &[&value])
    }
}

/// Статусбар.
pub fn status_bar(app: &mut App, ui: &mut egui::Ui) {
    let p = crate::theme::current();
    egui::Panel::bottom("status")
        .exact_size(28.0)
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(14, 4)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let active = app.op.as_ref().map(|op| (op.label.clone(), op.percent()));
                if let Some((label, percent)) = active {
                    ui.add(egui::Spinner::new().size(12.0).color(p.accent));
                    ui.label(egui::RichText::new(format!("{}…", label)).color(p.accent));
                    let (bar, _) = ui.allocate_exact_size(vec2(140.0, 5.0), Sense::hover());
                    ui.painter()
                        .rect_filled(bar, 2.5, mix(p.panel, p.text, 0.1));
                    let mut fill = bar;
                    fill.set_width(bar.width() * (percent / 100.0) as f32);
                    ui.painter().rect_filled(fill, 2.5, p.accent);
                    ui.label(
                        egui::RichText::new(format!("{percent:.0}%"))
                            .color(p.muted)
                            .monospace(),
                    );
                    if ui
                        .small_button(app.locale.text("operation-center"))
                        .clicked()
                    {
                        app.operation_center_open = true;
                    }
                } else if let Some(text) = &app.last_action {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, p.ok);
                    ui.label(egui::RichText::new(text).color(p.muted));
                } else {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, p.dim);
                    ui.label(egui::RichText::new(app.locale.text("status-done")).color(p.dim));
                }

                if let Some(loaded) = &app.loaded {
                    let index = &loaded.meta.index;
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let raw = index.total_size();
                        let stored = index.stored_size();
                        let saved = pliczip::util::saved_percent(raw, stored);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} → {} · −{saved:.1}%",
                                pliczip::util::human_bytes(raw),
                                pliczip::util::human_bytes(stored)
                            ))
                            .color(p.dim)
                            .monospace(),
                        );
                        ui.label(egui::RichText::new("·").color(p.dim));
                        let shown = loaded.view.len();
                        let total = index.entries.len();
                        let text = if shown == total {
                            plural_ru(total, "элемент", "элемента", "элементов")
                        } else {
                            format!("показано {shown} из {total}")
                        };
                        ui.label(egui::RichText::new(text).color(p.dim));
                    });
                }
            });
        });
}
