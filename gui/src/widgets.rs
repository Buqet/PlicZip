//! Рисованные иконки и мелкие виджеты.
//!
//! Эмодзи из шрифта отображаются не везде (на части систем вместо 📁 — пустой
//! квадрат), поэтому всё, что должно выглядеть одинаково, рисуется векторно.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui,
};
use std::f32::consts::{PI, TAU};

use crate::theme::Palette;

/// Набор рисованных иконок.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    File,
    Link,
    Archive,
    Plus,
    Extract,
    Shield,
    Check,
    Cross,
    Lock,
    Search,
    Sun,
    Moon,
    Close,
    Layers,
}

/// Смешивает два цвета: `t = 0` — `a`, `t = 1` — `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Точки дуги окружности.
pub fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    let n = n.max(2);
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// Рисует иконку в квадрате `rect` (сетка 16×16). `bg` — цвет подложки,
/// нужен иконкам с «вырезами» (луна).
pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32, bg: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0;
    let p = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(p(x0, y0), p(x1, y1));
    let stroke = Stroke::new((1.4 * u).max(1.0), color);
    let soft = color.gamma_multiply(0.22);

    match icon {
        Icon::Folder => {
            painter.rect_filled(r(1.5, 2.5, 7.0, 6.0), 1.2 * u, color);
            painter.rect(r(1.5, 4.5, 14.5, 13.5), 1.6 * u, soft, stroke, StrokeKind::Inside);
            painter.rect_filled(r(1.5, 5.5, 14.5, 7.0), 0.0, color.gamma_multiply(0.55));
        }
        Icon::File => {
            let outline = vec![p(3.0, 1.5), p(10.0, 1.5), p(13.0, 4.5), p(13.0, 14.5), p(3.0, 14.5)];
            painter.add(Shape::convex_polygon(outline.clone(), soft, Stroke::NONE));
            painter.add(Shape::closed_line(outline, stroke));
            painter.add(Shape::line(vec![p(10.0, 1.5), p(10.0, 4.5), p(13.0, 4.5)], stroke));
            painter.line_segment([p(5.5, 8.0), p(10.5, 8.0)], stroke);
            painter.line_segment([p(5.5, 11.0), p(9.0, 11.0)], stroke);
        }
        Icon::Link => {
            painter.rect_stroke(r(1.5, 5.5, 9.5, 10.5), 2.5 * u, stroke, StrokeKind::Middle);
            painter.rect_stroke(r(6.5, 5.5, 14.5, 10.5), 2.5 * u, stroke, StrokeKind::Middle);
        }
        Icon::Archive => {
            painter.rect(r(2.5, 5.0, 13.5, 14.5), 1.5 * u, soft, stroke, StrokeKind::Inside);
            painter.rect_filled(r(1.5, 2.0, 14.5, 5.5), 1.5 * u, color);
            painter.rect_filled(r(6.8, 7.0, 9.2, 10.8), 0.8 * u, color);
            painter.line_segment([p(8.0, 5.5), p(8.0, 7.0)], stroke);
        }
        Icon::Plus => {
            let st = Stroke::new((1.8 * u).max(1.0), color);
            painter.line_segment([p(8.0, 3.0), p(8.0, 13.0)], st);
            painter.line_segment([p(3.0, 8.0), p(13.0, 8.0)], st);
        }
        Icon::Extract => {
            painter.add(Shape::line(
                vec![p(2.0, 9.5), p(2.0, 14.0), p(14.0, 14.0), p(14.0, 9.5)],
                stroke,
            ));
            painter.line_segment([p(8.0, 11.0), p(8.0, 2.0)], stroke);
            painter.add(Shape::line(vec![p(4.5, 5.5), p(8.0, 2.0), p(11.5, 5.5)], stroke));
        }
        Icon::Shield | Icon::Check | Icon::Cross => {
            let shield = vec![
                p(8.0, 1.5),
                p(13.5, 3.5),
                p(13.5, 8.0),
                p(12.0, 11.5),
                p(8.0, 14.5),
                p(4.0, 11.5),
                p(2.5, 8.0),
                p(2.5, 3.5),
            ];
            painter.add(Shape::convex_polygon(shield.clone(), soft, Stroke::NONE));
            painter.add(Shape::closed_line(shield, stroke));
            match icon {
                Icon::Check => {
                    painter.add(Shape::line(vec![p(5.3, 8.0), p(7.3, 10.0), p(10.8, 6.0)], stroke));
                }
                Icon::Cross => {
                    painter.line_segment([p(5.8, 5.8), p(10.2, 10.2)], stroke);
                    painter.line_segment([p(10.2, 5.8), p(5.8, 10.2)], stroke);
                }
                _ => {
                    painter.line_segment([p(5.5, 8.0), p(10.5, 8.0)], stroke);
                }
            }
        }
        Icon::Lock => {
            let mut shackle = vec![p(4.8, 7.5)];
            shackle.extend(arc(p(8.0, 5.5), 3.2 * u, PI, TAU, 12));
            shackle.push(p(11.2, 7.5));
            painter.add(Shape::line(shackle, stroke));
            painter.rect_filled(r(3.0, 7.0, 13.0, 14.5), 1.6 * u, color);
            painter.circle_filled(p(8.0, 10.3), 1.1 * u, bg);
        }
        Icon::Search => {
            painter.circle_stroke(p(7.0, 7.0), 4.5 * u, stroke);
            painter.line_segment([p(10.4, 10.4), p(14.0, 14.0)], Stroke::new(stroke.width * 1.2, color));
        }
        Icon::Sun => {
            painter.circle_filled(c, 3.0 * u, color);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment([c + d * 5.0 * u, c + d * 7.0 * u], stroke);
            }
        }
        Icon::Moon => {
            painter.circle_filled(p(8.0, 8.5), 5.8 * u, color);
            painter.circle_filled(p(11.0, 5.5), 4.8 * u, bg);
        }
        Icon::Close => {
            painter.line_segment([p(4.0, 4.0), p(12.0, 12.0)], stroke);
            painter.line_segment([p(12.0, 4.0), p(4.0, 12.0)], stroke);
        }
        Icon::Layers => {
            let top = vec![p(8.0, 2.0), p(14.0, 5.5), p(8.0, 9.0), p(2.0, 5.5)];
            painter.add(Shape::convex_polygon(top.clone(), soft, Stroke::NONE));
            painter.add(Shape::closed_line(top, stroke));
            painter.add(Shape::line(vec![p(2.0, 8.5), p(8.0, 12.0), p(14.0, 8.5)], stroke));
            painter.add(Shape::line(vec![p(2.0, 11.0), p(8.0, 14.5), p(14.0, 11.0)], stroke));
        }
    }
}

