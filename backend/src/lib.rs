//! AnkiTov Backend Library
//!
//! Exposes all modules so that:
//!   - `cargo test` / integration tests use `lib.rs` as crate root
//!   - `main.rs` imports `App` via `use backend::App;`

pub mod app;
pub mod banner; // AnkiTov startup banner (replaces loco.rs's default art)
pub mod controllers;
pub mod i18n;
pub mod middleware; // JWT auth middleware
pub mod migrations; // resolves to src/migrations/ directory
pub mod models;
pub mod openapi;
pub mod server;
pub mod services; // AnkiConnect + Forensic + Composed health services // OpenAPI aggregation + /scalar API reference

// Re-export `App` so `main.rs` can use `backend::App`
pub use app::App;
