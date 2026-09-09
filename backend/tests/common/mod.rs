//! AnkiTov Test Harness — shared integration-test infrastructure.
//!
//! ## Purpose
//!
//! Provides the building blocks for end-to-end tests that require a running
//! headless Anki instance with AnkiConnect.  Before this harness existed,
//! every integration test that touched the Zone 2 AnkiConnect client was a
//! manual collaboration point — someone had to launch Anki, select the
//! right profile, and keep it running.  The harness automates all of that.
//!
//! ## What You Get
//!
//! | Component | What it does |
//! |---|---|
//! | [`AnkiSession`] | Launches Anki headless, polls AnkiConnect, kills on drop |
//! | [`TempAnkiProfile`] | Copies a playground profile + addons to a temp dir |
//! | [`playground_path()`] | Returns the canonical Anki playground dir |
//! | [`playground_profiles()`] | Lists all available playground profile names |
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use crate::common::{AnkiSession, playground_path};
//!
//! let session = AnkiSession::with_playground_profile("Maya Chen")
//!     .await
//!     .expect("launch Anki");
//!
//! let client = session.client();
//! let decks = client.deck_names().await.expect("deck names");
//! assert!(!decks.is_empty());
//!
//! // session drops here → Anki process is killed
//! ```
//!
//! ## Prerequisites
//!
//! - macOS or Linux with Anki installed (`/Applications/Anki.app` or `anki` in PATH)
//! - The playground at `$ANKIPLAYGROUND_PATH` or `AnkiTov/AnkiPlayGround/`
//!   must contain:
//!     - `addons21/2055492159/` — AnkiConnect addon
//!     - `<ProfileName>/collection.anki2` — at least one seeded profile

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use backend::services::anki_connect::AnkiConnectClient;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default port for AnkiConnect in tests (can differ from production 8765 to
/// avoid collisions with a developer's own running Anki).
const ANKICONNECT_PORT: u16 = 8765;

/// How long to wait for AnkiConnect to respond after launching Anki.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Interval between AnkiConnect polls.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------------------
// AnkiSession — manages a headless Anki process
// ---------------------------------------------------------------------------

/// A running headless Anki instance with AnkiConnect available.
///
/// Created via [`AnkiSession::launch`] or [`AnkiSession::with_playground_profile`].
/// When the session is dropped, the Anki process is killed and (if a temporary
/// profile was used) the temp directory is cleaned up.
pub struct AnkiSession {
    /// The Anki child process — killed on drop.
    process: Option<Child>,
    /// The base directory passed to Anki's `-b` flag.
    #[allow(dead_code)]
    base_dir: PathBuf,
    /// The AnkiConnect port this session is using.
    port: u16,
    /// If `Some`, this directory will be deleted on drop (temp profile).
    #[allow(dead_code)]
    cleanup_dir: Option<tempfile::TempDir>,
}

impl AnkiSession {
    // ── constructors ────────────────────────────────────────────────

    /// Launch Anki headless using an existing playground profile (read-only).
    ///
    /// Uses the playground's own `addons21/` directory and the named profile.
    /// The profile is NOT copied — use this for read-only tests.
    pub async fn with_playground_profile(profile_name: &str) -> Result<Self, String> {
        let base = playground_path()
            .ok_or_else(|| "ANKIPLAYGROUND_PATH not set and playground not found".to_string())?;

        if !base.join(profile_name).is_dir() {
            return Err(format!("profile '{}' not found in playground", profile_name));
        }

        Self::launch(&base, profile_name, port_from_env()).await
    }

    /// Launch Anki headless with a copied temporary profile (safe for writes).
    ///
    /// Copies the named playground profile + `addons21/` into a temp directory,
    /// so tests can modify the collection without affecting the canonical
    /// playground data.
    pub async fn with_temp_profile(profile_name: &str) -> Result<Self, String> {
        let temp_profile = TempAnkiProfile::from_playground(profile_name)
            .map_err(|e| format!("failed to create temp profile: {e}"))?;

        let base = temp_profile.base_dir().to_path_buf();

        let mut session = Self::launch(&base, profile_name, port_from_env()).await?;

        // Transfer ownership of the TempDir to the session so the directory
        // stays alive until Anki is killed (see Drop impl).
        session.cleanup_dir = Some(temp_profile.into_temp_dir());

        Ok(session)
    }

    /// Low-level launch: given a base directory and profile name, spawn Anki
    /// headless and wait for AnkiConnect to respond.
    async fn launch(base_dir: &Path, profile_name: &str, port: u16) -> Result<Self, String> {
        // 1. Check if Anki is already running on this port (fast path)
        if ankiconnect_alive(port).await {
            return Err(format!(
                "AnkiConnect is already running on port {port}. \
                 Stop it before running tests (killall AnkiMac || killall anki)."
            ));
        }

        // 2. Resolve the Anki binary
        let binary = anki_binary().ok_or("unsupported OS for headless Anki")?;

        // 3. Spawn the child process
        tracing::info!(
            "Launching Anki headless: {binary} -b {} -p \"{profile_name}\"",
            base_dir.display()
        );

        let child = Command::new(binary)
            .env("QT_QPA_PLATFORM", "offscreen")
            .arg("-b")
            .arg(base_dir)
            .arg("-p")
            .arg(profile_name)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn Anki: {e}"))?;

        // 4. Poll until AnkiConnect responds or timeout
        let start = Instant::now();
        while start.elapsed() < LAUNCH_TIMEOUT {
            tokio::time::sleep(POLL_INTERVAL).await;
            if ankiconnect_alive(port).await {
                let elapsed = start.elapsed();
                tracing::info!("AnkiConnect ready on :{port} after {elapsed:?}");

                // AnkiConnect may respond before the collection is fully loaded.
                // Wait for the collection to be available (deck_names succeeds).
                let collection_ready = wait_for_collection(port, LAUNCH_TIMEOUT - elapsed).await;
                if collection_ready {
                    return Ok(Self {
                        process: Some(child),
                        base_dir: base_dir.to_path_buf(),
                        port,
                        cleanup_dir: None,
                    });
                }
                // Collection didn't load — continue polling
                tracing::warn!("AnkiConnect alive but collection not loaded yet, retrying...");
            }
        }

        // Timeout — kill the process and report failure
        let _ = Command::new("kill").arg(child.id().to_string()).output();

        Err(format!(
            "AnkiConnect did not respond on :{port} within {}s",
            LAUNCH_TIMEOUT.as_secs()
        ))
    }

    // ── accessors ───────────────────────────────────────────────────

    /// Return a pre-configured `AnkiConnectClient` for this session.
    pub fn client(&self) -> AnkiConnectClient {
        AnkiConnectClient::new(self.port, None)
    }

    /// The AnkiConnect port this session is using.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The base directory Anki was launched with.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Check whether the Anki process is still alive.
    pub fn is_alive(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(None) => true,  // still running
                _ => false,
            }
        } else {
            false
        }
    }
}

