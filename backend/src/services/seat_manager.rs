//! SeatManager — one isolated real-Anki seat per practicing student.
//!
//! ## Product Law (owner directive, 2026-11-18)
//!
//! Practice happens in REAL Anki streamed via Selkies — never in
//! browser-rendered HTML cards. This service is the productization of the
//! Testing Army architecture: the backend spawns a `bwrap` sandbox per
//! active student, each running Anki + Selkies from a shared read-only
//! `seat-rootfs`, and hands the student's browser the Selkies stream URL.
//!
//! ## Why bwrap (decision record, 2026-11-19/20)
//!
//! * Shared page cache: every seat `--ro-bind`s the SAME rootfs, so
//!   QtWebEngine/Python/Qt pages are loaded into physical RAM once and
//!   shared across all seats (~2.2× marginal-RAM reduction vs per-seat
//!   container images; measured numbers pending the ub3090 load benchmark).
//! * `--dev-bind-try` gives automatic GPU passthrough when present and a
//!   silent llvmpipe/CPU-encode fallback when not — one code path for both.
//! * `--die-with-parent` + `--unshare-pid` guarantees the whole seat
//!   process tree dies with the backend handle (no zombie Anki seats).
//! * Runs identically on bare metal (.deb/.rpm hosts), inside Docker
//!   (`--ro-bind / /`), and — never — inside Flatpak (dropped 2026-11-21).
//!
//! ## Configuration
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `ANKITOV_SEAT_ROOTFS` | `/srv/ankitov-seat/rootfs` | Read-only seat rootfs (see `stream/seat-rootfs/`) |
//! | `ANKITOV_SEAT_BWRAP` | `bwrap` | bubblewrap binary |
//! | `ANKITOV_SEAT_BASE_PORT` | `8100` | First Selkies stream port |
//! | `ANKITOV_SEAT_MAX` | `64` | Concurrent seat cap |
//! | `ANKITOV_SEAT_IDLE_SECS` | `300` | Idle seconds before a seat is reaped |
//! | `ANKITOV_BACKEND_URL` | `http://127.0.0.1:5150` | Told to the seat for telemetry |
//!
//! The seat entry point (`/stream/start-seat.sh` inside the rootfs) starts
//! labwc + Selkies bound to `ANKITOV_SEAT_PORT` and launches Anki in kiosk
//! mode with the session-driver add-on (see `addons/ankitov_session_driver/`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How often the lazy reaper runs (checked on every list/start call).
const REAP_CHECK: Duration = Duration::from_secs(30);

/// A running seat.
#[derive(Debug, Clone)]
pub struct SeatInfo {
    pub student_id: String,
    /// Selkies stream port (the browser connects here).
    pub port: u16,
    /// PID of the `bwrap` process (the whole seat tree dies with it).
    pub pid: u32,
    pub started_at: Instant,
    /// Whether the seat got GPU devices bound.
    pub gpu: bool,
}

impl SeatInfo {
    /// The URL the student's browser opens (Selkies WebRTC/WebSocket page).
    pub fn stream_url(&self) -> String {
        // Public surface: students reach seats through the reverse proxy
        // (Caddy) or the server's public address — never a loopback URL.
        let base = std::env::var("ANKITOV_PUBLIC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1".to_string());
        format!("{}/seat/{}", base.trim_end_matches('/'), self.port)
    }
}

#[derive(Debug)]
pub enum SeatError {
    NotConfigured(String),
    CapReached,
    AlreadyRunning,
    NotFound,
    Spawn(String),
    Stop(String),
}

impl std::fmt::Display for SeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(r) => write!(f, "seat runtime not configured: {r}"),
            Self::CapReached => write!(f, "seat cap reached — no free slot"),
            Self::AlreadyRunning => write!(f, "seat already running for this student"),
            Self::NotFound => write!(f, "no running seat for this student"),
            Self::Spawn(e) => write!(f, "failed to spawn seat: {e}"),
            Self::Stop(e) => write!(f, "failed to stop seat: {e}"),
        }
    }
}

struct SeatRecord {
    info: SeatInfo,
    last_activity: Instant,
}

pub struct SeatManager {
    rootfs: PathBuf,
    /// Writable per-seat scratch root (s6 service dirs; army pattern)
    runtime_root: PathBuf,
    bwrap: String,
    base_port: u16,
    max_seats: usize,
    idle_timeout: Duration,
    backend_url: String,
    seats: Mutex<HashMap<String, SeatRecord>>,
}

