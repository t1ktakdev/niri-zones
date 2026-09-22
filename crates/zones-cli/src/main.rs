mod runtime_state;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use runtime_state::{RuntimeState, StoredGeometry, StoredSize, WindowRecord};
use zones_config::ActiveConfig;
use zones_core::{builtin_layout, LayoutDefinition, Rect, ResolvedZone, Size};
use zones_niri::{visual_geometry, NiriBackend, NiriEventStream};

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
    /// Restore a snapped window to its original floating geometry or tiled state.
    Restore {
        /// Niri runtime window id. Defaults to the focused window.
        #[arg(long)]
        id: Option<u64>,
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
            move_to_zone(cli.config.as_deref(), &zone, &layout, id, allow_float, gap)
        }
        Command::Restore { id } => restore_window_state(cli.config.as_deref(), id),
    }
}

fn move_to_zone(
    explicit_config: Option<&Path>,
    requested_zone: &str,
    layout_name: &str,
    id: Option<u64>,
    allow_float: bool,
    gap: Option<f64>,
) -> Result<(), String> {
    let config = load_config_if_present(explicit_config)?;
    let layout = find_layout(layout_name, config.as_ref())?;
    let zones = normalized_zones(&layout)?;
    let zone = zones
        .iter()
        .find(|candidate| {
            candidate.id.0 == requested_zone || candidate.name.eq_ignore_ascii_case(requested_zone)
        })
        .ok_or_else(|| format!("layout '{}' has no zone '{}'", layout.name, requested_zone))?;
    let gap =
        gap.or_else(|| config.as_ref().map(|active| active.source.general.gap)).unwrap_or(12.0);
    let tolerance =
        config.as_ref().map(|active| active.source.general.geometry_tolerance).unwrap_or(2.0);
    let allow_float = allow_float
        || config.as_ref().is_some_and(|active| active.source.general.allow_tiled_to_floating);

    let mut niri = NiriBackend::connect().map_err(|error| error.to_string())?;
    let window_id = resolve_window_id(&mut niri, id)?;
    let before = niri.window(window_id).map_err(|error| error.to_string())?;
    let mut state = load_reconciled_state(&mut niri)?;

    if state.get(window_id).is_some_and(|record| {
        record.app_id.as_deref() != before.app_id.as_deref() || record.pid != before.pid
    }) {
        state.remove(window_id);
        state.save_atomic()?;
    }

    if let Some(record) = state.get(window_id).cloned() {
        let applied = record.applied_geometry.to_rect()?;
        let current = visual_geometry(&before);
        let still_matches = before.is_floating
            && current.is_some_and(|geometry| geometry.approx_eq(applied, tolerance));

        if !still_matches {
            state.remove(window_id);
            state.save_atomic()?;
        } else if record.current_layout == layout.name && record.current_zone == zone.id.0 {
            println!(
                "Window {} is already snapped to {} ({}) in layout {}; no IPC mutation needed.",
                window_id, zone.id.0, zone.name, layout.name
            );
            return Ok(());
        }
    }

    let baseline_floating =
        state.get(window_id).map(|record| record.baseline_floating).unwrap_or(before.is_floating);
    let baseline_geometry = state
        .get(window_id)
        .and_then(|record| record.baseline_geometry)
        .map(|geometry| geometry.to_rect())
        .transpose()?
        .or_else(|| if before.is_floating { visual_geometry(&before) } else { None });

    let baseline_size = state
        .get(window_id)
        .and_then(|record| record.baseline_size)
        .map(StoredSize::to_size)
        .unwrap_or(Size { width: before.layout.tile_size.0, height: before.layout.tile_size.1 });

    let report = niri
        .apply_zone(window_id, zone.normalized, gap, allow_float)
        .map_err(|error| error.to_string())?;
    let applied_geometry = visual_geometry(&report.after).ok_or_else(|| {
        format!("Niri did not report visual geometry for floating window {window_id}")
    })?;

    state.upsert_snap(WindowRecord {
        id: window_id,
        app_id: before.app_id.clone(),
        pid: before.pid,
        baseline_floating,
        baseline_geometry: baseline_geometry.map(StoredGeometry::from_rect),
        baseline_size: Some(StoredSize::from_size(baseline_size)),
        current_layout: layout.name.clone(),
        current_zone: zone.id.0.clone(),
        applied_geometry: StoredGeometry::from_rect(applied_geometry),
    });
    state.save_atomic()?;

    println!(
        "Snapped window {} to {} ({}) using {} IPC actions.",
        report.after.id, zone.id.0, zone.name, report.actions_sent
    );
    Ok(())
}

