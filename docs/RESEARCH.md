# Research / feasibility — 2026-09-22

Target baseline: **Niri v26.04 / `niri-ipc = 26.4.0`**, the latest stable Niri release at the time this document was written.

## Confirmed capabilities

Niri IPC currently exposes:

- `Request::Outputs`, including logical output origin, logical width/height, scale and transform.
- `Request::Windows` and `Request::FocusedWindow`.
- per-window runtime ID, app id, title, PID (when available), workspace, focus, floating state and layout information.
- `WindowLayout` with tile size, window size, window offset and tile position in the workspace view.
- `Request::EventStream`, with complete initial state followed by incremental events.
- `WindowOpenedOrChanged`, `WindowClosed`, `WindowFocusChanged` and `WindowLayoutsChanged` events.
- actions to move a window to floating/tiling, set window width/height and move a floating window.
- fixed and proportional position/size changes. Proportional values are especially useful because Niri defines them relative to the working area.

The official IPC docs explicitly warn that requests/actions are processed separately and state can change between requests. `niri-zones` therefore must revalidate state before commit and verify after apply.

## Important limitation: exact usable area

`Output.logical` gives the logical output rectangle, but the IPC layer-surface list does not expose exclusive-zone geometry. Niri's own working area also includes effects from layer-shell panels/struts. Therefore a generic client should not pretend it can reconstruct the exact usable rectangle from output geometry alone.

MVP strategy:

1. Keep layouts in normalized coordinates.
2. Prefer Niri proportional position/size actions where possible, because Niri interprets them against its working area.
3. Keep fixed-pixel gap handling in the pure layout engine for previews/tests and for backends that expose a usable rectangle.
4. Do not infer panel sizes from layer surface names.

## Drag-to-snap feasibility

A true FancyZones drag hook is **not an MVP promise**.

Niri supports interactive move internally, but its public IPC event stream does not expose an "interactive move started", global pointer position, or button-release event. There is also an open discussion about exposing the window currently under the cursor. Standard Wayland clients do not know global positions of arbitrary surfaces.

Safe MVP UX:

```text
Mod+Z -> overlay -> 1..9 / arrows / click -> snap
```

Future drag-to-snap should only be implemented if Niri gains a clean public API or an appropriate protocol; no input injection, global evdev grabs, or polling hacks.

## Existing ecosystem

The official `niri-wm/awesome-niri` list includes window-management tools such as miri, niri-pip, scratchpads, sticky-window tools and a floating sidebar. At research time it does not list a dedicated FancyZones-style zone manager.

KDE has projects such as KZones which demonstrate the desired overlay/layout UX, but KWin scripting has compositor-internal capabilities that a generic Wayland client does not automatically have.

## Niri-native scope

`niri-zones` targets floating windows and optional explicit tiled-to-floating conversion. It does not attempt to replace Niri's scrolling layout.

## Session identity

Runtime Niri window IDs are stable only while the window is open. Persisted restore data must be tied to a compositor/session marker rather than treating `WindowId` as application identity. The first implementation will use a process-lifetime session token and discard runtime-only state on restart; a stronger Niri session marker can be added when the daemon phase lands.

## Current phase boundary

Verified by code/design in this workspace:

- compositor-independent geometry model
- normalized rectangle validation/resolution
- split-tree layouts
- built-in layouts
- directional zone selection
- snap/restore state semantics
- config validation and precompiled regex rules
- mock backend and transaction-oriented engine design
- Niri IPC adapter source targeting 26.04

Not verified here:

- compilation, because this isolated build environment has no Rust toolchain and no outbound package network
- real Niri socket interaction
- compositor apply/verify timing
- layer-shell overlay
- drag-to-snap
