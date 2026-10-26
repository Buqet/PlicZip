//! Ресурсы Windows: иконка приложения (индекс 0) и значок архива `.plz`
//! (индекс 1) — их показывает Проводник; плюс сведения о версии в свойствах.
//! На других ОС скрипт ничего не делает.

#[allow(dead_code)]
#[path = "src/mark.rs"]
mod mark;

use std::path::PathBuf;

const SIZES: [usize; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/mark.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let write_ico = |name: &str, draw: fn(usize) -> Vec<u8>| {
        let images: Vec<_> = SIZES.iter().map(|&s| (s, draw(s))).collect();
        let path = out.join(name);
        std::fs::write(&path, mark::ico(&images)).expect("запись .ico");
        // Прямые слеши: rc.exe и windres оба их понимают, экранировать не нужно.
        path.display().to_string().replace('\\', "/")
    };
    let app = write_ico("pliczip.ico", mark::app);
    let doc = write_ico("pliczip-archive.ico", mark::document);

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut nums: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    nums.resize(4, 0);
    let csv = format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]);
    let rc = format!(
        r#"1 ICON "{app}"
2 ICON "{doc}"

1 VERSIONINFO
FILEVERSION {csv}
PRODUCTVERSION {csv}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "PlicZip"
      VALUE "FileDescription", "PlicZip archiver"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "pliczip-gui"
      VALUE "OriginalFilename", "pliczip-gui.exe"
      VALUE "ProductName", "PlicZip"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let rc_path = out.join("pliczip.rc");
    std::fs::write(&rc_path, rc).expect("запись .rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|err| panic!("не удалось встроить иконку: {err}"));
}
