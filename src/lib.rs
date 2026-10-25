//! PlicZip — архиватор с собственным форматом `.plz`.
//!
//! Контейнер: индекс в конце файла (как в 7z), блоки данных со сжатием
//! `store` / `deflate` / `brotli`, BLAKE3 на каждый файл и на каждый блок.
//!
//! ```
//! use pliczip::prelude::*;
//!
//! # fn main() -> anyhow::Result<()> {
//! # let dir = std::env::temp_dir().join("pliczip-doc-example");
//! # let _ = std::fs::remove_dir_all(&dir);
//! # std::fs::create_dir_all(&dir)?;
//! # std::fs::write(dir.join("hello.txt"), b"hello pliczip")?;
//! # let archive = dir.join("example.plz");
//! // Упаковать
//! let opts = CreateOptions {
//!     show_progress: false,
//!     ..Default::default()
//! };
//! create(&archive, &[dir.join("hello.txt")], &opts)?;
//!
//! // Распаковать
//! let mut ar = Archive::open(&archive)?;
//! let stats = ar.extract(&ExtractOptions {
//!     output: dir.join("out"),
//!     show_progress: false,
//!     ..Default::default()
//! })?;
//! assert_eq!(stats.files, 1);
//! # let _ = std::fs::remove_dir_all(&dir);
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod compress;
pub mod crypto;
pub mod format;
pub mod glob;
pub mod reader;
pub mod recovery;
pub mod safety;
pub mod source;
pub mod util;
pub mod writer;

/// Основные типы и функции в одном импорте.
pub mod prelude {
    pub use crate::format::{Kind, Method, PzBlock, PzEntry, PzIndex};
    pub use crate::reader::{Archive, ExtractOptions, ExtractStats, TestReport};
    pub use crate::source::{FileSource, Source, VolumeSource};
    pub use crate::writer::{create, CreateOptions, CreateStats};
}
