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
//! * `--die-with-parent` + `--unshare-pid` tears the seat process tree
//!   down with the sandbox handle — but it CANNOT survive the backend
//!   process being restarted around a running seat (T-220, found live
//!   2026-11-25: the Nov-23 deploy left seat 8100 orphaned and serving
//!   while every student was authz-locked out). So start_seat also
//!   records a `seat.pid` file, and every fresh manager boot reaps
//!   pidfile-documented orphans (`reconcile_orphans`).
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
//! | `ANKITOV_SEAT_RUNTIME_DIR` | `/srv/ankitov-seat/runtime` | Per-seat writable scratch root (s6 service dirs) + the `seat.pid` files the boot-time orphan reconcile scans (T-220) |
//! | `ANKITOV_BACKEND_URL` | `http://127.0.0.1:5150` | Told to the seat for telemetry |
//! | `ANKITOV_PUBLIC_URL` | `http://127.0.0.1` | Public base URL the browser-facing `stream_url` is built from (e.g. `https://practice.ankitov.com`); the reverse proxy terminates TLS and routes `/seat/<port>/` to the seat |
//! | `ANKITOV_SEAT_PASSWD` | random per seat | RESERVED for per-seat stream auth (product TODO). Selkies basic auth is DISABLED in seats — selkies refuses to boot with auth on and no password (EX_CONFIG 78), and the namespace + public proxy are the boundary |
//! | `ANKITOV_SEAT_PACKS` | _(unset = none)_ | Comma-separated pack entries: seed `.apkg` file names (resolved against `/decks` then `/stream/decks` in the rootfs) or uploaded persisted file names (staged by the backend under `runtime/<id>/packs` and re-anchored to `/packs/<name>`, T-234). Unset = honest empty state. |
//! | `ANKITOV_SEAT_PREVIEW_MAX` | `4` | Concurrent preview-tier seats (drill links, T-215) allowed on top of `ANKITOV_SEAT_MAX`. |
//!
//! The seat entry point (`/stream/start-seat.sh` inside the rootfs) starts
//! labwc + Selkies bound to `ANKITOV_SEAT_PORT` and launches Anki in kiosk
//! mode with the session-driver add-on (see `addons/ankitov_session_driver/`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::services::deck_paths;
/// How often the lazy reaper runs (checked on every list/start call).
const REAP_CHECK: Duration = Duration::from_secs(30);

/// Preview-tier idle budget (T-215): anonymous drill seats reap faster
/// than the manager default (300 s) so scratch collections don't linger.
const PREVIEW_IDLE: Duration = Duration::from_secs(120);

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
    /// Selkies basic-auth password (basic-auth user is always `ubuntu`).
    /// Operator-fixed via ANKITOV_SEAT_PASSWD, else random per seat —
    /// the teacher hands URL + credentials to the student (the automated
    /// student-facing flow is a follow-up).
    pub password: String,
    /// Per-seat stream token for proxy-level auth (owner decision
    /// 2026-11-23): Caddy `forward_auth` validates this token (cookie or
    /// query param) against the live seat table before proxying
    /// `/seat/<port>/`. Random per seat — one student's token must never
    /// open another student's seat.
    pub token: String,
    /// Kiosk level this seat was spawned with (0 practice-only / 1 guided /
    /// 2 full Anki) — resolved from `users.kiosk_level` at start.
    pub kiosk_level: i32,
    /// True for preview-tier seats (drill links, T-215): scratch collection
    /// under the preview root, wiped when the seat stops/reaps.
    pub preview: bool,
}
impl SeatInfo {
    /// The URL the student's browser opens (Selkies WebRTC/WebSocket page).
    pub fn stream_url(&self) -> String {
        // Public surface: students reach seats through the reverse proxy
        // (Caddy) or the server's public address — never a loopback URL.
        // Trailing slash is REQUIRED: Caddy routes `handle_path /seat/<port>/*`
        // (a slashless /seat/<port> falls through to the backend and 404s).
        // The stream token is appended by the caller (`?seat_token=…`) —
        // kept out of here so logs/URL displays don't carry the credential.
        let base = std::env::var("ANKITOV_PUBLIC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1".to_string());
        format!("{}/seat/{}/", base.trim_end_matches('/'), self.port)
    }
}

/// Seat tier (T-215 First-60-Seconds): what kind of seat a launch creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeatTier {
    /// Logged-in student seat: persistent collection, standard idle reap.
    #[default]
    Full,
    /// Anonymous drill/preview seat (T-215): scratch collection under the
    /// preview root wiped on stop/reap, kiosk forced to 0, short idle reap,
    /// its own concurrent cap (`ANKITOV_SEAT_PREVIEW_MAX`).
    Preview,
}

/// Per-launch seat configuration, resolved by the CALLER — SeatManager
/// itself stays DB-free (same principle as the kiosk-level handoff).
#[derive(Debug, Clone, Default)]
pub struct SeatLaunch {
    /// Kiosk level for the seat (0/1/2); forced to 0 for preview tier.
    pub kiosk_level: i32,
    /// Telemetry attribution email (session driver prefers it over
    /// "unknown").
    pub student_email: Option<String>,
    /// Per-seat deck override (`ANKITOV_SEAT_DECK`). `None` inherits the
    /// backend's environment — the pre-T-215 global fallback, so
    /// management-started seats keep their existing behavior.
    pub deck: Option<String>,
    /// Per-seat pack list (`ANKITOV_SEAT_PACKS`, comma-separated .apkg
    /// names). `None` inherits the backend's environment (global fallback).
    pub packs: Option<String>,
    /// Seat tier (see [`SeatTier`]); default `Full`.
    pub tier: SeatTier,
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
    /// Per-seat idle budget (preview seats reap at 120 s, full seats at
    /// the manager default).
    idle_timeout: Duration,
}

