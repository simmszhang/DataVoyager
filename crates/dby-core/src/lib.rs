//! dby-core — 纯 Rust 数据库引擎。
//!
//! 本 crate 不依赖 Tauri / GUI，可独立用 `cargo test` 测试，并被桌面端、
//! 未来的 CLI / MCP / Web 复用。

pub mod config;
pub mod dialect;
pub mod driver;
pub mod error;
pub mod history;
pub mod metadata;
pub mod project;
pub mod query;
pub mod value;