static MANAGER: OnceLock<SeatManager> = OnceLock::new();

impl SeatManager {
    /// Process-global manager (services are unstructured in loco; mirrors
    /// the `classroom::set_router` OnceLock pattern).
    pub fn global() -> &'static Self {
        MANAGER.get_or_init(Self::from_env)
    }

    fn from_env() -> Self {
        let env_u16 = |k: &str, d: u16| {
            std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
        };
        Self {
            rootfs: PathBuf::from(
                std::env::var("ANKITOV_SEAT_ROOTFS")
                    .unwrap_or_else(|_| "/srv/ankitov-seat/rootfs".to_string()),
            ),
            runtime_root: std::env::var("ANKITOV_SEAT_RUNTIME_DIR").map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/srv/ankitov-seat/runtime")),
            bwrap: std::env::var("ANKITOV_SEAT_BWRAP").unwrap_or_else(|_| "bwrap".to_string()),
            base_port: env_u16("ANKITOV_SEAT_BASE_PORT", 8100),
            max_seats: env_u16("ANKITOV_SEAT_MAX", 64) as usize,
            idle_timeout: Duration::from_secs(env_u16("ANKITOV_SEAT_IDLE_SECS", 300) as u64),
            backend_url: std::env::var("ANKITOV_BACKEND_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5150".to_string()),
            seats: Mutex::new(HashMap::new()),
        }
    }

    /// GPU passthrough is available when a DRM device node exists.
    pub fn gpu_available() -> bool {
        PathBuf::from("/dev/dri").exists()
    }

    /// Start (or return the already-running) seat for a student.
    pub async fn start_seat(&self, student_id: &str) -> Result<SeatInfo, SeatError> {
        let safe = sanitize_student_id(student_id)
            .ok_or_else(|| SeatError::NotConfigured(format!("invalid student id {student_id:?}")))?;

        self.reap_idle().await;

        {
            let seats = self.lock();
            if seats.contains_key(&safe) {
                return Err(SeatError::AlreadyRunning);
            }
            if seats.len() >= self.max_seats {
                return Err(SeatError::CapReached);
            }
        }

        // The seat mounts ONLY this student's collection — the tmpfs /data
        // mask below hides the server's data dir and every other student's
        // collection from inside the sandbox.
        let collection_dir = collection_dir_for(&safe);
        std::fs::create_dir_all(&collection_dir).map_err(|e| {
            SeatError::Spawn(format!("cannot create collection dir {collection_dir:?}: {e}"))
        })?;

        // Per-seat writable scratch: s6-supervise writes state NEXT TO the
        // service dir, so /etc/service/anki must be a writable bind (the
        // Testing Army mounted per-student svc-anki dirs for the same reason).
        let svc_anki_dir = self.runtime_root.join(&safe).join("svc-anki");
        std::fs::create_dir_all(&svc_anki_dir).map_err(|e| {
            SeatError::Spawn(format!("cannot create seat scratch {svc_anki_dir:?}: {e}"))
        })?;
        let svc_anki_template = self.rootfs.join("etc/service/anki/run");
        if svc_anki_template.exists() {
            std::fs::copy(&svc_anki_template, svc_anki_dir.join("run")).map_err(|e| {
                SeatError::Spawn(format!("cannot stage svc-anki run script: {e}"))
            })?;
        }

        if !self.rootfs.join("stream").join("start-seat.sh").exists() {
            return Err(SeatError::NotConfigured(format!(
                "seat rootfs missing at {} — build it with stream/seat-rootfs/build-rootfs.sh",
                self.rootfs.display()
            )));
        }

        let port = self.free_port(&self.lock());
        let gpu = Self::gpu_available();
        let mut cmd = self.build_bwrap_command(&safe, &collection_dir, &svc_anki_dir, port, gpu);

        let child = cmd.spawn().map_err(|e| SeatError::Spawn(format!("bwrap: {e}")))?;
        let pid = child.id();
        let info = SeatInfo {
            student_id: safe.clone(),
            port,
            pid,
            started_at: Instant::now(),
            gpu,
        };

        tracing::info!(
            "seat started: student={safe} port={port} pid={pid} gpu={gpu} rootfs={}",
            self.rootfs.display()
        );
        self.lock().insert(
            safe,
            SeatRecord {
                info: info.clone(),
                last_activity: Instant::now(),
            },
        );
        Ok(info)
    }

    /// Stop a student's seat. Returns true if a seat was stopped.
    pub fn stop_seat(&self, student_id: &str) -> Result<bool, SeatError> {
        let safe = sanitize_student_id(student_id).ok_or(SeatError::NotFound)?;
        let removed = {
            let mut seats = self.lock();
            seats.remove(&safe)
        };
        match removed {
            Some(rec) => {
                // SIGTERM the bwrap root; --die-with-parent tears the tree down.
                let kill = Command::new("kill")
                    .arg("-TERM")
                    .arg(rec.info.pid.to_string())
                    .output();
                match kill {
                    Ok(_) => {
                        tracing::info!("seat stopped: student={} pid={}", safe, rec.info.pid);
                        Ok(true)
                    }
                    Err(e) => Err(SeatError::Stop(format!("kill {}: {e}", rec.info.pid))),
                }
            }
            None => Ok(false),
        }
    }

    /// All live seats (reaps stale ones first).
    pub fn list(&self) -> Vec<SeatInfo> {
        let mut seats = self.lock();
        Self::reap_locked(&mut seats, self.idle_timeout);
        let mut out: Vec<SeatInfo> = seats.values().map(|r| r.info.clone()).collect();
        out.sort_by(|a, b| a.student_id.cmp(&b.student_id));
        out
    }

    /// Stop seats idle beyond the timeout. Returns how many were reaped.
    pub async fn reap_idle(&self) -> usize {
        let mut seats = self.lock();
        let stale: Vec<(String, u32)> = seats
            .iter()
            .filter(|(_, r)| r.last_activity.elapsed() > self.idle_timeout)
            .map(|(k, r)| (k.clone(), r.info.pid))
            .collect();
        let mut reaped = 0;
        for (id, pid) in stale {
            let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).output();
            seats.remove(&id);
            tracing::info!("seat reaped (idle): student={id} pid={pid}");
            reaped += 1;
        }
        reaped
    }

    /// Touch a seat on observed student activity (called by the telemetry
    /// ingest path) so active students are never reaped mid-session.
    pub fn touch(&self, student_id: &str) {
        if let Some(safe) = sanitize_student_id(student_id) {
            if let Some(rec) = self.lock().get_mut(&safe) {
                rec.last_activity = Instant::now();
            }
        }
    }

    // ── internals ─────────────────────────────────────────────────────────

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SeatRecord>> {
        // Poisoning is not a state we can meaningfully recover from a seat
        // map; into_inner keeps the manager alive if a thread panicked while
        // holding the lock (no unwrap in production paths).
        self.seats.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn free_port(&self, seats: &HashMap<String, SeatRecord>) -> u16 {
        let mut used: Vec<u16> = seats.values().map(|r| r.info.port).collect();
        used.sort_unstable();
        let mut candidate = self.base_port;
        for u in used {
            if candidate == u {
                candidate += 1;
            } else {
                break;
            }
        }
        candidate
    }

    /// The exact sandbox shape approved 2026-11-19/20 (see module docs).
    fn build_bwrap_command(
        &self,
        safe_id: &str,
        collection_dir: &std::path::Path,
        svc_anki_dir: &std::path::Path,
        port: u16,
        gpu: bool,
    ) -> Command {
        let mut cmd = Command::new(&self.bwrap);
        cmd.arg("--ro-bind").arg(&self.rootfs).arg("/")
            // Mask the host data dir BEFORE binding the student's collection:
            // inside the sandbox, ankitov.db and other students' folders do
            // not exist.
            .arg("--tmpfs").arg("/data")
            .arg("--tmpfs").arg("/tmp")
            // Chromium/QtWebEngine + PipeWire abort without a real /dev/shm
            .arg("--tmpfs").arg("/dev/shm")
            .arg("--proc").arg("/proc")
            .arg("--dev").arg("/dev")
            .arg("--ro-bind-try").arg("/sys").arg("/sys")
            // GPU when present; silent llvmpipe fallback when not.
            .arg("--dev-bind-try").arg("/dev/dri").arg("/dev/dri");
        if gpu {
            for dev in ["/dev/nvidia0", "/dev/nvidiactl", "/dev/nvidia-modeset"] {
                cmd.arg("--dev-bind-try").arg(dev).arg(dev);
            }
        }
        cmd.arg("--bind")
            .arg(collection_dir)
            .arg("/home/ubuntu/.local/share/Anki2")
            .arg("--unshare-pid")
            .arg("--unshare-ipc")
            .arg("--unshare-uts")
            .arg("--die-with-parent");
        cmd.arg("--bind").arg(svc_anki_dir).arg("/etc/service/anki")
            // The wrapper copies the RO template services into the writable
            // tmpfs (s6-supervise needs writable service dirs).
            .arg("--ro-bind")
            .arg(self.rootfs.join("etc/service"))
            .arg("/etc/service-template")
            // Seat entry point (delegates to the image s6 init).
            .arg("/stream/start-seat.sh")
            .env("ANKITOV_SEAT_PORT", port.to_string())
            .env("ANKITOV_SEAT_STUDENT", safe_id)
            .env("ANKITOV_BACKEND_URL", &self.backend_url)
            // QtWebEngine needs a writable cache inside the sandbox.
            .env("XDG_CACHE_HOME", format!("/tmp/ankitov-seat-{safe_id}/cache"));
        // NOTE: QT_QPA_PLATFORM is NOT forced — the seat entrypoint decides
        // Wayland vs X11 (Xvfb) from the GPU/compositor probe (army parity).
        cmd
    }

    fn reap_locked(seats: &mut HashMap<String, SeatRecord>, idle_timeout: Duration) {
        let now = Instant::now();
        let stale: Vec<String> = seats
            .iter()
            .filter(|(_, r)| {
                now.duration_since(r.last_activity) > idle_timeout.max(REAP_CHECK)
                    || (now.duration_since(r.last_activity) > REAP_CHECK && !pid_alive(r.info.pid))
            })
            .map(|(k, _)| k.clone())
            .collect();
        for id in stale {
            if let Some(rec) = seats.remove(&id) {
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(rec.info.pid.to_string())
                    .output();
                tracing::info!("seat reaped: student={id} pid={}", rec.info.pid);
            }
        }
    }
}