impl Drop for AnkiSession {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.process {
            let pid = child.id();
            tracing::info!("Shutting down Anki (PID {pid})");

            // SIGTERM for graceful shutdown
            let _ = Command::new("kill").arg(pid.to_string()).output();

            // Brief wait, then SIGKILL as fallback
            std::thread::sleep(Duration::from_millis(500));
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
            let _ = child.wait();
        }
        // `cleanup_dir` (TempDir) drops here, deleting the temp dir if present
    }
}

// `AnkiSession` is `Send` because all its fields are `Send`:
// `Child` is Send on Unix, `PathBuf`/`u16` are always Send,
// `TempDir` is Send+Sync.  Rust auto-derives this — no manual
// unsafe impl needed.

// ---------------------------------------------------------------------------
// TempAnkiProfile — throwaway profile for write tests
// ---------------------------------------------------------------------------

/// A temporary Anki profile directory suitable for tests that modify the
/// collection (adding cards, changing schedules, etc.).
///
/// Creates a temp directory containing:
/// - `addons21/2055492159/` — copy of AnkiConnect
/// - `<profile_name>/` — copy of a playground profile
///
/// The directory is deleted when the `TempDir` is dropped.
pub struct TempAnkiProfile {
    dir: tempfile::TempDir,
    profile_name: String,
}

impl TempAnkiProfile {
    /// Copy a playground profile into a temp directory.
    pub fn from_playground(profile_name: &str) -> Result<Self, String> {
        let playground = playground_path()
            .ok_or_else(|| "ANKIPLAYGROUND_PATH not set and playground not found".to_string())?;

        let profile_src = playground.join(profile_name);
        if !profile_src.is_dir() {
            return Err(format!("profile '{}' not found", profile_name));
        }

        let addons_src = playground.join("addons21");
        if !addons_src.is_dir() {
            return Err("addons21/ directory not found in playground".to_string());
        }

        let dir = tempfile::TempDir::new().map_err(|e| format!("tempdir: {e}"))?;

        // Copy addons21
        copy_dir(&addons_src, &dir.path().join("addons21"))
            .map_err(|e| format!("copy addons21: {e}"))?;

        // Copy profile
        let profile_dst = dir.path().join(profile_name);
        copy_dir(&profile_src, &profile_dst)
            .map_err(|e| format!("copy profile '{profile_name}': {e}"))?;

        Ok(Self {
            dir,
            profile_name: profile_name.to_string(),
        })
    }

