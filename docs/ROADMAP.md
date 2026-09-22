# Roadmap

## 0.1 core milestone — current

- normalized and split-tree layouts
- deterministic directional zone selection
- snap/restore state machine against a mock backend
- versioned TOML config and precompiled rules
- Niri 26.04 IPC translation using working-area percentages
- `status`, `doctor`, `list`, `move`

## 0.1 integration milestone

- verify fixed/proportional size and tile-position semantics in a real Niri session
- bounded/event-driven post-apply verification
- wire rollback against verified Niri geometry semantics
- output/workspace-aware layout selection
- persist state with a session marker and atomic writes

## Overlay milestone

- Wayland-native layer-shell overlay
- per-output surfaces and HiDPI
- keyboard selection: 1..9, arrows, Tab, Enter, Escape
- pointer hover with hysteresis
- exact preview from the same resolved zone model used for apply

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
