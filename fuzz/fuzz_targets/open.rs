#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Явный пароль не даёт fuzzing уйти в интерактивный prompt на случайном
    // вводе, а from_bytes не создаёт временные файлы.
    let _ = pliczip::reader::Archive::from_bytes_with(data, Some(b"fuzz-password"));
});
