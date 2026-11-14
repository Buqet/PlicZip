# External archive backends

Chosen pure-Rust crates for read-only support:

- `sevenz-rust` 0.6.1: 7z reader, AES-256/BZip2/Zstd features, docs: https://docs.rs/sevenz-rust/0.6.1
- `rars` 0.12.4: RAR 1.3 through RAR 5/7 reader, streaming extraction and encryption feature, docs: https://docs.rs/rars/0.12.4

The project keeps archive creation exclusively in `.plz`; these backends are only for detection, listing, testing and extraction.
