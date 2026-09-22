# Changelog

All notable changes to niri-zones are documented here.

## 0.1.0 - 2026-09-23

First public release.

### Added

- Wayland-native wlr-layer-shell zone overlay.
- Keyboard selection with digits, arrows, Enter and Escape.
- Pointer hover and left-click zone selection.
- Built-in halves, thirds, quarters and main-stack layouts.
- Normalized zone geometry shared by preview and apply paths.
- Niri 26.04 IPC backend for outputs, windows, focus, floating conversion, resize and move.
- Explicit tiled-to-floating opt-in with `--float`.
- Restore state for returning snapped windows to their original tiled or floating state.
- Idempotent repeated snaps and stale runtime WindowId reconciliation.
- Versioned TOML configuration with validation and deterministic rule resolution.
- Niri event-stream adapter and integration probe.
- `status`, `doctor`, `list`, `move`, `show` and `restore` commands.

### Verified

- Rust fmt/check/clippy/test/release-build gate on Arch Linux.
- Live Niri 26.04 IPC on a 1920x1200 scale-1 output.
- Tiled refusal without `--float`.
- Tiled-to-floating snap to left/right zones.
- Keyboard and pointer overlay selection.
- Restore to the original tiled geometry.
- Repeated snap no-op behavior.
- Window disappearance returns a clean error.
- Niri event-stream open/focus/close events.
- Delayed Wayland configure settling before applied geometry is committed.

### Known limitations

- Physical multi-monitor integration is not yet verified.
- Fractional-scale and rotated-output hardware integration is not yet verified.
- Drag-to-snap is intentionally not implemented.
- No long-running daemon or automatic rule-driven placement yet.
