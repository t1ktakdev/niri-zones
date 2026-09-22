use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use zones_config::ActiveConfig;
use zones_core::{builtin_layout, LayoutDefinition, Rect, ResolvedZone};
use zones_niri::{NiriBackend, NiriEventStream};

#[derive(Debug, Parser)]
#[command(name = "niri-zones", version, about = "FancyZones-style window zones for Niri")]
struct Cli {
    /// Optional config path. Defaults to $XDG_CONFIG_HOME/niri-zones/config.toml.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show Niri connection and focused-window status.
    Status,
    /// Diagnose the Niri socket, IPC, event stream and config.
    Doctor,
    /// List zones in a built-in or configured layout.
    List {
        #[arg(default_value = "halves")]
        layout: String,
    },
    /// Snap a floating window to a zone.
    Move {
        /// Zone id or name.
        zone: String,
        #[arg(long, default_value = "halves")]
        layout: String,
        /// Niri runtime window id. Defaults to the focused window.
        #[arg(long)]
        id: Option<u64>,
        /// Explicitly allow converting a tiled window to floating.
        #[arg(long = "float")]
        allow_float: bool,
        /// Override configured gap in logical pixels.
        #[arg(long)]
        gap: Option<f64>,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("niri-zones: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Status => status(),
        Command::Doctor => doctor(cli.config.as_deref()),
        Command::List { layout } => {
            let config = load_config_if_present(cli.config.as_deref())?;
            let layout = find_layout(&layout, config.as_ref())?;
            for zone in normalized_zones(&layout)? {
                println!("{}\t{}", zone.id.0, zone.name);
            }
            Ok(())
        }
        Command::Move { zone, layout, id, allow_float, gap } => {
            let config = load_config_if_present(cli.config.as_deref())?;
            let layout = find_layout(&layout, config.as_ref())?;
            let zones = normalized_zones(&layout)?;
            let zone = zones
                .iter()
                .find(|candidate| candidate.id.0.as_str() == zone.as_str() || candidate.name.eq_ignore_ascii_case(&zone))
                .ok_or_else(|| format!("layout '{}' has no zone '{}'", layout.name, zone))?;
            let gap = gap.or_else(|| config.as_ref().map(|active| active.source.general.gap)).unwrap_or(12.0);
            let allow_float = allow_float
                || config
                    .as_ref()
                    .is_some_and(|active| active.source.general.allow_tiled_to_floating);

            let mut niri = NiriBackend::connect().map_err(|error| error.to_string())?;
            let window_id = match id {
                Some(id) => id,
                None => niri
                    .focused_window()
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "no focused window".to_owned())?
                    .id,
            };
            let report = niri
                .apply_zone(window_id, zone.normalized, gap, allow_float)
                .map_err(|error| error.to_string())?;
            println!(
                "Snapped window {} to {} ({}) using {} IPC actions.",
                report.after.id, zone.id.0, zone.name, report.actions_sent
            );
            Ok(())
        }
    }
}

fn status() -> Result<(), String> {
    let mut niri = NiriBackend::connect().map_err(|error| error.to_string())?;
    let version = niri.version().map_err(|error| error.to_string())?;
    let outputs = niri.outputs().map_err(|error| error.to_string())?;
    let focused = niri.focused_window().map_err(|error| error.to_string())?;

    println!("niri-zones {}", env!("CARGO_PKG_VERSION"));
    println!("Niri: {version}");
    println!("Outputs: {}", outputs.len());
    match focused {
        Some(window) => println!(
            "Focused window: {}{} ({})",
            window.app_id.as_deref().unwrap_or("unknown"),
            if window.is_floating { " [floating]" } else { " [tiled]" },
            window.id
        ),
        None => println!("Focused window: none"),
    }
    Ok(())
}

fn doctor(explicit_config: Option<&Path>) -> Result<(), String> {
    let mut failed = false;

    if env::var_os("NIRI_SOCKET").is_some() {
        println!("✓ NIRI_SOCKET available");
    } else {
        println!("✗ NIRI_SOCKET is unavailable");
        println!("  niri-zones must run inside a Niri session.");
        failed = true;
    }

    match NiriBackend::connect() {
        Ok(mut niri) => {
            println!("✓ Niri IPC socket connected");
            match niri.version() {
                Ok(version) => println!("✓ Niri IPC responding ({version})"),
                Err(error) => {
                    println!("✗ Niri IPC request failed: {error}");
                    failed = true;
                }
            }
            match niri.outputs() {
                Ok(outputs) if !outputs.is_empty() => println!("✓ {} output(s) visible through IPC", outputs.len()),
                Ok(_) => println!("! Niri reports no connected outputs"),
                Err(error) => {
                    println!("✗ Could not query outputs: {error}");
                    failed = true;
                }
            }
        }
        Err(error) => {
            println!("✗ Could not connect to Niri IPC: {error}");
            failed = true;
        }
    }

    match NiriEventStream::connect() {
        Ok(_) => println!("✓ Event stream handshake accepted"),
        Err(error) => {
            println!("✗ Event stream unavailable: {error}");
            failed = true;
        }
    }

    match load_config_if_present(explicit_config) {
        Ok(Some(_)) => println!("✓ Config parsed, validated and rules compiled"),
        Ok(None) => println!("✓ Config absent; built-in defaults remain available"),
        Err(error) => {
            println!("✗ Config invalid: {error}");
            failed = true;
        }
    }

    println!("! Overlay backend: not implemented in the current core milestone");
    if failed {
        Err("doctor found one or more blocking problems".into())
    } else {
        Ok(())
    }
}

fn load_config_if_present(explicit: Option<&Path>) -> Result<Option<ActiveConfig>, String> {
    let path = explicit.map(Path::to_path_buf).or_else(default_config_path);
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        if explicit.is_some() {
            return Err(format!("config file does not exist: {}", path.display()));
        }
        return Ok(None);
    }
    let source = fs::read_to_string(&path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    ActiveConfig::from_toml(&source)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn default_config_path() -> Option<PathBuf> {
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(base).join("niri-zones/config.toml"));
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/niri-zones/config.toml"))
}

fn find_layout(name: &str, config: Option<&ActiveConfig>) -> Result<LayoutDefinition, String> {
    if let Some(config) = config {
        if let Some(layout) = config.layouts.get(name) {
            return Ok(layout.clone());
        }
    }

    builtin_layout(name).ok_or_else(|| format!("unknown layout '{name}'"))
}

fn normalized_zones(layout: &LayoutDefinition) -> Result<Vec<ResolvedZone>, String> {
    let unit = Rect::new(0.0, 0.0, 1.0, 1.0).map_err(|error| error.to_string())?;
    layout.resolve(unit, 0.0).map_err(|error| error.to_string())
}