pub struct SeatManager {
    rootfs: PathBuf,
    /// Writable per-seat scratch root (s6 service dirs; army pattern)
    runtime_root: PathBuf,
    bwrap: String,
    base_port: u16,
    max_seats: usize,
    /// Concurrent preview-tier cap (drill seats; T-215).
    preview_max: usize,
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
        let runtime_root = std::env::var("ANKITOV_SEAT_RUNTIME_DIR").map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/srv/ankitov-seat/runtime"));
        let bwrap = std::env::var("ANKITOV_SEAT_BWRAP").unwrap_or_else(|_| "bwrap".to_string());
        // T-220: before the first seat action, reap sandboxes a backend
        // restart left orphaned (seat state is in-memory; --die-with-parent
        // protects the seat from ITS parent dying, not from the backend
        // being restarted around it). Only processes whose kernel command
        // name matches our bwrap binary are ever touched.
        let expected_comm = PathBuf::from(&bwrap)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "bwrap".to_string());
        let reaped_orphans = reconcile_orphans(&runtime_root, &expected_comm);
        if reaped_orphans > 0 {
            tracing::info!("seat orphan reconcile at boot: reaped {reaped_orphans} orphaned seat(s)");
        }
        Self {
            rootfs: PathBuf::from(
                std::env::var("ANKITOV_SEAT_ROOTFS")
                    .unwrap_or_else(|_| "/srv/ankitov-seat/rootfs".to_string()),
            ),
            runtime_root,
            bwrap,
            base_port: env_u16("ANKITOV_SEAT_BASE_PORT", 8100),
            max_seats: env_u16("ANKITOV_SEAT_MAX", 64) as usize,
            preview_max: env_u16("ANKITOV_SEAT_PREVIEW_MAX", 4) as usize,
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

    /// Rootfs readiness gate (T-237): both required trees present.
    ///
    /// Shared by [`Self::start_seat`] and the classroom-mode enable handler
    /// so a rootfs-less install fails LOUD — with the build command — before
    /// any student-facing surface that depends on seats is offered.
    pub fn rootfs_ready(&self) -> Result<(), SeatError> {
        if !self.rootfs.join("etc").join("service").exists() {
            return Err(SeatError::NotConfigured(format!(
                "seat rootfs missing etc/service at {} — build it with stream/seat-rootfs/build-rootfs.sh",
                self.rootfs.display()
            )));
        }
        if !self.rootfs.join("stream").join("start-seat.sh").exists() {
            return Err(SeatError::NotConfigured(format!(
                "seat rootfs missing at {} — build it with stream/seat-rootfs/build-rootfs.sh",
                self.rootfs.display()
            )));
        }
        Ok(())
    }

    /// Start (or return the already-running) seat for a student.
    ///
    /// All seat policy comes from the caller via [`SeatLaunch`] —
    /// SeatManager itself stays DB-free (same principle as the kiosk-level
    /// handoff, owner decision 2026-11-23). `None` deck/packs inherit the
    /// backend's environment (pre-T-215 global fallback; per-link values
    /// come from drill links).
    pub async fn start_seat(
        &self,
        student_id: &str,
        launch: &SeatLaunch,
    ) -> Result<SeatInfo, SeatError> {
        let safe = sanitize_student_id(student_id)
            .ok_or_else(|| SeatError::NotConfigured(format!("invalid student id {student_id:?}")))?;
        let preview = launch.tier == SeatTier::Preview;
        // Preview seats are anonymous drill clicks — the practice-only
        // kiosk (level 0) is the only level they may run (T-215).
        let kiosk_level = if preview { 0 } else { launch.kiosk_level };

        self.reap_idle().await;

        {
            let seats = self.lock();
            if seats.contains_key(&safe) {
                return Err(SeatError::AlreadyRunning);
            }
            if seats.len() >= self.max_seats {
                return Err(SeatError::CapReached);
            }
            // Preview cap is ON TOP of the global cap (T-215).
            if preview
                && seats.values().filter(|r| r.info.preview).count() >= self.preview_max
            {
                return Err(SeatError::CapReached);
            }
        }

        // The seat mounts ONLY this student's collection — the tmpfs /data
        // mask below hides the server's data dir and every other student's
        // collection from inside the sandbox.
        let collection_dir = collection_dir_for(&safe, preview);
        std::fs::create_dir_all(&collection_dir).map_err(|e| {
            SeatError::Spawn(format!("cannot create collection dir {collection_dir:?}: {e}"))
        })?;

        // Rootfs readiness gate (T-237): fail LOUD before any staging work.
        self.rootfs_ready()?;

        // Per-seat writable copy of the WHOLE /etc/service tree, bound
        // wholesale at /etc/service (live-proven 2026-11-23 on Hetzner).
        // s6-supervise writes supervise/ + event/ state INSIDE each service
        // dir, so neither a read-only bind nor a tmpfs + template-copy split
        // survives boot; a full writable copy is the shape that boots. Files
        // land owned by the server user (std::fs::copy does not preserve
        // ownership), which root-in-namespace can always write — no chown
        // pass needed. dbus is DROPPED: bwrap (0.11) cannot map arbitrary
        // gids, so dbus-daemon's setgid(messagebus) fails EPERM and
        // crash-loops the seat; audio rides pipewire directly, no bus needed.
        let svc_dir = self.runtime_root.join(&safe).join("etc-service");
        if svc_dir.exists() {
            std::fs::remove_dir_all(&svc_dir).map_err(|e| {
                SeatError::Spawn(format!("cannot clear seat scratch {svc_dir:?}: {e}"))
            })?;
        }
        std::fs::create_dir_all(&svc_dir).map_err(|e| {
            SeatError::Spawn(format!("cannot create seat scratch {svc_dir:?}: {e}"))
        })?;
        let svc_template = self.rootfs.join("etc/service");
        Self::copy_dir_all(&svc_template, &svc_dir).map_err(|e| {
            SeatError::Spawn(format!("cannot stage seat service tree: {e}"))
        })?;
        let _ = std::fs::remove_dir_all(svc_dir.join("dbus"));

        let port = self.free_port(&self.lock());
        let gpu = Self::gpu_available();
        let password = self.seat_password();
        let token = Self::random_secret(32);
        // T-234: staged uploaded packs land under runtime/<id>/packs and
        // are ro-bound at /packs; seed names pass through unchanged.
        let (packs, packs_dir) = self.stage_packs(&safe, launch.packs.as_deref())?;
        let mut cmd = self.build_bwrap_command(
            &safe,
            &collection_dir,
            &svc_dir,
            port,
            gpu,
            &password,
            kiosk_level,
            launch.student_email.as_deref(),
            launch.deck.as_deref(),
            packs.as_deref(),
            packs_dir.as_deref(),
        );
        let child = cmd.spawn().map_err(|e| SeatError::Spawn(format!("bwrap: {e}")))?;
        let pid = child.id();
        // T-220: record the spawn for the boot-time orphan reconcile —
        // written BEFORE the seat becomes visible so even a seat started
        // moments before a restart is findable. Best effort: the write
        // must never fail a seat that already spawned.
        write_pidfile(&self.runtime_root, &safe, pid, preview);
        let info = SeatInfo {
            student_id: safe.clone(),
            port,
            pid,
            started_at: Instant::now(),
            gpu,
            password,
            token,
            kiosk_level: kiosk_level.clamp(0, 2),
            preview,
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
                idle_timeout: if preview { PREVIEW_IDLE } else { self.idle_timeout },
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
                // Preview scratch is wiped whether or not the kill reported
                // success — the seat is leaving the table either way.
                wipe_preview_scratch(&rec.info, &self.runtime_root);
                // Drop the boot-reconcile pidfile (wiped with the whole
                // scratch dir for preview seats; explicit for full seats).
                let _ = std::fs::remove_file(pidfile_path(&self.runtime_root, &safe));
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
        Self::reap_locked(&mut seats, &self.runtime_root);
        let mut out: Vec<SeatInfo> = seats.values().map(|r| r.info.clone()).collect();
        out.sort_by(|a, b| a.student_id.cmp(&b.student_id));
        out
    }

    /// Stop seats idle beyond their own timeout (preview seats reap at
    /// `PREVIEW_IDLE`, full seats at `ANKITOV_SEAT_IDLE_SECS`). Returns how
    /// many were reaped.
    pub async fn reap_idle(&self) -> usize {
        let mut seats = self.lock();
        let stale: Vec<String> = seats
            .iter()
            .filter(|(_, r)| r.last_activity.elapsed() > r.idle_timeout)
            .map(|(k, _)| k.clone())
            .collect();
        let mut reaped = 0;
        for id in stale {
            let rec = seats.remove(&id);
            if let Some(rec) = rec {
                wipe_preview_scratch(&rec.info, &self.runtime_root);
                let _ = std::fs::remove_file(pidfile_path(&self.runtime_root, &id));
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(rec.info.pid.to_string())
                    .output();
                tracing::info!("seat reaped (idle): student={} pid={}", id, rec.info.pid);
                reaped += 1;
            }
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
        loop {
            // skip ports our own seats already hold
            if used.contains(&candidate) {
                candidate += 1;
                continue;
            }
            // skip FOREIGN listeners too (2026-11-22: headscale holds
            // 127.0.0.1:8080 on the seat host — a seat spawning there would
            // fight it). A probe bind is the only reliable check.
            match std::net::TcpListener::bind(("0.0.0.0", candidate)) {
                Ok(listener) => {
                    drop(listener);
                    return candidate;
                }
                Err(_) => {
                    tracing::warn!("seat port {} already in use by another process — skipping", candidate);
                    candidate += 1;
                }
            }
        }
    }

    /// Seat basic-auth password: operator-fixed via ANKITOV_SEAT_PASSWD
    /// (one shared classroom door code), else random per seat. Exposed to
    /// teacher/admin via the management API so the demo/class flow can hand
    /// URL + credentials to the student.
    fn seat_password(&self) -> String {
        if let Ok(p) = std::env::var("ANKITOV_SEAT_PASSWD") {
            if !p.is_empty() {
                return p;
            }
        }
        Self::random_secret(16)
    }

    /// Random alphanumeric secret (per-seat stream tokens, fallback
    /// basic-auth passwords). Always random per call — never operator-shared.
    fn random_secret(len: usize) -> String {
        use rand::Rng;
        rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(len)
            .map(char::from)
            .collect()
    }

    /// Resolve a launch's pack list for staging (T-234).
    ///
    /// Entries come from drill links and are either SEED pack names
    /// (resolved inside the rootfs against `/decks` then `/stream/decks`)
    /// or UPLOADED persisted file names (e.g. `a1b2c3d4-my-deck.apkg`)
    /// living in the backend's upload dir. Uploaded entries are copied
    /// per-seat under `runtime/<id>/packs/` and re-anchored to
    /// `/packs/<name>` so the sandbox sees them without binding the whole
    /// upload dir (which also holds decks not bound to this seat).
    ///
    /// Returns the rewritten comma list plus the staging dir to ro-bind at
    /// `/packs` (Some only when at least one entry was staged). Unknown
    /// names pass through untouched — the in-seat importer logs-and-skips
    /// missing packs (existing graceful degradation).
    fn stage_packs(
        &self,
        safe_id: &str,
        packs: Option<&str>,
    ) -> Result<(Option<String>, Option<std::path::PathBuf>), SeatError> {
        let Some(packs) = packs else {
            return Ok((None, None));
        };
        if packs.trim().is_empty() {
            return Ok((Some(packs.to_string()), None));
        }
        let mut staged: Vec<String> = Vec::new();
        let mut packs_dir: Option<std::path::PathBuf> = None;
        for name in packs.split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            // Absolute paths and anything that is not a bare `.apkg` file
            // name (operator-staged paths, `/packs/...` re-entries) pass
            // through untouched — the importer resolves them as-is.
            if !deck_paths::valid_package_name(name) {
                staged.push(name.to_string());
                continue;
            }
            let upload_file = deck_paths::upload_dir().join(name);
            if upload_file.is_file() {
                let dir = self.runtime_root.join(safe_id).join("packs");
                // A previous start for the same student may have staged a
                // different pack list — clear the scratch dir first.
                if dir.exists() {
                    std::fs::remove_dir_all(&dir).map_err(|e| {
                        SeatError::Spawn(format!("cannot clear pack scratch {dir:?}: {e}"))
                    })?;
                }
                std::fs::create_dir_all(&dir).map_err(|e| {
                    SeatError::Spawn(format!("cannot create pack staging dir {dir:?}: {e}"))
                })?;
                std::fs::copy(&upload_file, dir.join(name)).map_err(|e| {
                    SeatError::Spawn(format!("cannot stage uploaded pack {name:?}: {e}"))
                })?;
                packs_dir = Some(dir);
                staged.push(format!("/packs/{name}"));
            } else {
                // Not in the upload dir: a seed name (resolved in-seat) or
                // a name that no longer exists anywhere (logged in-seat).
                staged.push(name.to_string());
            }
        }
        Ok((Some(staged.join(",")), packs_dir))
    }

    /// The exact sandbox shape approved 2026-11-19/20 (see module docs).
    fn build_bwrap_command(        &self,
        safe_id: &str,
        collection_dir: &std::path::Path,
        svc_dir: &std::path::Path,
        port: u16,
        gpu: bool,
        password: &str,
        kiosk_level: i32,
        student_email: Option<&str>,
        deck: Option<&str>,
        packs: Option<&str>,
        packs_dir: Option<&std::path::Path>,
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
            // The session user's home must be writable (selkies menu dirs,
            // Anki profile scratch); the collection is bound INTO it below.
            .arg("--tmpfs").arg("/home/ubuntu")
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
        // The staged writable service tree is bound WHOLESALE at /etc/service
        // (live-proven shape — see start_seat staging comment). Staged
        // uploaded packs (T-234) mount read-only at /packs so the in-seat
        // importer resolves `/packs/<name>` entries without the sandbox
        // seeing the whole upload dir.
        cmd.arg("--bind")
            .arg(svc_dir)
            .arg("/etc/service");
        if let Some(dir) = packs_dir {
            cmd.arg("--ro-bind").arg(dir).arg("/packs");
        }
        // Seat entry point (delegates to the image s6 init).
        cmd.arg("/stream/start-seat.sh")
            .env("ANKITOV_SEAT_PORT", port.to_string())
            .env("ANKITOV_SEAT_STUDENT", safe_id)
            .env("ANKITOV_BACKEND_URL", &self.backend_url)
            // Selkies config refusals (EX_CONFIG 78) seen live 2026-11-23:
            // basic auth ON with no password env refuses to start (army
            // docker set PASSWD per container; seats inherit backend env),
            // and HTTPS ON makes the seat serve TLS where the reverse proxy
            // speaks plain HTTP (502 / curl 000). Both OFF: the namespace +
            // public proxy are the boundary; per-seat tokens are the TODO.
            .env("SELKIES_ENABLE_BASIC_AUTH", "false")
            .env("SELKIES_ENABLE_HTTPS", "false")
            // Websockets transport never dials TURN; keep the coturn service
            // out of the seat's way (live launcher parity).
            .env("SELKIES_ENABLE_INTERNAL_TURN", "false")
            // Seat kiosk (owner directive 2026-11-23): 0 = practice-only
            // (deck auto-chosen, reviewer auto-started, escapes bounced),
            // 1 = guided (deck opened, UI free), 2 = full Anki. The level
            // is PER-STUDENT (users.kiosk_level, resolved by the caller —
            // owner decision 2026-11-23); clamped defensively.
            .env(
                "ANKITOV_SEAT_KIOSK_LEVEL",
                kiosk_level.clamp(0, 2).to_string(),
            )
            // Telemetry attribution: the session driver prefers this over
            // "unknown" when posting card answers (session.py).
            .env(
                "ANKITOV_SEAT_EMAIL",
                student_email.unwrap_or(""),
            )
            // The session-driver addon listens on 127.0.0.1 inside the seat —
            // and seats SHARE the host network namespace (no --unshare-net),
            // so the default :18765 would collide across seats. Derive a
            // per-seat port from the stream port (+500: 8100 -> 8600).
            .env("ANKITOV_CAPSULE_PORT", (port + 500).to_string())
            // Reserved for per-seat stream auth (product TODO): selkies
            // ignores PASSWD while basic auth is disabled.
            .env("PASSWD", password)
            // The seat session runs as root inside bwrap (the OS sandbox is
            // the boundary), and QtWebEngine refuses its setuid sandbox as
            // root — Anki crash-loops on the zygote host check without this.
            // Set here, not in the rootfs entrypoint, so it ships with the
            // backend and never drifts from the deployed rootfs copy.
            .env("QTWEBENGINE_DISABLE_SANDBOX", "1")
            // QtWebEngine needs a writable cache inside the sandbox.
            .env("XDG_CACHE_HOME", format!("/tmp/ankitov-seat-{safe_id}/cache"));
        // Per-launch deck/packs binding (T-215): drill links pass the
        // link's values here; management-started seats pass `None`, which
        // inherits the backend's environment (the pre-T-215 global env
        // fallback — Command inherits the parent env by default).
        if let Some(deck) = deck {
            if !deck.is_empty() {
                cmd.env("ANKITOV_SEAT_DECK", deck);
            }
        }
        if let Some(packs) = packs {
            if !packs.is_empty() {
                cmd.env("ANKITOV_SEAT_PACKS", packs);
            }
        }
        // NOTE: QT_QPA_PLATFORM is NOT forced — the seat entrypoint decides
        // Wayland vs X11 (Xvfb) from the GPU/compositor probe (army parity).
        cmd
    }

    /// Recursive copy used to stage the per-seat /etc/service tree. Unlike
    /// `cp -a`, files created this way are OWNED BY THE SERVER USER — which
    /// is exactly what the seat needs: root-in-the-bwrap-namespace maps to
    /// the server user, and s6-supervise can write its state. Preserving
    /// foreign ownership (the army image ships runner-owned files) would
    /// recreate the "unable to mkdir .s6-svscan: Permission denied" DAC
    /// failure inside the unmapped user namespace.
    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let to = dst.join(entry.file_name());
            if ty.is_dir() {
                Self::copy_dir_all(&entry.path(), &to)?;
            } else if ty.is_symlink() {
                #[cfg(unix)]
                {
                    let target = std::fs::read_link(entry.path())?;
                    std::os::unix::fs::symlink(&target, &to)?;
                }
            } else {
                std::fs::copy(entry.path(), &to)?;
            }
        }
        Ok(())
    }

    fn reap_locked(
        seats: &mut HashMap<String, SeatRecord>,
        runtime_root: &std::path::Path,
    ) {
        let now = Instant::now();
        let stale: Vec<String> = seats
            .iter()
            .filter(|(_, r)| {
                now.duration_since(r.last_activity) > r.idle_timeout.max(REAP_CHECK)
                    || (now.duration_since(r.last_activity) > REAP_CHECK && !pid_alive(r.info.pid))
            })
            .map(|(k, _)| k.clone())
            .collect();
        for id in stale {
            if let Some(rec) = seats.remove(&id) {
                wipe_preview_scratch(&rec.info, runtime_root);
                let _ = std::fs::remove_file(pidfile_path(runtime_root, &id));
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
    // /proc is authoritative on Linux. Elsewhere (macOS dev) /proc does not
    // exist, so fall back to a kill(pid, 0) existence probe: rc 0 = can
    // signal, EPERM = exists but not ours — BOTH mean alive. A missing /proc
    // must never read as "dead", or the dead-pid reap branch would kill every
    // seat REAP_CHECK after boot on non-Linux hosts (T-215 test catch).
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

// ── T-220: boot-time orphan reconcile ────────────────────────────────────
//
// SeatManager's seat table is in-memory, so a backend restart leaves
// every running seat sandbox orphaned: still holding its port + RAM/GPU
// while the live seat table — and with it the stream-auth token↔port
// bindings — is empty. No student can re-enter an orphaned seat anyway
// (`/api/v1/seats/authz` validates against the LIVE table), so
// reap-then-fresh-start is the honest minimal fix (T-220, filed
// 2026-11-25; adoption cannot recover per-seat tokens, and sqlite seat
// persistence is deferred until the student-facing seat flow settles).
//
// The reconcile is pidfile-driven: start_seat records `{pid} {preview}`
// in `<runtime_root>/<student>/seat.pid`, and every fresh manager boot
// reaps what is PROVABLY still a seat sandbox — alive AND whose kernel
// command name matches our bwrap binary (pid-reuse guard; headscale
// holding 127.0.0.1:8080 on the seat host is the standing reminder that
// foreign pids must never be touched). Dirs without a pidfile are
// skipped: pre-T-220 orphans keep the documented manual recovery path.

/// How long the reconcile waits after SIGTERM before escalating to
/// SIGKILL (bwrap teardown is normally sub-100 ms).
const ORPHAN_TERM_WAIT: Duration = Duration::from_secs(2);

/// `{pid} {preview_flag}` — the seat.pid content start_seat records
/// (1 = preview-tier).
fn parse_pidfile(text: &str) -> Option<(u32, bool)> {
    let mut parts = text.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let preview = parts.next().is_some_and(|f| f == "1");
    Some((pid, preview))
}

/// `<runtime_root>/<student>/seat.pid` — the reconcile's only handle on a
/// spawned seat.
fn pidfile_path(runtime_root: &std::path::Path, safe_id: &str) -> PathBuf {
    runtime_root.join(safe_id).join("seat.pid")
}

/// Record a spawn in the per-seat runtime dir (T-220). Best effort: the
/// pidfile only powers boot-time reaping, and a failed write must NOT
/// fail a seat that already spawned — that would orphan exactly the
/// sandbox we were trying to account for.
fn write_pidfile(runtime_root: &std::path::Path, safe_id: &str, pid: u32, preview: bool) {
    let path = pidfile_path(runtime_root, safe_id);
    let write = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))
        .and_then(|_| std::fs::write(&path, format!("{pid} {}\n", preview as u8)));
    if let Err(e) = write {
        tracing::warn!("seat pidfile write failed {}: {e}", path.display());
    }
}

/// Kernel-reported command name for `pid` (best effort): Linux reads
/// /proc/<pid>/comm directly; elsewhere `ps -o comm=`. None = unknown.
fn process_comm(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|comm| comm.trim().to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Command::new("ps")
            .arg("-o")
            .arg("comm=")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|comm| !comm.is_empty())
    }
}

