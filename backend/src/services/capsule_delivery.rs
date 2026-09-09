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
//! | `ANKITOV_CAPSULE_PORT` | `18765` | Addon HTTP listener port |

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
}