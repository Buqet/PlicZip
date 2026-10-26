//! Иконка окна. Сам знак рисуется в [`crate::mark`] — тот же код кладёт
//! иконки в ресурсы exe (см. `build.rs`).

/// Сторона иконки окна.
const SIZE: usize = 128;

/// Собирает RGBA-картинку знака.
pub fn rgba() -> Vec<u8> {
    crate::mark::app(SIZE)
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
