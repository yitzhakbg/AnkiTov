//! AnkiTov startup banner.
//!
//! Replaces loco-rs's default `loco.rs` ASCII art (`loco_rs::banner::print_banner`)
//! with the AnkiTov brand mark: the dot-cluster + ANKITOV wordmark rendered in the
//! logo's palette (blue ANKI, rust TOV, orange/yellow dots), followed by the same
//! runtime info lines loco prints (environment, database flags, logger, compilation
//! mode, listening address).
//!
//! Colors are emitted with the `colored` crate, which auto-strips ANSI codes when
//! stdout is not a TTY (e.g. the launcher's `ankitov.log` redirect), so log files
//! stay clean.
//!
//! Wired up in `main.rs`: the `start` subcommand takes a custom path that prints
//! this banner and calls `loco_rs::boot::start(.., no_banner = true)`; every other
//! subcommand delegates to `loco_rs::cli::main` unchanged.

use colored::Colorize;

use loco_rs::app::Hooks;
use loco_rs::boot::{BootResult, ServeParams};

use crate::app::App;

/// Logo palette (sampled from specs/outreach/youtube/assets/ankitov-logo.png).
const BLUE: (u8, u8, u8) = (46, 117, 182); // "ANKI" wordmark + blue dot
const RUST: (u8, u8, u8) = (139, 58, 26); // "TOV" wordmark + rust dots
const ORANGE: (u8, u8, u8) = (232, 150, 60); // orange dots + the drip
const YELLOW: (u8, u8, u8) = (240, 192, 32); // yellow dots

/// ANKITOV block wordmark, 5 rows × 53 cols. Columns 0..31 spell ANKI, 31..53 TOV
/// (the split index matters for the two-tone coloring below).
const WORDMARK: [&str; 5] = [
    " █████  ██   ██ ██   ██ ██████ ██████  ████  ██    ██",
    "██   ██ ███  ██ ██  ██    ██     ██   ██  ██ ██    ██",
    "███████ ██ █ ██ ████      ██     ██   ██  ██  ██  ██ ",
    "██   ██ ██  ███ ██  ██    ██     ██   ██  ██   ████  ",
    "██   ██ ██   ██ ██   ██ ██████   ██    ████     ██   ",
];
const WORDMARK_SPLIT: usize = 31; // ANKI | TOV

/// Dot-cluster rows above the wordmark: (column, palette) pairs, echoing the
/// logo's scattered molecules with the drip descending toward the O.
const DOTS_ROW_1: &[(usize, (u8, u8, u8))] = &[
    (8, YELLOW),
    (11, ORANGE),
    (16, ORANGE),
    (17, RUST),
    (21, ORANGE),
    (26, RUST),
    (31, ORANGE),
    (32, RUST),
    (37, ORANGE),
    (43, RUST),
    (47, ORANGE),
];
const DOTS_ROW_2: &[(usize, (u8, u8, u8))] = &[
    (5, ORANGE),
    (10, YELLOW),
    (14, ORANGE),
    (19, RUST),
    (24, BLUE),
    (29, ORANGE),
    (34, RUST),
    (39, ORANGE),
    (44, RUST),
    (48, ORANGE),
    (50, RUST),
];

fn paint(text: &str, rgb: (u8, u8, u8)) -> String {
    text.truecolor(rgb.0, rgb.1, rgb.2).to_string()
}

fn dots_row(dots: &[(usize, (u8, u8, u8))]) -> String {
    let width = dots.iter().map(|(c, _)| *c).max().unwrap_or(0) + 1;
    let mut row: Vec<Option<(u8, u8, u8)>> = vec![None; width];
    for (col, rgb) in dots {
        row[*col] = Some(*rgb);
    }
    row.into_iter()
        .map(|cell| match cell {
            Some(rgb) => paint("●", rgb),
            None => " ".to_string(),
        })
        .collect()
}

/// Print the AnkiTov banner plus the runtime info lines loco would have printed.
pub fn print_banner(boot_result: &BootResult, server_config: &ServeParams) {
    let ctx = &boot_result.app_context;

    // The dot cluster sits ABOVE the wordmark (as in the logo), with the drip
    // descending toward the O.
    println!("{}", dots_row(DOTS_ROW_1));
    println!("{}", dots_row(DOTS_ROW_2));
    for row in WORDMARK {
        // Char-indexed split (█ is multi-byte UTF-8, so str::split_at's byte
        // index would panic mid-glyph).
        let left: String = row.chars().take(WORDMARK_SPLIT).collect();
        let right: String = row.chars().skip(WORDMARK_SPLIT).collect();
        println!("{}{}", paint(&left, BLUE), paint(&right, RUST));
    }
    println!();
    println!(
        " {} v{} · {}",
        "AnkiTov".bold(),
        App::app_version(),
        "https://ankitov.com".blue()
    );
    println!();

    println!("environment: {}", ctx.environment.to_string().green());

    #[cfg(feature = "with-db")]
    {
        let config = &ctx.config;
        let mut database = Vec::new();
        if config.database.enable_logging {
            database.push("logging".green());
        }
        if config.database.auto_migrate {
            database.push("automigrate".yellow());
        }
        if config.database.dangerously_recreate {
            database.push("recreate".bright_red());
        }
        if config.database.dangerously_truncate {
            database.push("truncate".bright_red());
        }
        if !database.is_empty() {
            println!(
                "   database: {}",
                database
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    if ctx.config.logger.enable {
        println!("     logger: {}", ctx.config.logger.level.to_string().green());
    } else {
        println!("     logger: {}", "disabled".bright_red());
    }
    if cfg!(debug_assertions) {
        println!("compilation: {}", "debug".bright_red());
    } else {
        println!("compilation: {}", "release".green());
    }

    let mut modes = Vec::new();
    let mut servingline = Vec::new();
    if boot_result.router.is_some() {
        modes.push("server".green());
        servingline.push(format!(
            "listening on http://{}:{}",
            server_config.binding.to_string().green(),
            server_config.port.to_string().green()
        ));
    }
    if boot_result.run_worker {
        modes.push("worker".green());
        servingline.push(format!("worker is {}", "online".green()));
    }
    if !modes.is_empty() {
        println!(
            "      modes: {}",
            modes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    println!();
    println!("{}", servingline.join("\n"));
}
