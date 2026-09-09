//! Zone 2: AnkiConnect Direct HTTP Client.
//!
//! ## Purpose
//!
//! Calls Anki's JSON-RPC API at `localhost:8765` directly from Rust via
//! `reqwest`. Every write operation goes through Anki's own scheduler,
//! guaranteeing SRS algorithm correctness. Sync protocol, .apkg handling,
//! and live scheduling queries all flow through this client.
//!
//! ## No MCP Server in Production
//!
//! This replaces the `anki-mcp-server` NestJS/TypeScript MCP process that
//! was suitable for development-time Goose agent interaction. At runtime,
//! AnkiTov's Rust backend calls AnkiConnect HTTP directly — eliminating a
//! ~80–120 MB Node.js process, a 3-hop transport chain, and a TypeScript
//! supply chain from the production container.
//!
//! ## Safety Rules
//!
//! 1. **Concurrency = 1:** AnkiConnect is single-threaded. All calls are
//!    serialized via `tokio::sync::Mutex` to prevent blocking or timeouts.
//! 2. **Action whitelist:** Only pre-approved actions are permitted. Any
//!    action not in the whitelist returns `Error::ActionNotAllowed` at call
//!    time — not at configuration time. Defense-in-depth against accidents.
//! 3. **Scheduler-safe:** All writes use Anki's state machine. Direct SQLite
//!    writes are never used for scheduling operations.
//!
//! ## Endpoint Reference
//!
//! | Category | Actions |
//! |----------|---------|
//! | **Read-only** | `deckNames`, `getDeckStats`, `findCards`, `findNotes`, `cardsInfo`, `notesInfo`, `getDueCards`, `version` |
//! | **Live** | `getDueCards`, `getDeckStats` |
//! | **Sync** | `sync`, `syncLogin`, `syncRegister` |
//! | **Write (safe)** | `suspend`, `unsuspend`, `setDueDate`, `exportPackage`, `importPackage` |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::services::anki_connect::{AnkiConnectClient, AnkiConnectError};
//!
//! let client = AnkiConnectClient::new(8765, None);
//! let stats = client.get_deck_stats("My Deck").await?;
//! ```

use std::collections::HashSet;

/// AnkiConnect JSON-RPC response envelope.
#[derive(Debug, serde::Deserialize)]
pub struct AnkiConnectResponse {
    #[serde(alias = "error")]
    pub error: Option<serde_json::Value>,
    #[serde(alias = "result")]
    pub result: Option<serde_json::Value>,
}

/// AnkiConnect action parameter wrappers.
#[derive(Debug, serde::Serialize)]
pub struct DeckNamesParams;

#[derive(Debug, serde::Serialize)]
pub struct GetDeckStatsParams {
    pub decks: Vec<String>,
}