    /// The base directory to pass to Anki's `-b` flag.
    pub fn base_dir(&self) -> &Path {
        self.dir.path()
    }

    /// The profile name to pass to Anki's `-p` flag.
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    /// Path to this profile's collection.anki2.
    pub fn collection_path(&self) -> PathBuf {
        self.dir.path().join(&self.profile_name).join("collection.anki2")
    }

    /// Consume the `TempAnkiProfile` and return the underlying `TempDir`.
    ///
    /// This transfers ownership of the temporary directory to the caller,
    /// preventing the directory from being deleted.  Used by `AnkiSession`
    /// to keep the profile alive for the duration of a test.
    pub fn into_temp_dir(self) -> tempfile::TempDir {
        self.dir
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect the OS and return the path to the Anki binary.
fn anki_binary() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("/Applications/Anki.app/Contents/MacOS/Anki")
    } else if cfg!(target_os = "linux") {
        Some("anki")
    } else {
        None
    }
}

/// Wait for Anki's collection to be fully loaded.
///
/// After AnkiConnect responds with `version: 6`, Anki may still be loading
/// the collection (especially for large profiles).  This function polls
/// `deckNames` until it succeeds, confirming the collection is available.
async fn wait_for_collection(port: u16, timeout: Duration) -> bool {
    let url = format!("http://localhost:{port}");
    let body = serde_json::json!({
        "action": "deckNames",
        "version": 6,
        "params": {},
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    // deckNames returns an array on success, or an error object
                    if json.get("result").map(|r| r.is_array()).unwrap_or(false) {
                        return true;
                    }
                }
            }
            _ => continue,
        }
    }
    false
}

/// Check if AnkiConnect is responding on the given port.
async fn ankiconnect_alive(port: u16) -> bool {
    let url = format!("http://localhost:{port}");
    let body = serde_json::json!({
        "action": "version",
        "version": 6,
        "params": {},
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("result")
                    .and_then(|v| v.as_i64())
                    .map(|v| v == 6)
                    .unwrap_or(false)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Resolve ANKICONNECT_PORT from env, or return the default (8765).
fn port_from_env() -> u16 {
    std::env::var("ANKICONNECT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(ANKICONNECT_PORT)
}

/// Return the canonical playground path, checking env var first.
pub fn playground_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("ANKIPLAYGROUND_PATH") {
        let p = PathBuf::from(&dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    // Fallback: relative to the AnkiTov workspace root
    let candidates = [
        PathBuf::from("/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround"),
    ];
    for p in &candidates {
        if p.is_dir() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// List all available profile names in the playground.
pub fn playground_profiles() -> Vec<String> {
    let base = match playground_path() {
        Some(p) => p,
        None => return vec![],
    };

    let mut profiles: Vec<String> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Only include directories that contain a collection.anki2
            if entry.path().join("collection.anki2").exists() {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    profiles.sort();
    profiles
}

/// Recursively copy a directory.
fn copy_dir(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod harness_tests {
    use super::*;

    #[test]
    fn test_playground_path_resolves() {
        let path = playground_path();
        assert!(path.is_some(), "playground should be resolvable on dev machines");
        let path = path.unwrap();
        assert!(path.is_dir());
        assert!(path.join("addons21").is_dir(), "addons21 must exist");
    }

    #[test]
    fn test_playground_has_profiles() {
        let profiles = playground_profiles();
        assert!(
            !profiles.is_empty(),
            "playground should have at least one profile with collection.anki2"
        );
        // The playground should contain our known classroom profiles
        assert!(
            profiles.contains(&"Maya Chen".to_string()),
            "should contain Maya Chen"
        );
        assert!(
            profiles.contains(&"Aisha Patel".to_string()),
            "should contain Aisha Patel"
        );
    }

    #[test]
    fn test_temp_profile_creation() {
        let temp = TempAnkiProfile::from_playground("Maya Chen")
            .expect("create temp profile from Maya Chen");
        assert!(temp.base_dir().is_dir());
        assert!(temp.collection_path().exists());
        assert!(temp.base_dir().join("addons21").is_dir());
        // The temp dir is cleaned up when `temp` is dropped
    }

    #[test]
    fn test_temp_profile_nonexistent() {
        let result = TempAnkiProfile::from_playground("NonExistentProfile12345");
        assert!(result.is_err());
    }
}