#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Write;

fuzz_target!(|data: &[u8]| {
    let Ok(mut file) = tempfile::NamedTempFile::new() else {
        return;
    };
    if file.write_all(data).is_err() {
        return;
    }

    // Archive::open должен только вернуть ошибку или безопасно отвергнуть ввод,
    // но никогда не паниковать и не зависать на произвольных байтах.
    let _ = pliczip::reader::Archive::open(file.path());
});