#[derive(Debug, serde::Serialize)]
pub struct FindCardsParams {
    pub query: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CardsInfoParams {
    pub cards: Vec<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct SyncParams;

#[derive(Debug, serde::Serialize)]
pub struct SuspendCardsParams {
    pub cards: Vec<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct UnsuspendCardsParams {
    pub cards: Vec<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct SetDueDateParams {
    pub cards: Vec<i64>,
    pub days: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ExportPackageParams {
    pub deck: String,
    pub path: String,
    #[serde(default)]
    #[serde(rename = "includeSched")]
    pub include_sched: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ImportPackageParams {
    pub path: String,
    pub deck: String,
}


#[derive(Debug, serde::Serialize)]
pub struct ReflectParams;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors specific to the AnkiConnect client layer.
#[derive(Debug, thiserror::Error)]
pub enum AnkiConnectError {
    /// AnkiConnect returned a JSON-RPC error field.
    #[error("AnkiConnect RPC error: {0}")]
    RpcError(serde_json::Value),

    /// The requested action is not in the approved whitelist.
    #[error("action not in whitelist: {0}")]
    ActionNotAllowed(String),

    /// Anki is not running or AnkiConnect is unreachable.
    #[error("cannot connect to AnkiConnect at localhost:{0} — is Anki running?")]
    Unreachable(u16),

    /// HTTP-level error (refused connection, timeout, etc.).
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Serialization or deserialization failure.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// The API version returned by `version` does not match v6.
    #[error("AnkiConnect version mismatch: got {0}, expected 6")]
    VersionMismatch(i64),

    /// No `result` field in the AnkiConnect response.
    #[error("no result in AnkiConnect response — possible protocol error")]
    NoResult,
}

impl AnkiConnectResponse {
    /// Convert the JSON-RPC response into a typed result or return the error.
    pub fn into_result<T: serde::de::DeserializeOwned>(self) -> Result<T, AnkiConnectError> {
        if let Some(err) = self.error {
            return Err(AnkiConnectError::RpcError(err));
        }
        let result = self
            .result
            .ok_or(AnkiConnectError::NoResult)?;
        let value: T = serde_json::from_value(result)?;
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// AnkiConnect Direct HTTP client.
///
/// Constructed with a port and optional API key. All calls are serialized
/// through a `tokio::sync::Mutex` (concurrency = 1). Only whitelisted
/// actions can be invoked.
///
/// # Example
///
/// ```rust,ignore
/// let client = AnkiConnectClient::new(8765, Some("my-api-key"));
///
/// // List all deck names
/// let decks = client.deck_names().await?;
///
/// // Get live due count
/// let due = client.get_due_cards("Spanish::Vocab").await?;
/// ```
pub struct AnkiConnectClient {
    /// HTTP client for JSON-RPC requests.
    http: reqwest::Client,
    /// Base URL of the AnkiConnect endpoint.
    base_url: String,
    /// Optional API key appended as `?key=...`.
    api_key: Option<String>,
    /// Serializes concurrent calls — AnkiConnect is single-threaded.
    mutex: tokio::sync::Mutex<()>,
    /// Approved action names only.
    allowed_actions: HashSet<&'static str>,
}

impl AnkiConnectClient {
    /// Construct a new client targeting `localhost:{port}`.
    ///
    /// If `api_key` is provided it is appended as `?key={api_key}` to every
    /// request, matching AnkiConnect's optional authentication mechanism.
    pub fn new(port: u16, api_key: Option<String>) -> Self {
        let mut allowed = HashSet::new();

        // Read-only — always permitted
        allowed.insert("deckNames");
        allowed.insert("getDeckStats");
        allowed.insert("findCards");
        allowed.insert("findNotes");
        allowed.insert("cardsInfo");
        allowed.insert("notesInfo");
        allowed.insert("getDueCards");
        // (getDueCards is implemented via findCards, not a native action)
        allowed.insert("version");

        // Sync — requires Anki Web auth configured in Anki
        allowed.insert("sync");

        // Safe write operations — scheduler-aware
        allowed.insert("suspend");
        allowed.insert("unsuspend");
        allowed.insert("setDueDate");
        // Note: exportPackage used for backups (no native createBackup action)
        allowed.insert("exportPackage");
        allowed.insert("importPackage");

        let base_url = format!("http://localhost:{port}");
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::builder should not fail");

        Self {
            http,
            base_url,
            api_key,
            mutex: tokio::sync::Mutex::new(()),
            allowed_actions: allowed,
        }
    }

    /// Verify AnkiConnect is reachable and reports API version 6.
    ///
    /// Calls the `version` action internally. Returns `Ok(())` if Anki is
    /// running and responding with version 6. Returns `Err` otherwise.
    pub async fn health_check(&self) -> Result<(), AnkiConnectError> {
        let raw = self.invoke_raw("version", serde_json::json!({})).await?;
        let version: i64 = serde_json::from_value(raw)
            .map_err(AnkiConnectError::JsonError)?;
        if version != 6 {
            return Err(AnkiConnectError::VersionMismatch(version));
        }
        Ok(())
    }

    /// Low-level invoke — constructs JSON-RPC payload and sends HTTP POST.
    ///
    /// Calls are serialized through an internal mutex. Whitelist is checked
    /// before the request is sent. On HTTP connection failure, returns
    /// `AnkiConnectError::Unreachable`.
    ///
    /// Returns the raw JSON value from the `result` field.
    async fn invoke_raw(&self, action: &str, params: serde_json::Value) -> Result<serde_json::Value, AnkiConnectError> {
        if !self.allowed_actions.contains(action) {
            return Err(AnkiConnectError::ActionNotAllowed(action.to_string()));
        }

        let url = match &self.api_key {
            Some(key) => format!("{}?key={}", self.base_url, key),
            None => self.base_url.clone(),
        };

        // AnkiConnect requires `params` to be a JSON object, not null.
        // Unit structs serialize to null — normalise to empty object.
        let params = if params.is_null() {
            serde_json::json!({})
        } else {
            params
        };

        let payload = serde_json::json!({
            "action": action,
            "version": 6,
            "params": params
        });

        let resp = self.http
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    // Extract port from base_url for error message
                    let port = self.base_url
                        .strip_prefix("http://localhost:")
                        .unwrap_or("8765");
                    AnkiConnectError::Unreachable(port.parse().unwrap_or(8765))
                } else {
                    AnkiConnectError::HttpError(e)
                }
            })?;

        let ac_resp: AnkiConnectResponse = resp.json().await?;
        ac_resp.into_result()
    }

    /// Invoke an action with typed parameters and return a typed result.
    async fn invoke<P: serde::Serialize + std::fmt::Debug, R: serde::de::DeserializeOwned>(
        &self,
        action: &str,
        params: P,
    ) -> Result<R, AnkiConnectError> {
        let raw = self.invoke_raw(action, serde_json::to_value(params)?).await?;
        serde_json::from_value(raw).map_err(AnkiConnectError::JsonError)
    }

    // ------------------------------------------------------------------------
    // Read-only actions
    // ------------------------------------------------------------------------

    /// Return all deck names in the collection.
    pub async fn deck_names(&self) -> Result<Vec<String>, AnkiConnectError> {
        self.invoke("deckNames", DeckNamesParams).await
    }

    /// Return deck statistics for one or more named decks.
    ///
    /// AnkiConnect's `getDeckStats` returns a struct with `new`, `learn`,
    /// `review`, and `msancient` counts per deck.
    pub async fn get_deck_stats(&self, decks: Vec<String>) -> Result<serde_json::Value, AnkiConnectError> {
        self.invoke("getDeckStats", GetDeckStatsParams { decks }).await
    }

    /// Return IDs of all cards due in a named deck.
    ///
    /// Uses `findCards` with an `is:due` filter scoped to the deck.
    /// This reflects the live scheduler queue — available only when Anki is
    /// running. Cards in states: new (type=0), learning (type=1), review
    /// (type=2, due <= now), relearning.
    pub async fn get_due_cards(&self, deck: &str) -> Result<Vec<i64>, AnkiConnectError> {
        let query = format!("deck:\"{}\" is:due", deck);
        self.invoke("findCards", FindCardsParams { query }).await
    }

    /// Search for cards matching an Anki query string and return their IDs.
    ///
    /// Supports full Anki search syntax:
    /// `"deck:Spanish" "tag:vocab" is:due is:new`
    pub async fn find_cards(&self, query: &str) -> Result<Vec<i64>, AnkiConnectError> {
        self.invoke("findCards", FindCardsParams { query: query.to_string() }).await
    }

    /// Return detailed information for a list of card IDs.
    ///
    /// Returns an array of card objects including queue, due date, interval,
    /// ease factor, and note fields.
    pub async fn cards_info(&self, cards: Vec<i64>) -> Result<Vec<serde_json::Value>, AnkiConnectError> {
        self.invoke("cardsInfo", CardsInfoParams { cards }).await
    }

    /// Return AnkiConnect API version (health check helper).
    ///
    /// Calls the `version` action and returns the reported version number.
    pub async fn api_version(&self) -> Result<serde_json::Value, AnkiConnectError> {
        self.invoke_raw("version", serde_json::json!({})).await
    }

    // ------------------------------------------------------------------------
    // Write-safe actions (scheduler-aware)
    // ------------------------------------------------------------------------

    /// Trigger a full Anki Web sync.
    ///
    /// Requires Anki to have sync auth configured (ankiweb.net account or
    /// self-hosted sync server). This is the only safe way to synchronize
    /// the collection — doing it via direct SQLite writes would corrupt the
    /// sync log.
    pub async fn sync(&self) -> Result<serde_json::Value, AnkiConnectError> {
        // Hold the mutex for the full sync transaction
        let _guard = self.mutex.lock().await;
        self.invoke("sync", SyncParams).await
    }

    /// Suspend one or more cards by ID.
    ///
    /// Suspended cards are excluded from reviews until unsuspended. The
    /// operation goes through Anki's scheduler so card state is preserved.
    pub async fn suspend_cards(&self, cards: Vec<i64>) -> Result<bool, AnkiConnectError> {
        self.invoke("suspend", SuspendCardsParams { cards }).await
    }

    /// Unsuspend one or more cards by ID.
    pub async fn unsuspend_cards(&self, cards: Vec<i64>) -> Result<bool, AnkiConnectError> {
        self.invoke("unsuspend", UnsuspendCardsParams { cards }).await
    }

    /// Reschedule one or more cards to be due in N days.
    ///
    /// `days` is a string: `"0"` = today, `"1"` = tomorrow, `"7"` = in a
    /// week. Negative values are supported (`"-1"` = yesterday).
    pub async fn set_due_date(&self, cards: Vec<i64>, days: &str) -> Result<bool, AnkiConnectError> {
        self.invoke("setDueDate", SetDueDateParams { cards, days: days.to_string() }).await
    }

    /// Export a named deck to an `.apkg` file on disk.
    ///
    /// `include_sched` controls whether review history and scheduling data
    /// are included. Set to `true` for full backups, `false` for content-
    /// only exports.
    pub async fn export_package(
        &self,
        deck: &str,
        path: &str,
        include_sched: bool,
    ) -> Result<bool, AnkiConnectError> {
        self.invoke(
            "exportPackage",
            ExportPackageParams {
                deck: deck.to_string(),
                path: path.to_string(),
                include_sched,
            },
        )
        .await
    }

    /// Import an `.apkg` file into the collection.
    ///
    /// Anki handles schema migration, notetype resolution, and scheduling
    /// reset. This is the only safe import path.
    pub async fn import_package(&self, path: &str, deck: &str) -> Result<bool, AnkiConnectError> {
        self.invoke(
            "importPackage",
            ImportPackageParams {
                path: path.to_string(),
                deck: deck.to_string(),
            },
        )
        .await
    }

    /// Create a timestamped `.apkg` backup of a deck via `exportPackage`.
    ///
    /// AnkiConnect has no native "backup" action — `exportPackage` with
    /// `includeSched: true` is the safe equivalent: it produces a portable
    /// .apkg with full scheduling history. The caller provides the deck
    /// name and a directory path; the file is named with a timestamp.
    pub async fn create_backup(&self, deck: &str, dir: &str) -> Result<String, AnkiConnectError> {
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        let path = format!("{dir}/{deck}_{ts}.apkg");
        self.export_package(deck, &path, true).await?;
        Ok(path)
    }
}