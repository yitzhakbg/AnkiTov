pub mod dashboard;
pub mod health;
pub mod locale;
pub mod management;
pub mod sync;

// Authentication
pub mod auth;
pub mod setup;

// Telemetry ingestion
pub mod telemetry;

// Seat stream authorization (Caddy forward_auth target — public by design,
// access is the per-seat token; see seats_stream.rs)
pub mod seats_stream;

// Drill-link front door (public by design — the token IS the credential;
// see drill.rs / T-215)
pub mod drill;