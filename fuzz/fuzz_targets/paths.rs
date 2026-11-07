#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Проверяем, что не-UTF-8 имена корректно отклоняются, а не вызывают
    // панику при преобразовании пути в переносимый формат архива.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(data.to_vec()));
        let _ = pliczip::safety::path_to_archive_string(&path);
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = pliczip::safety::validate_rel_path(text);
        let _ = pliczip::safety::validate_link_target("dir/file", text);
        let _ = pliczip::safety::join_safe(std::path::Path::new("/tmp/out"), text);
    }
});
