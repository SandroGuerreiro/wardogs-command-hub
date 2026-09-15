//! Wardogs Command Hub core: capture, OCR, parse, state.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod config;
pub mod dedup;
pub mod maps;
pub mod parser;
