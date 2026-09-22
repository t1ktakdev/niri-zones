# Roadmap

## 0.1 core milestone — implemented

- normalized and split-tree layouts
- deterministic directional zone selection
- snap/restore state machine against a mock backend
- versioned TOML config and precompiled rules
- Niri 26.04 IPC translation using working-area percentages
- `status`, `doctor`, `list`, `move`, `restore`

## 0.1 integration milestone — live single-output path verified

- fixed/proportional size and floating-position semantics verified on Niri 26.04
- delayed configure settling is bounded before actual geometry is committed
- restore state persists with a Niri-session marker and atomic writes
- stale runtime WindowIds are reconciled against live `id + app_id + pid`
- real Niri event-stream open/focus/close events verified
- output/workspace-aware layout selection remains pending

## Overlay milestone — first live implementation

- Wayland-native wlr-layer-shell overlay implemented
- keyboard selection: 1..9, arrows, Enter and Escape implemented
- pointer hover and left-click selection implemented
- exact preview uses the same resolved zone model used for apply
- explicit per-output targeting, fractional-scale rendering, Tab cycling and hover hysteresis remain pending

## Daemon milestone

- one command socket owned by the current user
- separate Niri event-stream socket
- generation and topology counters
- safe config reload
- auto-rules after event-driven window readiness
- crash recovery and stale runtime-window cleanup

## Later

- visual zone editor
- previous-zone toggle
- groups for placement of existing windows
- optional local layout-memory suggestions
- drag-to-snap only if Niri/Wayland exposes a clean, secure API
- AUR and GitHub Releases after explicit release approval
