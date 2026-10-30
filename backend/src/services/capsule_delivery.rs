//! Capsule Delivery — HTTP client for the AnkiTov Session Driver addon.
//!
//! After the backend generates a remediation capsule (card IDs), this client
//! delivers it to the Anki addon running inside the student's Anki container.
//! The addon (port 18765) creates a filtered deck, tags the capsule cards,
//! and opens Anki's reviewer so the student can begin their session.
//!
//! ## Protocol
//!
//! | Method | Path | Purpose |
//! |--------|------|---------|
//! | POST | `/session` | Start a new capsule review session |
//! | GET | `/status` | Check current session state |
//! | DELETE | `/session` | Cancel the active session |
//!
//! ## Configuration
//!
//! | Env var | Default | Description |
//! |---------|---------|-------------|
//! | `ANKITOV_CAPSULE_PORT` | `18765` | Addon HTTP listener port (single co-located addon) |
//! | `ANKITOV_CAPSULE_ENDPOINTS` | (none) | Per-student endpoint registry (T-182): comma-separated `name=url` pairs, e.g. `army-student-1=http://host.docker.internal:18701,army-student-2=http://host.docker.internal:18702`. A single bare URL is also accepted and applies to every student. Resolution order for a student: exact name match (case-insensitive) → single bare URL → legacy `ANKITOV_CAPSULE_PORT`/default. |

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default port for the session driver addon.
const DEFAULT_CAPSULE_PORT: u16 = 18765;

/// HTTP timeout for addon communication.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Payload sent to the addon to start a session.
#[derive(Debug, Serialize)]
pub struct StartSessionRequest {
    pub session_uuid: String,
    pub card_ids: Vec<i64>,
    pub capsule_size: usize,
    pub track_profile_name: String,
}

/// Response from the addon after a `/session` POST.
#[derive(Debug, Serialize, Deserialize)]
pub struct StartSessionResponse {
    pub status: String,
    pub session_uuid: String,
    #[serde(default)]
    pub deck_name: Option<String>,
    #[serde(default)]
    pub card_count: usize,
    #[serde(default)]
    pub replaced_session: Option<String>,
}

/// Response from the addon after a `/status` GET.
#[derive(Debug, Deserialize)]
pub struct SessionStatus {
    pub session_active: bool,
    #[serde(default)]
    pub session_uuid: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub cards_total: usize,
    #[serde(default)]
    pub cards_reviewed: usize,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("addon unreachable at {url}: {source}")]
    Unreachable {
        url: String,
        source: reqwest::Error,
    },

    #[error("addon returned error: {0}")]
    AddonError(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("addon returned unexpected status: {0}")]
    UnexpectedStatus(String),
}

// ---------------------------------------------------------------------------
// Endpoint registry (T-182)
// ---------------------------------------------------------------------------

