//! Иконка PlicZip, общая с Rune preview.

/// Загружает PNG иконки, встроенный в egui-бинарник.
pub fn icon_data() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../assets/pliczip-icon.png"))
        .expect("embedded PlicZip icon must be a valid PNG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_size_and_content() {
        let data = icon_data();
        assert_eq!((data.width, data.height), (512, 512));
        assert_eq!(data.rgba.len(), 512 * 512 * 4);
        assert!(data.rgba.chunks(4).any(|pixel| pixel[3] == 255));
    }
}
