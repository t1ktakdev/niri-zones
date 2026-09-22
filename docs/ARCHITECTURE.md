# Architecture

## Principles

- Reliable over clever.
- Event-driven over polling.
- Pure geometry separated from compositor code.
- Runtime window IDs are not persistent app identities.
- Never commit internal snap state before the backend operation is verified.
- A stale overlay/topology generation must not apply geometry.
- No global-input hacks.

## Crates

```text
crates/
  zones-core/     pure domain model + engine + mock backend
  zones-config/   TOML schema + validation + compiled rules
  zones-niri/     Niri IPC translation only
  zones-overlay/  Wayland/wlr-layer-shell chooser using core-resolved geometry
  zones-cli/      user-facing commands
```

The overlay is a short-lived process, not a polling daemon. It captures the target Niri window before taking keyboard focus, renders zones from the same core layout result used by apply, and exits after selection or cancellation. The daemon remains a later milestone.

## Layout model

Two forms coexist because they solve different problems:

1. `NormalizedRect` for arbitrary free-form zones.
2. `SplitNode` for structured layouts where the split topology is meaningful.

Both resolve to `Zone` values. Structured layouts keep their intent across aspect ratios; arbitrary zones remain flexible for editors and overlapping advanced layouts.

## Geometry coordinate spaces

The core names coordinate spaces explicitly instead of pretending all rectangles are global desktop coordinates:

- normalized zone space: `[0,1] x [0,1]`
- backend working-area space: logical compositor coordinates local to a working area
- output topology: separate metadata, versioned by caller/daemon

For Niri, normalized targets can map directly to proportional `PositionChange`/`SizeChange` operations. This lets Niri itself account for its working area.

## Directional selection

Directional navigation filters candidates to the requested half-plane, then scores them by:

1. whether there is orthogonal overlap (overlap strongly preferred);
2. larger overlap ratio;
3. axis distance in the requested direction;
4. orthogonal center distance;
5. Euclidean center distance;
6. stable zone id as final tie-breaker.

This prevents `move right` from simply following zone IDs.

## Snap state

A window starts `Free`.

On first successful snap, save the pre-snap geometry as `baseline`. Moving between zones keeps the same baseline and rotates `current_zone -> previous_zone`.

A verified manual geometry change outside tolerance while snapped ends the snapped state. The manually chosen geometry becomes the new free baseline for the next snap. Restore always returns to the original baseline for the current snap chain.

## Transaction model

Niri cannot make resize + move atomic through one IPC request. The engine therefore uses an optimistic transaction:

```text
read current snapshot
-> validate generation/topology externally
-> no-op if already at target
-> optionally make floating (only when explicitly allowed)
-> apply size
-> apply position
-> read back
-> verify position with tolerance
-> classify size-only divergence as an adjusted/constraint result
-> commit the actual applied geometry
```

On a failed step or a position mismatch, attempt best-effort rollback to the captured snapshot. Rollback failure is surfaced as `Degraded`; internal state is not advanced as though the snap succeeded. A size-only mismatch is kept as `AppliedAdjusted` because application/compositor minimum-size constraints can legitimately alter size without making the placement itself invalid.

## Concurrency

The daemon phase should own a monotonic generation number and a separate output-topology generation. Overlay selection captures both. Immediately before applying a selection, the daemon compares captured generations with current state; stale results are discarded or recalculated.

## Config/rules

Rule regexes are compiled once during config activation. A new config only replaces the active config after parse + validation + rule compilation succeeds.

Conflict model:

1. larger explicit `priority` wins;
2. on equal priority, more match constraints wins;
3. final tie uses file order (earlier rule wins).

This is deterministic and easy to explain.
