<div align="center">

# niri-zones

**FancyZones-style window zones for Niri**

Wayland-native zone selection for floating windows — fast, small, and built around Niri IPC.

[English](README.md) · [Русский](README.ru.md)

[![CI](https://github.com/t1ktakdev/niri-zones/actions/workflows/ci.yml/badge.svg)](https://github.com/t1ktakdev/niri-zones/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/t1ktakdev/niri-zones)](https://github.com/t1ktakdev/niri-zones/releases/latest)
[![License](https://img.shields.io/github/license/t1ktakdev/niri-zones)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.86%2B-orange)
![Niri](https://img.shields.io/badge/Niri-26.04-blue)

<img src=".github/assets/hero.svg" alt="niri-zones overlay preview" width="100%">

</div>

---

## What it does

Press a key, choose a zone, and the focused window snaps into place.

```text
Mod+Z
  ↓
┌─────────────────────────────────────────┐
│  ╭────────────────╮ ╭────────────────╮  │
│  │       1        │ │       2        │  │
│  │                │ │                │  │
│  ╰────────────────╯ ╰────────────────╯  │
└─────────────────────────────────────────┘
  ↓
1 / 2 / arrows / mouse
  ↓
window snaps to the selected zone
```

The overlay and the snap backend use the **same geometry engine**, so the preview is the target that Niri receives.

## Features

| Feature | Status |
| --- | --- |
| Wayland-native layer-shell overlay | ✅ |
| Keyboard selection: 1–9, arrows, Enter, Escape | ✅ |
| Mouse hover and click | ✅ |
| Halves / thirds / quarters / main-stack | ✅ |
| Restore original window state | ✅ |
| Explicit tiled → floating conversion | ✅ |
| Idempotent repeated snaps | ✅ |
| Niri event stream integration | ✅ |
| Versioned TOML config | ✅ |
| Physical multi-monitor verification | ⏳ |
| Fractional-scale hardware verification | ⏳ |
| Drag-to-snap | Not implemented |

## Quick start

### 1. Install

Download the Linux x86_64 archive from [Releases](https://github.com/t1ktakdev/niri-zones/releases/latest), extract it, then:

```bash
install -Dm755 niri-zones ~/.local/bin/niri-zones
```

Or build from source:

```bash
git clone https://github.com/t1ktakdev/niri-zones.git
cd niri-zones
cargo build --release
install -Dm755 target/release/niri-zones ~/.local/bin/niri-zones
```

### 2. Verify the session

```bash
niri-zones doctor
```

### 3. Try the overlay

```bash
niri-zones show --layout halves --float
```

Controls:

- `1..9` — select a zone immediately
- `← ↑ ↓ →` — change selection
- `Enter` — confirm
- `Escape` — cancel
- left click — select the zone under the pointer

### 4. Add a Niri keybind

```kdl
Mod+Z repeat=false { spawn "niri-zones" "show" "--float"; }
```

`niri-zones` never edits your Niri config automatically.

## Commands

```bash
niri-zones doctor
niri-zones status
niri-zones list halves
niri-zones show --layout halves --float
niri-zones move 2 --layout halves --float
niri-zones restore
```

## Built-in layouts

- `halves`
- `thirds`
- `quarters`
- `main-stack`

Custom layouts are supported through the versioned TOML config.

## Configuration

Copy the example:

```bash
mkdir -p ~/.config/niri-zones
cp examples/config.toml ~/.config/niri-zones/config.toml
```

Rules are deterministic: higher priority wins, then specificity, then file order. Invalid regexes or malformed zones are rejected before activation.

## How it works

`niri-zones` is split into small Rust crates:

- **zones-core** — geometry, layouts, directional selection, snap state
- **zones-config** — TOML schema, validation, compiled rules
- **zones-niri** — Niri 26.04 IPC adapter
- **zones-overlay** — Wayland/wlr-layer-shell chooser
- **niri-zones** — CLI

Normalized zones are stored in `0..1` space and translated into Niri's proportional working-area operations. A logical-pixel gap is applied after proportional placement.

## Requirements

- Niri 26.04
- Wayland
- `libxkbcommon`
- Rust 1.86+ when building from source

## Known limitations

- Physical multi-monitor integration has not yet been verified.
- Fractional-scale and rotated-output hardware integration still need live testing.
- Drag-to-snap is intentionally not implemented; no global input interception hacks are used.
- No always-running daemon or automatic rule-driven placement yet.

See [ROADMAP](docs/ROADMAP.md) and [ARCHITECTURE](docs/ARCHITECTURE.md) for details.

## Development

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
