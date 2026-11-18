use backend::{app::App, banner, migrations::Migrator};
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

    // The `start` subcommand takes a custom path so the AnkiTov banner replaces
    // loco-rs's default loco.rs art (loco 0.11 has no banner hook). Every other
    // subcommand delegates to loco's CLI unchanged.
    if is_start_invocation() {
        start_with_ankitov_banner().await
    } else {
        cli::main::<App, Migrator>().await
    }
}

/// True when the first positional CLI argument is the `start` subcommand (or its
/// `s` alias). Global flags may precede it; `-e/--environment` consumes a value.
/// Anything else (db, routes, doctor, no args, …) delegates to `loco_rs::cli`.
fn is_start_invocation() -> bool {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy().into_owned();
        match arg.as_str() {
            "-e" | "--environment" => {
                args.next(); // skip the environment value
            }
            _ if arg.starts_with('-') => {} // global flag without a value
            subcommand => return subcommand == "start" || subcommand == "s",
        }
    }
    false // no subcommand — let loco print its own "requires a subcommand" error
}

/// Mirror of `loco_rs::cli::main`'s `Start` branch (same flags, same boot flow),
/// printing the AnkiTov banner instead of loco's art. Loco's own banner is
/// suppressed by passing `no_banner = true` to `boot::start`.
/// Minimal stand-in for loco's private `logger::init` (loco-rs 0.11 keeps the
/// `logger` module private, so `cli::main`'s fallback is unreachable from the
/// app crate). Mirrors loco's stdout path exactly for our configs: a stdout fmt
/// layer + the same env-filter rules (RUST_LOG → `override_filter` → module
/// whitelist + app level). Neither development.yaml nor production.yaml sets
/// `logger.file_appender` (loco 0.11 schema), so stdout-only is faithful; if a
/// file appender is ever configured, extend this or route `start` back through
/// `loco_rs::cli::main`.
fn init_stdout_logger(config: &loco_rs::config::Logger) {
    use loco_rs::app::Hooks;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

    /// Mirrors loco-rs 0.11.2 `logger::MODULE_WHITELIST`.
    const MODULE_WHITELIST: &[&str] =
        &["loco_rs", "sea_orm_migration", "tower_http", "sqlx::query", "sidekiq"];

    let level = config.level.to_string();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // loco's Format/LogLevel enums live in its private `logger` module and
        // cannot be named or matched from here — route on the Debug spelling.
        let resolved = match config.override_filter.as_ref() {
            Some(f) => EnvFilter::try_new(f.clone()),
            None => EnvFilter::try_new(
                MODULE_WHITELIST
                    .iter()
                    .map(|m| format!("{m}={level}"))
                    .chain(std::iter::once(format!("{}={}", App::app_name(), level)))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        };
        resolved.expect("logger initialization failed")
    });

    let fmt_layer = match format!("{:?}", config.format).as_str() {
        "Pretty" => tracing_subscriber::fmt::layer().pretty().with_ansi(true).boxed(),
        "Json" => tracing_subscriber::fmt::layer().json().with_ansi(true).boxed(),
        _ => tracing_subscriber::fmt::layer().compact().with_ansi(true).boxed(),
    };

    tracing_subscriber::registry().with(fmt_layer).with(filter).init();
}

async fn start_with_ankitov_banner() -> loco_rs::Result<()> {
    use clap::Parser;
    use loco_rs::app::Hooks;
    use loco_rs::boot::{create_app, start, ServeParams, StartMode};
    use loco_rs::environment::{resolve_from_env, Environment};

    #[derive(Parser)]
    #[command(name = "backend")]
    #[command(about = "AnkiTov server (loco-based)", long_about = None)]
    struct StartCli {
        /// Specify the environment
        #[arg(short, long, global = true)]
        environment: Option<String>,
        #[command(subcommand)]
        command: StartCommand,
    }

    #[derive(clap::Subcommand)]
    enum StartCommand {
        /// Start an app
        #[clap(alias("s"))]
        Start {
            /// start worker
            #[arg(short, long)]
            worker: bool,
            /// start same-process server and worker
            #[arg(short, long)]
            server_and_worker: bool,
            /// server bind address
            #[arg(short, long)]
            binding: Option<String>,
            /// server port address
            #[arg(short, long)]
            port: Option<i32>,
            /// disable the banner display
            #[arg(short, long)]
            no_banner: bool,
        },
    }

    let cli = StartCli::parse();
    let environment: Environment = cli
        .environment
        .unwrap_or_else(resolve_from_env)
        .into();

    let config = environment.load()?;

    if !App::init_logger(&config, &environment)? {
        init_stdout_logger(&config.logger);
    }

    let (worker, server_and_worker, binding, port, no_banner) = match cli.command {
        StartCommand::Start {
            worker,
            server_and_worker,
            binding,
            port,
            no_banner,
        } => (worker, server_and_worker, binding, port, no_banner),
    };

    let start_mode = if worker {
        StartMode::WorkerOnly
    } else if server_and_worker {
        StartMode::ServerAndWorker
    } else {
        StartMode::ServerOnly
    };

    let boot_result = create_app::<App, Migrator>(start_mode, &environment).await?;
    let serve_params = ServeParams {
        port: port.map_or(boot_result.app_context.config.server.port, |p| p),
        binding: binding
            .unwrap_or_else(|| boot_result.app_context.config.server.binding.to_string()),
    };

    if !no_banner {
        banner::print_banner(&boot_result, &serve_params);
    }

    // Loco's own banner is always suppressed here — ours (or nothing) already printed.
    start::<App>(boot_result, serve_params, true).await
}