fn pid_alive(pid: u32) -> bool {
    // /proc is authoritative on Linux; on other targets assume alive (the
    // idle timeout still reaps).
    PathBuf::from(format!("/proc/{pid}")).exists()
}

/// Seat collection dir — mirrors `collection_store`'s layout
/// ({collection_root}/{student_id}) without importing its private sanitizer.
fn collection_dir_for(safe_id: &str) -> PathBuf {
    let root = std::env::var("ANKITOV_COLLECTION_ROOT")
        .unwrap_or_else(|_| "./data/collections".to_string());
    PathBuf::from(root).join(safe_id)
}

/// Allow only [A-Za-z0-9._-], reject "." / ".." / traversal — the same class
/// of guard `collection_store::sanitize_student_id` applies before SQLite use.
fn sanitize_student_id(student_id: &str) -> Option<String> {
    if student_id.is_empty()
        || student_id.len() > 64
        || student_id == "."
        || student_id == ".."
        || !student_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return None;
    }
    Some(student_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert!(sanitize_student_id("../../etc").is_none());
        assert!(sanitize_student_id("").is_none());
        assert!(sanitize_student_id("..").is_none());
        assert!(sanitize_student_id("talmid-1").is_some());
        assert!(sanitize_student_id("a.b_c-9").is_some());
    }

    #[test]
    fn free_port_skips_used() {
        let mgr = SeatManager {
            rootfs: PathBuf::from("/srv/ankitov-seat/rootfs"),
            runtime_root: PathBuf::from("/srv/ankitov-seat/runtime"),
            bwrap: "bwrap".into(),
            base_port: 8100,
            max_seats: 64,
            idle_timeout: Duration::from_secs(300),
            backend_url: "http://127.0.0.1:5150".into(),
            seats: Mutex::new(HashMap::new()),
        };
        let mut seats = HashMap::new();
        let mk = |port: u16| SeatRecord {
            info: SeatInfo {
                student_id: "x".into(),
                port,
                pid: 1,
                started_at: Instant::now(),
                gpu: false,
            },
            last_activity: Instant::now(),
        };
        seats.insert("a".into(), mk(8100));
        seats.insert("b".into(), mk(8101));
        assert_eq!(mgr.free_port(&seats), 8102);
    }
}
