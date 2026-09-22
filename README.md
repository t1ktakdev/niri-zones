# niri-zones

**FancyZones-style window zones for Niri.**

`niri-zones` is a native-to-Niri zone engine for floating windows. The goal is simple: choose a zone and place the current floating window there without replacing Niri's scrolling tiling model.

> **v0.1.0** — usable first release. Core snapping, restore state, Niri 26.04 IPC integration, and the Wayland-native layer-shell overlay have been exercised on a live Niri session.

## Why

Niri has a capable floating layout and IPC actions for moving/resizing windows, but it does not currently ship a FancyZones-style zone manager. `niri-zones` adds deterministic layouts, named zones, keyboard/mouse selection, restore semantics, rules, and a Wayland-native overlay.

## Current workspace

- `zones-core` — compositor-independent geometry, layouts, directional selection, snap state and a mock backend.
- `zones-config` — schema-versioned TOML, validation and regexes compiled at activation time.
- `zones-niri` — thin Niri 26.04 IPC adapter; normalized `0..1` zones become Niri percentages.
- `zones-overlay` — Wayland-native wlr-layer-shell chooser with transparent previews, pointer selection, digits, arrows, Enter and Escape.
- `niri-zones` — CLI with `status`, `doctor`, `list`, `move`, `show` and `restore`.

The workspace stays deliberately small: core math is shared by the IPC backend and overlay so preview geometry cannot drift from snap geometry.

## Requirements

- Niri 26.04
- Wayland session with `wlr-layer-shell`
- `libxkbcommon`
- Rust 1.86+ only when building from source

## Installation

### Release binary

Download the Linux x86_64 archive from the GitHub Releases page, extract it, then place `niri-zones` somewhere in your `PATH`, for example `~/.local/bin`.

### From source

```bash
git clone https://github.com/t1ktakdev/niri-zones.git
cd niri-zones
cargo build --release
install -Dm755 target/release/niri-zones ~/.local/bin/niri-zones
```

## Quick start

```bash
cargo build --release
./target/release/niri-zones doctor
./target/release/niri-zones list halves
./target/release/niri-zones show --layout halves --float
./target/release/niri-zones move 2 --layout halves --float
./target/release/niri-zones restore
```

A tiled window is refused by default. Converting it to floating must be explicit:

```bash
niri-zones move 1 --float
```

Configured zones can be named, so the same flow can be `niri-zones move terminal --layout coding`. The overlay captures the focused Niri window before taking keyboard focus, then snaps that original window after selection.

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

## Overlay UX

The first layer-shell overlay is implemented. A recommended Niri binding is:

```kdl
Mod+Z repeat=false { spawn "niri-zones" "show" "--float"; }
```

`1..9` selects immediately, arrows change selection, Enter confirms, Escape cancels, and a left click selects the zone under the pointer. Tiled-to-floating conversion remains explicit through `--float` (or the corresponding config option).

The binding above is only a snippet; `niri-zones` never rewrites your Niri config automatically.

## Known limitations

- Real multi-monitor placement has not yet been exercised on a physical multi-output setup.
- Fractional-scale and rotated-output integration still need live hardware verification.
- Drag-to-snap is intentionally not implemented; there is no global-input interception hack.
- The current overlay is short-lived and event-driven; there is no always-running daemon yet.
- Automatic rule-driven placement and hot config reload are planned beyond v0.1.0.

## Development

Quality gate:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

CI is prepared in `.github/workflows/ci.yml`. See `docs/RESEARCH.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md` and `docs/INTEGRATION-CHECKLIST.md`.

## Non-goals

No compositor fork, no telemetry, no cloud service, no network listener, no arbitrary shell commands from config, and no global input hacks.

## License

MIT
