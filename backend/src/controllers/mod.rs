pub mod dashboard;
pub mod health;
pub mod locale;
pub mod management;
pub mod sync;

// Authentication
pub mod auth;
pub mod setup;

// Telemetry ingestion
pub mod reviewer;
pub mod telemetry;

// Seat stream authorization (Caddy forward_auth target — public by design,
// access is the per-seat token; see seats_stream.rs)
pub mod seats_stream;