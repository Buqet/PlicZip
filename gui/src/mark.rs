//! Знак PlicZip и значок архива `.plz` — чистая отрисовка, без зависимостей.
//!
//! Модуль используют и окно (`icon.rs`, иконка заголовка), и `build.rs`
//! (иконки в ресурсах exe для Проводника), поэтому здесь только `std`.
//! Геометрия — в координатах 256×256, как у `design/mark.svg`; каждый пиксель
//! усредняется по сетке 4×4, чтобы знак не рассыпался на 16×16.

/// Сторона сетки сглаживания.
const AA: usize = 4;

/// Знак приложения размером `size`×`size`, RGBA.
pub fn app(size: usize) -> Vec<u8> {
    render(size, pixel)
}

/// Значок файла `.plz`: лист с загнутым углом и знаком, RGBA.
pub fn document(size: usize) -> Vec<u8> {
    render(size, document_pixel)
}

fn render(size: usize, shade: fn(f32, f32) -> (u8, u8, u8, u8)) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    let step = 256.0 / size as f32;
    for y in 0..size {
        for x in 0..size {
            // Усредняем в предумноженной альфе, иначе края темнеют.
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..AA {
                for sx in 0..AA {
                    let px = (x as f32 + (sx as f32 + 0.5) / AA as f32) * step;
                    let py = (y as f32 + (sy as f32 + 0.5) / AA as f32) * step;
                    let (pr, pg, pb, pa) = shade(px, py);
                    let pa = pa as f32 / 255.0;
                    r += pr as f32 * pa;
                    g += pg as f32 * pa;
                    b += pb as f32 * pa;
                    a += pa;
                }
            }
            let i = (y * size + x) * 4;
            if a > 0.0 {
                out[i] = (r / a).round() as u8;
                out[i + 1] = (g / a).round() as u8;
                out[i + 2] = (b / a).round() as u8;
                out[i + 3] = (a / (AA * AA) as f32 * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Лист 40..216 × 12..244 с загнутым правым верхним углом, знак — в нижней части.
fn document_pixel(x: f32, y: f32) -> (u8, u8, u8, u8) {
    const L: f32 = 40.0;
    const T: f32 = 12.0;
    const R: f32 = 216.0;
    const B: f32 = 244.0;
    const FOLD: f32 = 56.0;
    const EDGE: f32 = 5.0;

    // Знак 136×136 поверх листа.
    let (mx, my, ms) = (60.0, 96.0, 136.0);
    if x >= mx && y >= my && x < mx + ms && y < my + ms {
        let k = 256.0 / ms;
        let m = pixel((x - mx) * k, (y - my) * k);
        if m.3 == 255 {
            return m;
        }
    }

    let in_page = inside_rounded(x, y, L, T, R - L, B - T, 14.0)
        && (x - (R - FOLD)) + (T + FOLD - y) <= FOLD; // срез угла
    if !in_page {
        return (0, 0, 0, 0);
    }
    // Загиб: треугольник под срезом.
    let fx = x - (R - FOLD);
    let fy = y - T;
    if fx >= 0.0 && fy <= FOLD && fx <= fy {
        return (0xC9, 0xBF, 0xAA, 255);
    }
    // Кромка листа — чтобы он читался на белом фоне.
    let near_cut = ((x - (R - FOLD)) + (T + FOLD - y) - FOLD).abs() / std::f32::consts::SQRT_2;
    let edge = (x - L).min(R - x).min(y - T).min(B - y).min(near_cut);
    if edge < EDGE {
        return (0x8A, 0x84, 0x78, 255);
    }
    // Бумага с лёгким вертикальным градиентом.
    let t = ((y - T) / (B - T)).clamp(0.0, 1.0);
    let paper = lerp((0xFB, 0xF9, 0xF4), (0xEC, 0xE7, 0xDC), t);
    (paper.0, paper.1, paper.2, 255)
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

/// Кодирует картинки в ICO. 256 и больше — PNG без сжатия (так их ждёт Windows
/// Vista+), остальные — 32-битные DIB с альфой.
pub fn ico(images: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let blobs: Vec<Vec<u8>> = images
        .iter()
        .map(|(size, rgba)| if *size >= 256 { png(*size, rgba) } else { dib(*size, rgba) })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for ((size, _), blob) in images.iter().zip(&blobs) {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // плоскости
        out.extend_from_slice(&32u16.to_le_bytes()); // бит на пиксель
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += blob.len();
    }
    for blob in blobs {
        out.extend_from_slice(&blob);
    }
    out
}

/// BITMAPINFOHEADER + BGRA снизу вверх + пустая маска AND.
fn dib(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + size * size * 4 + mask_row * size);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // цвет + маска
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // BI_RGB, размеры и палитра не нужны
    for y in (0..size).rev() {
        for x in 0..size {
            let i = (y * size + x) * 4;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.resize(out.len() + mask_row * size, 0);
    out
}

/// Минимальный PNG: RGBA 8 бит, deflate «stored» без сжатия.
fn png(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(size * (size * 4 + 1));
    for row in rgba.chunks(size * 4) {
        raw.push(0); // фильтр None
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(block) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(&data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_icon_has_page_and_mark() {
        let data = document(64);
        assert_eq!(data.len(), 64 * 64 * 4);
        assert_eq!(data[3], 0, "левый верхний угол прозрачный");
        let opaque = data.chunks(4).filter(|c| c[3] == 255).count();
        assert!(opaque > 64 * 64 / 3, "лист почти пустой: {opaque}");
    }

    #[test]
    fn ico_directory_is_consistent() {
        let images: Vec<_> = [16usize, 32, 256].iter().map(|&s| (s, app(s))).collect();
        let ico = ico(&images);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 3, 0]);
        let mut expected_offset = 6 + 16 * 3;
        for (n, (size, _)) in images.iter().enumerate() {
            let e = &ico[6 + 16 * n..6 + 16 * (n + 1)];
            assert_eq!(e[0], if *size >= 256 { 0 } else { *size as u8 });
            let len = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(e[12..16].try_into().unwrap()) as usize;
            assert_eq!(off, expected_offset);
            expected_offset += len;
        }
        assert_eq!(expected_offset, ico.len());
        // Последний — PNG.
        let off = u32::from_le_bytes(ico[6 + 32 + 12..6 + 32 + 16].try_into().unwrap()) as usize;
        assert_eq!(&ico[off..off + 4], b"\x89PNG");
    }

    #[test]
    fn checksums_match_reference_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}