/// True when `pid` exists AND still runs the expected binary — the
/// pid-reuse guard: a pidfile can outlive its process, and the pid may
/// have been reused by an unrelated process (never touch one).
fn pid_matches(pid: u32, expected_comm: &str) -> bool {
    pid_alive(pid) && process_comm(pid).is_some_and(|comm| comm == expected_comm)
}

/// Poll `pid` up to `budget` so a normal shutdown is allowed to finish
/// before escalating to SIGKILL.
fn wait_for_death(pid: u32, budget: Duration) {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Reap orphaned seat sandboxes recorded by pidfiles under
/// `runtime_root` (see the block comment above). Returns how many were
/// reaped.
fn reconcile_orphans(runtime_root: &std::path::Path, expected_comm: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(runtime_root) else {
        return 0; // fresh host / no seat runtime yet — nothing to do
    };
    let mut reaped = 0;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|ty| ty.is_dir()) {
            continue;
        }
        let pidfile = entry.path().join("seat.pid");
        let Ok(text) = std::fs::read_to_string(&pidfile) else {
            // No pidfile: a pre-T-220 orphan (manual recovery path) or a
            // non-seat dir. Leave it alone.
            continue;
        };
        let safe = entry.file_name().to_string_lossy().into_owned();
        let Some((pid, preview)) = parse_pidfile(&text) else {
            // Corrupt pidfile — unusable by construction; drop the file so
            // a fresh start records a clean one.
            let _ = std::fs::remove_file(&pidfile);
            tracing::warn!("seat pidfile corrupt, dropped: {}", pidfile.display());
            continue;
        };
        if pid_matches(pid, expected_comm) {
            // Same shape as stop_seat: SIGTERM the bwrap root —
            // --die-with-parent tears the tree down.
            let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).output();
            wait_for_death(pid, ORPHAN_TERM_WAIT);
            if pid_matches(pid, expected_comm) {
                let _ = Command::new("kill").arg("-KILL").arg(pid.to_string()).output();
                wait_for_death(pid, ORPHAN_TERM_WAIT);
            }
            tracing::info!("seat orphan reaped at boot: student={safe} pid={pid}");
            reaped += 1;
        } else if pid_alive(pid) {
            // A live pid whose comm is NOT ours: the pidfile is stale and
            // the slot was reused — never kill; just drop the stale file.
            tracing::info!(
                "seat pidfile pid {pid} reused by a foreign process — pidfile dropped, process untouched"
            );
        }
        // The pidfile's process is gone or foreign — either way the file
        // is stale: drop it so a fresh start records a clean one.
        let _ = std::fs::remove_file(&pidfile);
        if preview {
            // Preview orphan: nothing student-specific survives the tier
            // (T-215) — wipe the scratch dirs. Stub info carries only the
            // fields wipe_preview_scratch reads.
            let info = SeatInfo {
                student_id: safe,
                port: 0,
                pid,
                started_at: Instant::now(),
                gpu: false,
                password: String::new(),
                token: String::new(),
                kiosk_level: 0,
                preview: true,
            };
            wipe_preview_scratch(&info, runtime_root);
        }
    }
    reaped
}

