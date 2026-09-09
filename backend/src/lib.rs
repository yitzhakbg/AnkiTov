//! AnkiTov Backend Library
//!
//! Exposes all modules so that:
//!   - `cargo test` / integration tests use `lib.rs` as crate root
//!   - `main.rs` imports `App` via `use backend::App;`

pub mod app;
pub mod controllers;
pub mod i18n;
pub mod models;
pub mod server;
pub mod migrations; // resolves to src/migrations/ directory
pub mod services;   // AnkiConnect + Forensic + Composed health services
pub mod middleware;  // JWT auth middleware

// Re-export `App` so `main.rs` can use `backend::App`
pub use app::App;
