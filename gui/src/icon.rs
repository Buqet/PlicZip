//! Знак PlicZip: молния на тёмной плашке, собирается кодом.
//!
//! Рисуем тот же знак, что и `design/mark.svg`, но без зависимости от
//! SVG-рендерера: геометрия простая (скруглённый квадрат, зубцы, бегунок),
//! а результат кладётся прямо в exe.
/// Геометрия знака в координатах SVG (256×256).
const SIZE: usize = 128;

/// Собирает RGBA-картинку знака.
pub fn rgba() -> Vec<u8> {
    let mut out = vec![0u8; SIZE * SIZE * 4];
    let scale = SIZE as f32 / 256.0;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 / scale;
            let py = y as f32 / scale;
            let (r, g, b, a) = pixel(px, py);
            let i = (y * SIZE + x) * 4;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = a;
        }
    }
    out
}

/// Цвет пикселя знака в координатах 256×256.
fn pixel(x: f32, y: f32) -> (u8, u8, u8, u8) {
    // Плашка: скруглённый квадрат 8..248, радиус 56.
    if !inside_rounded(x, y, 8.0, 8.0, 240.0, 240.0, 56.0) {
        return (0, 0, 0, 0);
    }

    // Фон — градиент от #24282F к #121418.
    let t = ((x + y) / 512.0).clamp(0.0, 1.0);
    let bg = lerp(
        (0x24u8, 0x28u8, 0x2Fu8),
        (0x12u8, 0x14u8, 0x18u8),
        t,
    );

    // Зубцы: два ряда латунных квадратов.
    let mut is_tooth = false;
    for row in [100.0f32, 136.0f32] {
        for col in [44.0f32, 74.0, 104.0, 134.0, 164.0] {
            if inside_rounded(x, y, col, row, 20.0, 20.0, 4.0) {
                is_tooth = true;
            }
        }
    }
    // Бегунок молнии.
    if inside_rounded(x, y, 178.0, 94.0, 46.0, 68.0, 15.0) {
        is_tooth = true;
    }
    if inside_rounded(x, y, 195.0, 72.0, 12.0, 26.0, 6.0) {
        is_tooth = true;
    }

    if is_tooth {
        // Латунный градиент сверху вниз.
        let t = ((y - 72.0) / 122.0).clamp(0.0, 1.0);
        let brass = lerp((0xF2u8, 0xCCu8, 0x7Cu8), (0xA9u8, 0x7Bu8, 0x24u8), t);
        return (brass.0, brass.1, brass.2, 255);
    }

    // Шов и прорезь бегунка — тёмные.
    if inside_rounded(x, y, 36.0, 126.0, 136.0, 4.0, 2.0) {
        return (0x0B, 0x0D, 0x10, 216);
    }
    if inside_rounded(x, y, 192.0, 112.0, 18.0, 32.0, 9.0) {
        return (0x0B, 0x0D, 0x10, 128);
    }

    (bg.0, bg.1, bg.2, 255)
}

/// Точка внутри скруглённого прямоугольника.
fn inside_rounded(x: f32, y: f32, rx: f32, ry: f32, w: f32, h: f32, r: f32) -> bool {
    if x < rx || y < ry || x > rx + w || y > ry + h {
        return false;
    }
    // Проверяем только углы: центр прямоугольный.
    let cx = if x < rx + r {
        rx + r
    } else if x > rx + w - r {
        rx + w - r
    } else {
        x
    };
    let cy = if y < ry + r {
        ry + r
    } else if y > ry + h - r {
        ry + h - r
    } else {
        y
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

/// Смешивает два цвета.
fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

/// Данные иконки окна.
pub fn icon_data() -> eframe::egui::IconData {
    eframe::egui::IconData {
        rgba: rgba(),
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_size_and_content() {
        let data = rgba();
        assert_eq!(data.len(), SIZE * SIZE * 4);
        // Внутри знака есть и непрозрачные пиксели (зубцы)...
        let opaque = data.chunks(4).filter(|c| c[3] == 255).count();
        assert!(opaque > 100, "знак почти пустой: {opaque}");
        // ...и прозрачные по углам (скруглённый квадрат).
        assert_eq!(data[3], 0, "угол должен быть прозрачным");
    }
}