fn restore_window_state(explicit_config: Option<&Path>, id: Option<u64>) -> Result<(), String> {
    let config = load_config_if_present(explicit_config)?;
    let tolerance =
        config.as_ref().map(|active| active.source.general.geometry_tolerance).unwrap_or(2.0);

    let mut niri = NiriBackend::connect().map_err(|error| error.to_string())?;
    let window_id = resolve_window_id(&mut niri, id)?;
    let current = niri.window(window_id).map_err(|error| error.to_string())?;
    let mut state = load_reconciled_state(&mut niri)?;
    let record = state
        .get(window_id)
        .cloned()
        .ok_or_else(|| format!("window {window_id} has no saved snap state to restore"))?;

    if record.app_id.as_deref() != current.app_id.as_deref() || record.pid != current.pid {
        state.remove(window_id);
        state.save_atomic()?;
        return Err(format!(
            "saved state for window {window_id} belongs to a different window and was discarded"
        ));
    }

    let applied = record.applied_geometry.to_rect()?;
    if !current.is_floating
        || visual_geometry(&current)
            .map_or(true, |geometry| !geometry.approx_eq(applied, tolerance))
    {
        state.remove(window_id);
        state.save_atomic()?;
        return Err(format!(
            "window {window_id} changed manually since snap; its saved restore baseline was discarded"
        ));
    }

    let baseline_geometry =
        record.baseline_geometry.map(|geometry| geometry.to_rect()).transpose()?;
    let baseline_size = record.baseline_size.map(StoredSize::to_size);
    let restored = niri
        .restore_window(window_id, record.baseline_floating, baseline_geometry, baseline_size)
        .map_err(|error| error.to_string())?;

    if record.baseline_floating {
        let expected =
            baseline_geometry.ok_or_else(|| "floating baseline geometry is missing".to_owned())?;
        let actual = visual_geometry(&restored).ok_or_else(|| {
            format!("Niri did not report restored geometry for window {window_id}")
        })?;
        if !actual.approx_eq(expected, tolerance) {
            return Err(format!(
                "restore verification failed for window {window_id}: expected {expected:?}, got {actual:?}"
            ));
        }
    } else {
        if restored.is_floating {
            return Err(format!(
                "restore verification failed: window {window_id} is still floating"
            ));
        }
        let expected = baseline_size.ok_or_else(|| "tiled baseline size is missing".to_owned())?;
        let actual =
            Size { width: restored.layout.tile_size.0, height: restored.layout.tile_size.1 };
        if (actual.width - expected.width).abs() > tolerance
            || (actual.height - expected.height).abs() > tolerance
        {
            return Err(format!(
                "restore verification failed for tiled window {window_id}: expected {expected:?}, got {actual:?}"
            ));
        }
    }

    state.remove(window_id);
    state.save_atomic()?;
    println!("Restored window {window_id} to its original state.");
    Ok(())
}

fn resolve_window_id(niri: &mut NiriBackend, id: Option<u64>) -> Result<u64, String> {
    match id {
        Some(id) => Ok(id),
        None => niri
            .focused_window()
            .map_err(|error| error.to_string())?
            .map(|window| window.id)
            .ok_or_else(|| "no focused window".to_owned()),
    }
}

fn load_reconciled_state(niri: &mut NiriBackend) -> Result<RuntimeState, String> {
    let mut state = RuntimeState::load_current()?;
    let windows = niri.windows().map_err(|error| error.to_string())?;
    let changed = state.retain_where(|record| {
        windows.iter().any(|window| {
            window.id == record.id
                && window.app_id.as_deref() == record.app_id.as_deref()
                && window.pid == record.pid
        })
    });
    if changed {
        state.save_atomic()?;
    }
    Ok(state)
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
                Ok(outputs) if !outputs.is_empty() => {
                    println!("✓ {} output(s) visible through IPC", outputs.len())
                }
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
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
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
