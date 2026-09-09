//! AnkiTov service layer.
//!
//! # Architecture
//!
//! Two orthogonal access zones provide Anki data to the application:
//!
//! ## Zone 1 — Direct SQLite (Forensic Reader)
//! [`ForensicReader`] opens `.anki2` database files directly via `rusqlite`.
//! **Read-only only.** Handles cold-start health checks, retention curve
//! forensics, WAL telemetry aggregation, and batch analytics. Never writes.
//!
//! ## Zone 2 — AnkiConnect Direct (HTTP Client)
//! [`AnkiConnectClient`] calls Anki's JSON-RPC API at `localhost:8765` via
//! `reqwest`. All writes are scheduler-safe. Handles live operations:
//! sync, .apkg export/import, suspend, backup, and live "what's due now"
//! queries.
//!
//! ## Zone 3 — Composed Analytics (Health Reporter)
//! [`DeckHealthReporter`] composes Zone 1 + Zone 2 into a single dashboard
//! endpoint. Returns a fused report with live metrics from AnkiConnect and
//! forensic history from SQLite.
//!
//! ## Auto-Launch
//! [`anki_launcher`] ensures Anki is running with AnkiConnect available.
//! If AnkiConnect is not responding, it launches Anki headless using
//! `QT_QPA_PLATFORM=offscreen` and polls until ready.
//!
//! # Configuration
//!
//! Both services are configured from environment / YAML config:
//! - `ANKICONNECT_PORT` — AnkiConnect HTTP port (default: `8765`)
//! - `ANKICONNECT_API_KEY` — Optional API key for AnkiConnect auth
//! - `ANKICOLLECTION_PATH` — Path to `.anki2` file for forensic reads
//!
//! # Safety Rules
//!
//! | Rule | Zone | Enforcement |
//! |------|------|-------------|
//! | Never write to SQLite | Zone 1 | `SQLITE_OPEN_READ_ONLY` flag |
//! | Never write without Anki scheduler | Zone 2 | Only AnkiConnect action whitelist |
//! | Serialize concurrent AnkiConnect calls | Zone 2 | `tokio::sync::Mutex` (concurrency = 1) |
//! | No flashcard or note creation | Both | Disabled in whitelist; not in SQLite direct path |

pub mod anki_connect;
pub mod anki_launcher;
pub mod forensic_reader;
pub mod deck_health;

// Interleaved Mastery Pipeline — Phase 3 (Capsule Generation Engine)
pub mod capsule_slicer;
pub mod fsrs_sort;
pub mod capsule_generator;
pub mod capsule_delivery;
pub mod capsule_generator_job;

// Interleaved Mastery Pipeline — Phase 4 (Telemetry, Analytics, Audit)
pub mod telemetry_ingestor;
pub mod track_health;
pub mod audit_logger;
pub mod rig_nlu;
pub mod student_import;

// Deck verification — P1 producer intake (env-selectable Verifier)
pub mod deck_verification;

// Authentication
pub mod auth;