/// Parse `ANKITOV_CAPSULE_ENDPOINTS` into `(name, base_url)` pairs.
///
/// Format: comma-separated `name=url` pairs. A value with no `=` is treated
/// as a single bare URL mapped to the empty name (applies to every student).
/// Empty segments are dropped; surrounding whitespace is trimmed.
fn parse_endpoint_registry(value: Option<&str>) -> Vec<(String, String)> {
    let Some(raw) = value else { return Vec::new() };
    raw.split(',')
        .map(|seg| seg.trim())
        .filter(|seg| !seg.is_empty())
        .map(|seg| match seg.split_once('=') {
            Some((name, url)) => (name.trim().to_string(), url.trim().to_string()),
            None => (String::new(), seg.to_string()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// HTTP client for the AnkiTov Session Driver addon.
///
/// ```rust,ignore
/// let client = CapsuleDelivery::new(18765);
/// let response = client.start_session(
///     "uuid-1234",
///     &[1001, 1002, 1003],
///     25,
///     "Grade 7 Math Profile",
/// ).await?;
/// ```
pub struct CapsuleDelivery {
    http: Client,
    base_url: String,
}

impl CapsuleDelivery {
    /// Create a new delivery client targeting the addon on the given port.
    pub fn new(port: u16) -> Self {
        let base_url = format!("http://127.0.0.1:{port}");
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest::Client::builder should not fail");

        Self { http, base_url }
    }

    /// Resolve the addon port from environment, or use the default (18765).
    pub fn from_env() -> Self {
        let port = std::env::var("ANKITOV_CAPSULE_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CAPSULE_PORT);
        Self::new(port)
    }

    /// Resolve the delivery endpoint for a specific student (T-182).
    ///
    /// Multi-student/fleet delivery needs one addon endpoint per student;
    /// `from_env()` hardcodes a single `127.0.0.1:$PORT` co-located addon, so
    /// every student's capsule would land on the same container. Resolution
    /// order (see module docs): exact name match in
    /// `ANKITOV_CAPSULE_ENDPOINTS` → single bare URL → legacy port env/default.
    /// Unmatched students with a populated registry are logged loudly so
    /// operators catch missing registry entries.
    pub fn for_student(profile_name: &str) -> Self {
        let registry = parse_endpoint_registry(
            std::env::var("ANKITOV_CAPSULE_ENDPOINTS").ok().as_deref(),
        );
        match registry.as_slice() {
            [] => Self::from_env(),
            [(name, url)] if name.is_empty() => Self::from_base_url(url), // bare URL: any student
            entries => {
                if let Some((_, url)) = entries
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(profile_name))
                {
                    Self::from_base_url(url)
                } else {
                    tracing::warn!(
                        "ANKITOV_CAPSULE_ENDPOINTS has {} entries but no match for student {:?} — falling back to legacy single-addon endpoint",
                        entries.len(),
                        profile_name
                    );
                    Self::from_env()
                }
            }
        }
    }

    /// Build a client against a full base URL (used by the registry path).
    pub fn from_base_url(base_url: &str) -> Self {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest::Client::builder should not fail");
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Deliver a capsule to the addon and start a review session.
    ///
    /// Sends `POST /session` with the capsule metadata. The addon will:
    /// 1. Tag all card_ids with `ankitov-capsule-<session_uuid>`
    /// 2. Create a filtered deck "AnkiTov Capsule" with those cards
    /// 3. Open the reviewer in random order (interleaved)
    ///
    /// Returns the addon's response, which includes the session status.
    pub async fn start_session(
        &self,
        session_uuid: &str,
        card_ids: &[i64],
        capsule_size: usize,
        track_profile_name: &str,
    ) -> Result<StartSessionResponse, DeliveryError> {
        let url = format!("{}/session", self.base_url);

        let payload = StartSessionRequest {
            session_uuid: session_uuid.to_string(),
            card_ids: card_ids.to_vec(),
            capsule_size,
            track_profile_name: track_profile_name.to_string(),
        };

        let resp = self
            .http
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    DeliveryError::Unreachable {
                        url: url.clone(),
                        source: e,
                    }
                } else {
                    DeliveryError::Http(e)
                }
            })?;

        if !resp.status().is_success() {
            let status_code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DeliveryError::AddonError(format!(
                "HTTP {status_code}: {body}"
            )));
        }

        let response: StartSessionResponse = resp.json().await?;

        match response.status.as_str() {
            "started" | "replaced" | "empty" => Ok(response),
            "error" => Err(DeliveryError::AddonError(
                format!(
                    "addon rejected session {}: {:?}",
                    response.session_uuid,
                    response.status
                )
            )),
            other => Err(DeliveryError::UnexpectedStatus(other.to_string())),
        }
    }

    /// Query the addon for the current session status.
    ///
    /// Returns information about the active session: how many cards have been
    /// reviewed, the current state (IDLE / TAGGING / REVIEWING / CLEANUP), and
    /// the session UUID if one is active.
    pub async fn get_status(&self) -> Result<SessionStatus, DeliveryError> {
        let url = format!("{}/status", self.base_url);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    DeliveryError::Unreachable {
                        url: url.clone(),
                        source: e,
                    }
                } else {
                    DeliveryError::Http(e)
                }
            })?;

        let status: SessionStatus = resp.json().await?;
        Ok(status)
    }

    /// Cancel the active session on the addon.
    ///
    /// Sends `DELETE /session`. The addon will remove the filtered deck,
    /// untag the cards, and reset to IDLE state.
    pub async fn cancel_session(&self) -> Result<serde_json::Value, DeliveryError> {
        let url = format!("{}/session", self.base_url);

        let resp = self
            .http
            .delete(&url)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    DeliveryError::Unreachable {
                        url: url.clone(),
                        source: e,
                    }
                } else {
                    DeliveryError::Http(e)
                }
            })?;

        let body: serde_json::Value = resp.json().await?;
        Ok(body)
    }

    /// Check if the addon is reachable (health check).
    ///
    /// Returns `true` if the addon responds to a status query.
    pub async fn health_check(&self) -> bool {
        self.get_status().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registry tests mutate process-global env vars — serialize them.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_client_construction() {
        let client = CapsuleDelivery::new(18765);
        assert!(client.base_url.contains("18765"));
    }

    #[test]
    fn test_client_from_env_default() {
        // When ANKITOV_CAPSULE_PORT is not set, should default to 18765
        std::env::remove_var("ANKITOV_CAPSULE_PORT");
        let client = CapsuleDelivery::from_env();
        assert!(client.base_url.contains("18765"));
    }

    // ── T-182: per-student endpoint registry ──

    #[test]
    fn test_registry_parse_pairs() {
        let reg = parse_endpoint_registry(Some(
            "a=http://host:18701, b=http://host:18702 ,c=http://host:18703",
        ));
        assert_eq!(
            reg,
            vec![
                ("a".into(), "http://host:18701".into()),
                ("b".into(), "http://host:18702".into()),
                ("c".into(), "http://host:18703".into()),
            ]
        );
    }

    #[test]
    fn test_registry_parse_bare_url() {
        let reg = parse_endpoint_registry(Some("http://host.docker.internal:18765"));
        assert_eq!(reg, vec![("".into(), "http://host.docker.internal:18765".into())]);
    }

    #[test]
    fn test_registry_parse_none_and_garbage() {
        assert!(parse_endpoint_registry(None).is_empty());
        // empty segments are dropped
        assert!(parse_endpoint_registry(Some(",,")).is_empty());
    }

    #[test]
    fn test_for_student_exact_match() {
        // env vars are process-global; serialize registry tests with a lock
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ANKITOV_CAPSULE_PORT");
        std::env::set_var(
            "ANKITOV_CAPSULE_ENDPOINTS",
            "army-student-1=http://host.docker.internal:18701,army-student-2=http://host.docker.internal:18702",
        );
        let c = CapsuleDelivery::for_student("army-student-2");
        assert_eq!(c.base_url, "http://host.docker.internal:18702");
        // case-insensitive
        let c = CapsuleDelivery::for_student("ARMY-STUDENT-1");
        assert_eq!(c.base_url, "http://host.docker.internal:18701");
        std::env::remove_var("ANKITOV_CAPSULE_ENDPOINTS");
    }

    #[test]
    fn test_for_student_bare_url_applies_to_all() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ANKITOV_CAPSULE_ENDPOINTS", "http://host.docker.internal:18765");
        let c = CapsuleDelivery::for_student("anyone");
        assert_eq!(c.base_url, "http://host.docker.internal:18765");
        std::env::remove_var("ANKITOV_CAPSULE_ENDPOINTS");
    }

    #[test]
    fn test_for_student_unmatched_falls_back_to_legacy_port() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ANKITOV_CAPSULE_PORT", "18999");
        std::env::set_var("ANKITOV_CAPSULE_ENDPOINTS", "other=http://host:18701");
        let c = CapsuleDelivery::for_student("army-student-1");
        assert!(c.base_url.ends_with(":18999"));
        std::env::remove_var("ANKITOV_CAPSULE_ENDPOINTS");
        std::env::remove_var("ANKITOV_CAPSULE_PORT");
    }

    #[test]
    fn test_for_student_no_registry_uses_port_env() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("ANKITOV_CAPSULE_PORT", "18888");
        std::env::remove_var("ANKITOV_CAPSULE_ENDPOINTS");
        let c = CapsuleDelivery::for_student("army-student-1");
        assert!(c.base_url.ends_with(":18888"));
        std::env::remove_var("ANKITOV_CAPSULE_PORT");
    }
}