/// Иконка как отдельный элемент разметки.
pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let bg = crate::theme::current().panel;
    paint_icon(ui.painter(), rect, icon, color, bg);
    resp
}

/// Кольцевая диаграмма: дорожка и дуга доли `frac` (0..1), старт сверху.
pub fn ring(painter: &Painter, center: Pos2, radius: f32, width: f32, frac: f32, track: Color32, color: Color32) {
    painter.circle_stroke(center, radius, Stroke::new(width, track));
    let frac = frac.clamp(0.0, 1.0);
    if frac > 0.002 {
        let start = -PI / 2.0;
        let n = (72.0 * frac).ceil() as usize;
        let pts = arc(center, radius, start, start + TAU * frac, n);
        // Скруглённые концы дуги.
        painter.circle_filled(pts[0], width / 2.0, color);
        painter.circle_filled(*pts.last().unwrap(), width / 2.0, color);
        painter.add(Shape::line(pts, Stroke::new(width, color)));
    }
}

/// Вид кнопки.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Главное действие: заливка акцентом.
    Primary,
    /// Обычная кнопка на подложке.
    Normal,
    /// Прозрачная, проявляется при наведении.
    Ghost,
}

/// Кнопка с рисованной иконкой.
pub fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, kind: Kind, enabled: bool) -> Response {
    let p = crate::theme::current();
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text);
    let pad = if label.is_empty() { 8.0 } else { 13.0 };
    let gap = if icon.is_some() && !label.is_empty() { 7.0 } else { 0.0 };
    let icon_w = if icon.is_some() { 16.0 } else { 0.0 };
    let width = (pad * 2.0 + icon_w + gap + galley.size().x).max(32.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 32.0), sense);

    if ui.is_rect_visible(rect) {
        let hovered = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let base_bg = ui.visuals().panel_fill;
        let (mut fill, mut fg, stroke) = match kind {
            Kind::Primary => (p.accent, p.on_accent(), Stroke::NONE),
            Kind::Normal => (p.panel2, p.text, Stroke::new(1.0, p.line)),
            Kind::Ghost => (base_bg, p.muted, Stroke::NONE),
        };
        if hovered {
            match kind {
                Kind::Primary => fill = mix(p.accent, Color32::WHITE, 0.14),
                Kind::Normal => fill = mix(p.panel2, p.accent, 0.10),
                Kind::Ghost => {
                    fill = mix(base_bg, p.accent, 0.14);
                    fg = p.text;
                }
            }
        }
        if pressed {
            fill = mix(fill, Color32::BLACK, 0.12);
        }
        if !enabled {
            fill = mix(fill, base_bg, 0.55);
            fg = mix(fg, base_bg, 0.6);
        }
        let painter = ui.painter();
        let stroke = if hovered && kind == Kind::Normal {
            Stroke::new(1.0, p.accent.gamma_multiply(0.7))
        } else {
            stroke
        };
        painter.rect(rect, 8.0, fill, stroke, StrokeKind::Inside);

        let content_w = icon_w + gap + galley.size().x;
        let mut x = rect.center().x - content_w / 2.0;
        if let Some(icon) = icon {
            let ir = Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0));
            paint_icon(painter, ir, icon, fg, fill);
            x += 16.0 + gap;
        }
        if !label.is_empty() {
            painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font, fg);
        }
    }
    if enabled {
        resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    resp
}

