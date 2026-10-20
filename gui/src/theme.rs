//! Палитра из docs/design.md: графит, латунь, зелёный/красный под целостность.
//!
//! Зелёный и красный зарезервированы под проверку архива — их нельзя тратить
//! на «сохранено» или «внимание», иначе цвет перестаёт означать проверку.

use eframe::egui::{self, Color32, Stroke};
use std::sync::RwLock;

use crate::config::{Accent, Config};

/// Набор цветов темы.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Графит — фон приложения.
    pub bg: Color32,
    /// Панель — окна, карточки.
    pub panel: Color32,
    /// Панель-2 — тулбар, боковые панели, статусбар.
    pub panel2: Color32,
    /// Линия — границы, разделители.
    pub line: Color32,
    /// Основной текст.
    pub text: Color32,
    /// Вторичный текст.
    pub muted: Color32,
    /// Тусклый текст — подписи колонок.
    pub dim: Color32,
    /// Акцент (латунь / сталь / зелёный — по настройке).
    pub accent: Color32,
    /// Целостность подтверждена.
    pub ok: Color32,
    /// Повреждён.
    pub err: Color32,
    /// Тёмная ли тема.
    pub dark: bool,
}

/// Акцент для тёмной темы.
fn accent_dark(a: Accent) -> Color32 {
    match a {
        Accent::Brass => Color32::from_rgb(0xE0, 0xB3, 0x56),
        Accent::Steel => Color32::from_rgb(0x8F, 0xA8, 0xC7),
        Accent::Green => Color32::from_rgb(0x3C, 0xB8, 0x8B),
    }
}

/// Акцент для светлой темы: латунь темнеет, иначе на белом не читается.
fn accent_light(a: Accent) -> Color32 {
    match a {
        Accent::Brass => Color32::from_rgb(0xA9, 0x7B, 0x24),
        Accent::Steel => Color32::from_rgb(0x4A, 0x63, 0x82),
        Accent::Green => Color32::from_rgb(0x1F, 0x8F, 0x68),
    }
}

impl Palette {
    pub fn of(cfg: &Config) -> Self {
        if cfg.dark {
            Palette {
                bg: Color32::from_rgb(0x0B, 0x0D, 0x10),
                panel: Color32::from_rgb(0x16, 0x19, 0x1E),
                panel2: Color32::from_rgb(0x1B, 0x1F, 0x25),
                line: Color32::from_rgb(0x26, 0x2B, 0x33),
                text: Color32::from_rgb(0xE7, 0xE9, 0xED),
                muted: Color32::from_rgb(0x9A, 0xA2, 0xAE),
                dim: Color32::from_rgb(0x6D, 0x76, 0x83),
                accent: accent_dark(cfg.accent),
                ok: Color32::from_rgb(0x3C, 0xB8, 0x8B),
                err: Color32::from_rgb(0xE5, 0x48, 0x4D),
                dark: true,
            }
        } else {
            Palette {
                bg: Color32::from_rgb(0xF4, 0xF4, 0xF1),
                panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                panel2: Color32::from_rgb(0xF8, 0xF8, 0xF6),
                line: Color32::from_rgb(0xE2, 0xE2, 0xDC),
                text: Color32::from_rgb(0x19, 0x1B, 0x1F),
                muted: Color32::from_rgb(0x5B, 0x62, 0x6D),
                dim: Color32::from_rgb(0x8A, 0x90, 0x9A),
                accent: accent_light(cfg.accent),
                ok: Color32::from_rgb(0x1F, 0x8F, 0x68),
                err: Color32::from_rgb(0xC9, 0x33, 0x3A),
                dark: false,
            }
        }
    }

    /// Полупрозрачный акцент под выделение строки.
    pub fn accent_soft(self) -> Color32 {
        self.accent.gamma_multiply(if self.dark { 0.16 } else { 0.12 })
    }

    /// Полупрозрачный зелёный под плашку «подтверждено».
    pub fn ok_soft(self) -> Color32 {
        self.ok.gamma_multiply(if self.dark { 0.12 } else { 0.10 })
    }

    /// Полупрозрачный красный под плашку «повреждено».
    pub fn err_soft(self) -> Color32 {
        self.err.gamma_multiply(if self.dark { 0.12 } else { 0.09 })
    }

    /// Полупрозрачный серый под «не проверено».
    pub fn neutral_soft(self) -> Color32 {
        self.muted.gamma_multiply(if self.dark { 0.12 } else { 0.10 })
    }
}

/// Текущая палитра — обновляется в начале каждого кадра.
static PALETTE: RwLock<Palette> = RwLock::new(Palette {
    bg: Color32::from_rgb(0x0B, 0x0D, 0x10),
    panel: Color32::from_rgb(0x16, 0x19, 0x1E),
    panel2: Color32::from_rgb(0x1B, 0x1F, 0x25),
    line: Color32::from_rgb(0x26, 0x2B, 0x33),
    text: Color32::from_rgb(0xE7, 0xE9, 0xED),
    muted: Color32::from_rgb(0x9A, 0xA2, 0xAE),
    dim: Color32::from_rgb(0x6D, 0x76, 0x83),
    accent: Color32::from_rgb(0xE0, 0xB3, 0x56),
    ok: Color32::from_rgb(0x3C, 0xB8, 0x8B),
    err: Color32::from_rgb(0xE5, 0x48, 0x4D),
    dark: true,
});

/// Палитра текущего кадра.
pub fn current() -> Palette {
    *PALETTE.read().unwrap_or_else(|e| e.into_inner())
}

/// Ставит палитру кадра и применяет её к визуалам egui.
pub fn apply(ctx: &egui::Context, cfg: &Config) {
    let palette = Palette::of(cfg);
    *PALETTE.write().unwrap_or_else(|e| e.into_inner()) = palette;

    // Тема выбирается явно: системная настройка тут не нужна, пользователь
    // переключает тему в меню «Вид».
    let theme = if cfg.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);

    let mut visuals = if cfg.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.panel;
    visuals.extreme_bg_color = palette.panel2;
    visuals.faint_bg_color = palette.panel2;
    visuals.override_text_color = Some(palette.text);
    visuals.selection.bg_fill = palette.accent_soft();
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.hyperlink_color = palette.accent;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.line);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.line);
    visuals.widgets.inactive.weak_bg_fill = palette.panel2;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.hovered.weak_bg_fill = palette.panel2;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.weak_bg_fill = palette.accent_soft();
    ctx.set_visuals(visuals);

    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}

