#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = pliczip::safety::validate_rel_path(text);
        let _ = pliczip::safety::validate_link_target("dir/file", text);
        let _ = pliczip::safety::join_safe(std::path::Path::new("/tmp/out"), text);
    }
});