/// Плашка-«чип»: короткий признак архива.
pub fn chip(ui: &mut Ui, icon: Option<Icon>, text: &str, color: Color32) -> Response {
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), color);
    let icon_w = if icon.is_some() { 12.0 + 5.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 18.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    let fill = mix(ui.visuals().panel_fill, color, 0.13);
    painter.rect(rect, 12.0, fill, Stroke::new(1.0, color.gamma_multiply(0.35)), StrokeKind::Inside);
    let mut x = rect.left() + 9.0;
    if let Some(icon) = icon {
        let ir = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0));
        paint_icon(painter, ir, icon, color, fill);
        x += icon_w;
    }
    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, font, color);
    resp
}

/// Заголовок раздела: мелкие прописные буквы.
pub fn section(ui: &mut Ui, text: &str, p: Palette) {
    ui.label(egui::RichText::new(text).size(11.0).color(p.dim).strong());
}

/// Карточка: панель со скруглением и тонкой рамкой.
pub fn card<R>(ui: &mut Ui, p: Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.panel)
        .corner_radius(12u8)
        .stroke(Stroke::new(1.0, p.line))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Пунктирный прямоугольник со скруглёнными углами (зона перетаскивания).
pub fn dashed_rect(painter: &Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut pts = Vec::new();
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -PI / 2.0),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), PI / 2.0),
        (pos2(rect.left() + r, rect.top() + r), PI),
    ];
    for (c, a) in corners {
        pts.extend(arc(c, r, a, a + PI / 2.0, 8));
    }
    pts.push(pts[0]);
    painter.extend(Shape::dashed_line(&pts, stroke, 7.0, 5.0));
}

/// Маленький треугольник-стрелка сортировки.
pub fn sort_arrow(painter: &Painter, center: Pos2, desc: bool, color: Color32) {
    let (a, b, c) = if desc {
        (center + vec2(-3.5, -2.0), center + vec2(3.5, -2.0), center + vec2(0.0, 2.5))
    } else {
        (center + vec2(-3.5, 2.0), center + vec2(3.5, 2.0), center + vec2(0.0, -2.5))
    };
    painter.add(Shape::convex_polygon(vec![a, b, c], color, Stroke::NONE));
}
