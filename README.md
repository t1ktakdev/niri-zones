# niri-zones

**FancyZones-style window zones for Niri.**

`niri-zones` is a native-to-Niri zone engine for floating windows. The goal is simple: choose a zone and place the current floating window there without replacing Niri's scrolling tiling model.

> Status: pre-release `0.1.0` development. The core and Niri IPC source are implemented; real compositor integration still needs to be verified inside an actual Niri session.

## Why

Niri has a capable floating layout and IPC actions for moving/resizing windows, but it does not currently ship a FancyZones-style zone manager. `niri-zones` adds deterministic layouts, named zones, keyboard-friendly navigation, restore semantics, rules, and later a Wayland-native overlay.

## Current workspace

- `zones-core` — compositor-independent geometry, layouts, directional selection, snap state and a mock backend.
- `zones-config` — schema-versioned TOML, validation and regexes compiled at activation time.
- `zones-niri` — thin Niri 26.04 IPC adapter; normalized `0..1` zones become Niri percentages.
- `niri-zones` — CLI with `status`, `doctor`, `list` and `move`.

The project intentionally starts with four crates rather than splitting every concern into a separate crate.

## Quick start (after building on Linux inside Niri)

```bash
cargo build --release
./target/release/niri-zones doctor
./target/release/niri-zones list halves
./target/release/niri-zones move 1 --layout halves
./target/release/niri-zones move 2 --layout halves
```

A tiled window is refused by default. Converting it to floating must be explicit:

```bash
niri-zones move 1 --float
```

Configured zones can be named, so later the same flow can be `niri-zones move terminal --layout coding`.

## Configuration

Copy `examples/config.toml` to `$XDG_CONFIG_HOME/niri-zones/config.toml` (or `~/.config/niri-zones/config.toml`). The config starts at schema version `1`.

Rules are resolved deterministically: higher explicit priority, then more constraints, then earlier file order. Regexes are compiled only when the candidate config is activated; an invalid candidate never replaces a valid running config.

## Built-in layouts

- `halves`
- `thirds`
- `main-stack`
- `quarters`

The core supports both free-form normalized rectangles and structured split trees.

## Niri integration model

Niri's IPC exposes proportional position/size actions relative to its working area. `niri-zones` stores zones as normalized `0..1` rectangles and translates them to the percentage values expected by Niri. A pixel gap is applied as a fixed inset after the proportional operation, so the client does not need to guess panel/strut dimensions.

A true FancyZones-style drag hook is not promised yet: the current public IPC does not expose enough global pointer/interactive-move state to implement it cleanly without input hacks.

## Planned UX

```kdl
// after the overlay milestone
Mod+Z { spawn "niri-zones" "show"; }
```

Then `Mod+Z`, followed by `1..9`, arrows or a click, will select the zone. The overlay is deferred until the core geometry and real Niri apply semantics are verified.

## Development

Quality gate:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

CI is prepared in `.github/workflows/ci.yml`. See `docs/RESEARCH.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md` and `docs/INTEGRATION-CHECKLIST.md`.

## Non-goals

No compositor fork, no telemetry, no cloud service, no network listener, no arbitrary shell commands from config, and no global input hacks.

## License

MIT
