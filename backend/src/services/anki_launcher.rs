//! Anki Launcher — headless Anki startup with AnkiConnect verification.
//!
//! ## Purpose
//!
//! Ensures Anki is running with AnkiConnect available before any Zone 2
//! operation. If AnkiConnect is not responding, launches Anki headless
//! using `QT_QPA_PLATFORM=offscreen` and polls until it's ready.
//!
//! ## Platform Support
//!
//! | OS | Binary | Command |
//! |----|--------|---------|
//! | macOS | `/Applications/Anki.app/Contents/MacOS/Anki` | `QT_QPA_PLATFORM=offscreen <bin> -b <base> &` |
//! | Linux | `anki` (in PATH) | `QT_QPA_PLATFORM=offscreen anki -b <base> &` |
//!
//! ## Configuration
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `ANKI_BASE_DIR` | — (required for auto-launch) | Base folder passed to `-b` flag |
//! | `ANKI_PROFILE` | — (optional) | Profile name for `-p` flag |
//! | `ANKICONNECT_PORT` | `8765` | Port to poll for AnkiConnect |
//!
//! See: `specs/headless-anki-launch.md`

use std::time::{Duration, Instant};

/// Maximum time to wait for AnkiConnect to respond after launching.
const POLL_TIMEOUT: Duration = Duration::from_secs(20);

/// Interval between AnkiConnect polls.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Result of an ensure_anki_running call.
#[derive(Debug, Clone)]
pub enum LaunchResult {
    /// Anki was already running — AnkiConnect responded on first check.
    AlreadyRunning,
    /// Anki was launched headless and AnkiConnect came up within timeout.
    Launched { elapsed: Duration },
    /// Anki was launched but AnkiConnect did not respond within timeout.
    Timeout { elapsed: Duration },
    /// Could not launch — missing configuration (e.g. ANKI_BASE_DIR not set).
    NotConfigured { reason: String },
}

impl LaunchResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, LaunchResult::AlreadyRunning | LaunchResult::Launched { .. })
    }
}

/// Detect the operating system and return the appropriate Anki binary path.
fn anki_binary() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("/Applications/Anki.app/Contents/MacOS/Anki")
    } else if cfg!(target_os = "linux") {
        Some("anki")
    } else {
        None
    }
}

/// Check if AnkiConnect is responding on the given port.
///
/// Sends a `version` action JSON-RPC request. Returns `true` if the
/// response contains `"result":6`.
async fn ankiconnect_alive(port: u16) -> bool {
    let url = format!("http://localhost:{port}");
    let body = serde_json::json!({
        "action": "version",
        "version": 6,
        "params": {},
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();

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

/// Ensure Anki is running with AnkiConnect available.
///
/// 1. If AnkiConnect is already responding → `AlreadyRunning`
/// 2. If `ANKI_BASE_DIR` is set → launch headless → poll → `Launched` or `Timeout`
/// 3. If `ANKI_BASE_DIR` is not set → `NotConfigured`
pub async fn ensure_anki_running(port: u16) -> LaunchResult {
    // Fast path: AnkiConnect is already up.
    if ankiconnect_alive(port).await {
        return LaunchResult::AlreadyRunning;
    }

    // Resolve base directory from env or runtime state.
    let base_dir = match resolve_base_dir() {
        Some(dir) => dir,
        None => {
            return LaunchResult::NotConfigured {
                reason: "ANKI_BASE_DIR not set and ANKIPLAYGROUND_PATH/ANKICOLLECTION_PATH not resolvable".to_string(),
            }
        }
    };

    let binary = match anki_binary() {
        Some(b) => b,
        None => {
            return LaunchResult::NotConfigured {
                reason: format!("unsupported OS for headless Anki launch"),
            }
        }
    };

    tracing::info!("AnkiConnect not responding — launching Anki headless");
    tracing::info!("  binary: {binary}");
    tracing::info!("  base_dir: {base_dir}");

    // Build the launch command.
    let mut cmd = std::process::Command::new(binary);
    cmd.env("QT_QPA_PLATFORM", "offscreen")
        .arg("-b")
        .arg(&base_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Optional profile selection.
    if let Ok(profile) = std::env::var("ANKI_PROFILE") {
        cmd.arg("-p").arg(&profile);
        tracing::info!("  profile: {profile}");
    }

    // Spawn the process (detached).
    match cmd.spawn() {
        Ok(child) => {
            tracing::info!("Anki launched (PID {}), polling AnkiConnect on :{port}", child.id());
        }
        Err(e) => {
            return LaunchResult::NotConfigured {
                reason: format!("failed to spawn Anki: {e}"),
            };
        }
    }

    // Poll AnkiConnect until it responds or timeout.
    let start = Instant::now();
    while start.elapsed() < POLL_TIMEOUT {
        tokio::time::sleep(POLL_INTERVAL).await;
        if ankiconnect_alive(port).await {
            let elapsed = start.elapsed();
            tracing::info!("AnkiConnect is up after {elapsed:?}");
            return LaunchResult::Launched { elapsed };
        }
    }

    let elapsed = start.elapsed();
    tracing::warn!("AnkiConnect did not respond within {elapsed:?}");
    LaunchResult::Timeout { elapsed }
}

/// Resolve the Anki base directory from environment variables.
///
/// Priority:
/// 1. `ANKI_BASE_DIR` — explicit base directory
/// 2. `ANKIPLAYGROUND_PATH` — playground directory
/// 3. `ANKICOLLECTION_PATH` — infer parent of the .anki2 file
fn resolve_base_dir() -> Option<String> {
    if let Ok(dir) = std::env::var("ANKI_BASE_DIR") {
        return Some(dir);
    }
    if let Ok(dir) = std::env::var("ANKIPLAYGROUND_PATH") {
        return Some(dir);
    }
    if let Ok(col_path) = std::env::var("ANKICOLLECTION_PATH") {
        let path = std::path::Path::new(&col_path);
        if let Some(parent) = path.parent() {
            return Some(parent.display().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_base_dir_explicit() {
        // ANKI_BASE_DIR should take priority when set.
        // (This test only runs meaningfully when the env var is set externally.)
        if let Ok(dir) = std::env::var("ANKI_BASE_DIR") {
            assert_eq!(resolve_base_dir(), Some(dir));
        }
    }

    #[test]
    fn test_anki_binary_returns_path() {
        // On supported platforms, this should always return Some.
        let bin = anki_binary();
        assert!(
            bin.is_some() || (!cfg!(target_os = "macos") && !cfg!(target_os = "linux")),
            "anki_binary() should return Some on macOS/Linux"
        );
    }

    #[test]
    fn test_launch_result_is_ok() {
        assert!(LaunchResult::AlreadyRunning.is_ok());
        assert!(LaunchResult::Launched { elapsed: Duration::from_secs(0) }.is_ok());
        assert!(!LaunchResult::Timeout { elapsed: Duration::from_secs(0) }.is_ok());
        assert!(!LaunchResult::NotConfigured { reason: "test".into() }.is_ok());
    }
}
