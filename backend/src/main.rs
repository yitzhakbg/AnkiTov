use backend::{app::App, migrations::Migrator};
use loco_rs::cli;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    // Load .env from the backend/ directory (gitignored, contains secrets).
    // dotenvy silently returns Err if no .env found in CWD — we try both
    // the CWD and the executable-relative path.
    if dotenvy::dotenv().is_err() {
        // Fallback: try relative to the executable
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = dotenvy::from_path(dir.join("../../.env"));
            }
        }
    }
    cli::main::<App, Migrator>().await
}