/// Seat collection dir — mirrors the seat-side Anki base layout
/// ({collection_root}/{student_id}) without importing its private sanitizer.
fn collection_dir_for(safe_id: &str, preview: bool) -> PathBuf {
    let root = std::env::var("ANKITOV_COLLECTION_ROOT")
        .unwrap_or_else(|_| "./data/collections".to_string());
    if preview {
        // Preview (drill) seats get a scratch collection under preview/ —
        // wiped on stop/reap (T-215).
        PathBuf::from(root).join("preview").join(safe_id)
    } else {
        PathBuf::from(root).join(safe_id)
    }
}

/// Remove a preview seat's scratch collection + staged service tree
/// (T-215): the whole point of a preview seat is that NOTHING student
/// specific survives the session. No-op for full-tier seats.
fn wipe_preview_scratch(info: &SeatInfo, runtime_root: &std::path::Path) {
    if !info.preview {
        return;
    }
    let collection = collection_dir_for(&info.student_id, true);
    if collection.exists() {
        let _ = std::fs::remove_dir_all(&collection);
    }
    let svc = runtime_root.join(&info.student_id);
    if svc.exists() {
        let _ = std::fs::remove_dir_all(&svc);
    }
}

/// Allow only [A-Za-z0-9._-], reject "." / ".." / traversal — this guard
/// applies before any SQLite path use.
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
            preview_max: 4,
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
                password: "test-pass".into(),
                token: "test-token".into(),
                kiosk_level: 0,
                preview: false,
            },
            last_activity: Instant::now(),
            idle_timeout: Duration::from_secs(300),
        };
        seats.insert("a".into(), mk(8100));
        seats.insert("b".into(), mk(8101));
        assert_eq!(mgr.free_port(&seats), 8102);
    }

    /// Drill seat ids (`drill-<id8>-<rand4>`, T-215) must pass the sanitizer.
    /// rand_suffix is alnum-only, so real ids never contain dots; the
    /// rejection arm proves traversal characters are refused.
    #[test]
    fn drill_seat_ids_pass_sanitize() {
        assert!(sanitize_student_id("drill-abc12345-Xy3z").is_some());
        assert!(sanitize_student_id("drill-../escape").is_none());
    }

    /// Preview scratch dirs are wiped; full-tier seats are untouched.
    #[test]
    fn wipe_preview_scratch_only_touches_preview() {
        let tmp = std::env::temp_dir().join("ankitov-seat-wipe-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("preview/drill-abc12345-x1")).unwrap();
        std::fs::create_dir_all(tmp.join("drill-abc12345-x1")).unwrap();
        std::fs::create_dir_all(tmp.join("runtime/drill-abc12345-x1")).unwrap();
        // collection_dir_for reads ANKITOV_COLLECTION_ROOT — point it at the
        // fixture (nextest isolates per test; same pattern as the caps test).
        std::env::set_var("ANKITOV_COLLECTION_ROOT", &tmp);

        let preview = SeatInfo {
            student_id: "drill-abc12345-x1".into(),
            port: 8100,
            pid: 1,
            started_at: Instant::now(),
            gpu: false,
            password: "p".into(),
            token: "t".into(),
            kiosk_level: 0,
            preview: true,
        };
        wipe_preview_scratch(&preview, &tmp.join("runtime"));
        assert!(!tmp.join("preview/drill-abc12345-x1").exists());
        assert!(!tmp.join("runtime/drill-abc12345-x1").exists());
        // The full-tier collection dir under the same root is untouched.
        assert!(tmp.join("drill-abc12345-x1").exists());

        let full = SeatInfo {
            preview: false,
            ..preview
        };
        wipe_preview_scratch(&full, &tmp.join("runtime"));
        assert!(tmp.join("drill-abc12345-x1").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Reap honors EACH seat's own idle budget: a preview seat idle past
    /// 120 s goes while a full seat idle the same time stays (300 s budget),
    /// and the reaped preview seat's scratch dirs are wiped.
    #[test]
    fn reap_locked_uses_per_seat_idle_and_wipes_preview() {
        let tmp = std::env::temp_dir().join("ankitov-seat-reap-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("preview/drill-abc12345-x1")).unwrap();
        std::fs::create_dir_all(tmp.join("runtime/drill-abc12345-x1")).unwrap();
        // wipe_preview_scratch resolves the collection root from the env —
        // point it at the fixture (nextest isolates per test).
        std::env::set_var("ANKITOV_COLLECTION_ROOT", &tmp);

        let ago = |secs: u64| Instant::now() - Duration::from_secs(secs);
        let mut seats = HashMap::new();
        seats.insert(
            "drill-abc12345-x1".to_string(),
            SeatRecord {
                info: SeatInfo {
                    student_id: "drill-abc12345-x1".into(),
                    port: 8100,
                    // pid 1 is alive on every target (launchd/init) — the
                    // dead-pid branch must not fire; the idle branch does.
                    pid: 1,
                    started_at: ago(400),
                    gpu: false,
                    password: "p".into(),
                    token: "t".into(),
                    kiosk_level: 0,
                    preview: true,
                },
                last_activity: ago(200),
                idle_timeout: PREVIEW_IDLE,
            },
        );
        seats.insert(
            "student-1".to_string(),
            SeatRecord {
                info: SeatInfo {
                    student_id: "student-1".into(),
                    port: 8101,
                    pid: 1,
                    started_at: ago(400),
                    gpu: false,
                    password: "p".into(),
                    token: "t".into(),
                    kiosk_level: 1,
                    preview: false,
                },
                last_activity: ago(200),
                idle_timeout: Duration::from_secs(300),
            },
        );

        SeatManager::reap_locked(&mut seats, &tmp.join("runtime"));
        assert!(
            !seats.contains_key("drill-abc12345-x1"),
            "preview seat idle 200s > 120s budget must be reaped"
        );
        assert!(
            seats.contains_key("student-1"),
            "full seat idle 200s < 300s budget must stay"
        );
        assert!(!tmp.join("preview/drill-abc12345-x1").exists());
        assert!(!tmp.join("runtime/drill-abc12345-x1").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Caps trip BEFORE any filesystem/spawn work: with the caps at zero,
    /// both tiers fail with CapReached and no collection dir is created.
    /// Runs first-touch on `SeatManager::global()` in this process, so the
    /// env must be set before the first `global()` call (nextest isolates
    /// per test; this file's other tests never touch `global()`).
    #[tokio::test]
    async fn caps_trip_before_spawn() {
        std::env::set_var("ANKITOV_SEAT_PREVIEW_MAX", "0");
        std::env::set_var("ANKITOV_SEAT_MAX", "0");
        let mgr = SeatManager::global();
        let preview = SeatLaunch {
            tier: SeatTier::Preview,
            kiosk_level: 0,
            student_email: Some("drill-test@drill.local".into()),
            deck: Some("D".into()),
            packs: Some("p.apkg".into()),
        };
        match mgr.start_seat("drill-captest-aa01", &preview).await {
            Err(SeatError::CapReached) => {}
            other => panic!("preview cap must trip before spawn, got {other:?}"),
        }
        let full = SeatLaunch {
            tier: SeatTier::Full,
            kiosk_level: 1,
            ..SeatLaunch::default()
        };
        match mgr.start_seat("student-captest", &full).await {
            Err(SeatError::CapReached) => {}
            other => panic!("global cap must trip before spawn, got {other:?}"),
        }
        // Nothing was created for either attempt.
        assert!(!std::path::Path::new("data/collections/drill-captest-aa01").exists());
        assert!(!std::path::Path::new("data/collections/student-captest").exists());
    }

    // ── T-220 boot-time orphan reconcile ────────────────────────────────

    #[test]
    fn parse_pidfile_roundtrip() {
        assert_eq!(parse_pidfile("123 1\n"), Some((123, true)));
        assert_eq!(parse_pidfile("456 0"), Some((456, false)));
        assert_eq!(parse_pidfile("789"), Some((789, false)));
        assert_eq!(parse_pidfile(""), None);
        assert_eq!(parse_pidfile("x 1"), None);
    }

    /// A pidfile recording a LIVE seat-like process gets reaped: TERM'd,
    /// pidfile dropped, full-tier runtime dir preserved for restaging.
    /// `sleep` has a stable kernel command name ("sleep") on macOS dev and
    /// Linux CI alike, so it stands in for bwrap with expected_comm="sleep".
    #[test]
    fn reconcile_orphans_live_orphan_killed() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let tmp = std::env::temp_dir().join("ankitov-seat-reconcile-live");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("talmid-1")).unwrap();
        std::fs::write(tmp.join("talmid-1/seat.pid"), format!("{pid} 0\n")).unwrap();

        reconcile_orphans(&tmp, "sleep");

        // try_wait also reaps the zombie, so pid_alive goes false.
        let deadline = Instant::now() + Duration::from_secs(5);
        let exited = loop {
            match child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => break false,
            }
        };
        assert!(exited, "live orphan must be reaped by the reconcile");
        assert!(
            !tmp.join("talmid-1/seat.pid").exists(),
            "pidfile must be dropped after reap"
        );
        assert!(
            tmp.join("talmid-1").exists(),
            "full-tier runtime dir is preserved (restaged on next start)"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A pidfile recording a DEAD pid is cleaned without any kill, and a
    /// preview-tier orphan additionally gets its scratch dirs wiped
    /// (nothing student-specific survives the tier, T-215).
    #[test]
    fn reconcile_orphans_dead_pid_and_preview_wiped() {
        let tmp = std::env::temp_dir().join("ankitov-seat-reconcile-dead");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("runtime/drill-abc12345-x1")).unwrap();
        std::fs::create_dir_all(tmp.join("preview/drill-abc12345-x1")).unwrap();
        std::fs::write(
            tmp.join("runtime/drill-abc12345-x1/seat.pid"),
            "999999999 1\n",
        )
        .unwrap();
        // wipe_preview_scratch resolves the collection root from the env —
        // point it at the fixture (nextest isolates per test).
        std::env::set_var("ANKITOV_COLLECTION_ROOT", &tmp);

        reconcile_orphans(&tmp.join("runtime"), "sleep");

        assert!(!tmp.join("runtime/drill-abc12345-x1/seat.pid").exists());
        assert!(
            !tmp.join("preview/drill-abc12345-x1").exists(),
            "preview orphan scratch collection must be wiped"
        );
        assert!(
            !tmp.join("runtime/drill-abc12345-x1").exists(),
            "preview runtime scratch dir is wiped with it"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A pidfile whose pid was REUSED by a foreign process (comm != ours)
    /// must never be killed — the stale pidfile is dropped, the foreign
    /// process keeps running (headscale-on-8080 lesson).
    #[test]
    fn reconcile_orphans_foreign_pid_untouched() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let tmp = std::env::temp_dir().join("ankitov-seat-reconcile-foreign");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("talmid-2")).unwrap();
        std::fs::write(tmp.join("talmid-2/seat.pid"), format!("{pid} 0\n")).unwrap();

        // Expected comm "bwrap" != the spawned sleep — the identity check
        // must refuse to kill.
        reconcile_orphans(&tmp, "bwrap");

        std::thread::sleep(Duration::from_millis(200));
        match child.try_wait() {
            Ok(Some(_)) => panic!("foreign process must NOT be killed"),
            Ok(None) => {}
            Err(e) => panic!("try_wait failed: {e}"),
        }
        assert!(
            !tmp.join("talmid-2/seat.pid").exists(),
            "stale pidfile must still be dropped"
        );
        assert!(tmp.join("talmid-2").exists());
        // Cleanup the sleeper so the test leaks nothing.
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
