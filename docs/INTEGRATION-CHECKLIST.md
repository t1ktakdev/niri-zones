# Niri integration checklist

Run this only from a real Niri session. It is intentionally separate from the compositor-independent test suite.

## Before testing

```bash
niri --version
printf '%s\n' "$NIRI_SOCKET"
cargo build --release
./target/release/niri-zones doctor
```

Record the Niri version and monitor topology when reporting a problem.

## Smoke matrix

Test each case with an ordinary floating terminal first, then Firefox or another application with realistic size constraints.

1. `niri-zones list halves`
2. `niri-zones move 1 --layout halves`
3. repeat the same command and confirm the visible geometry does not drift
4. `niri-zones move 2 --layout halves`
5. run with `--gap 0`, `--gap 12`, and a fractional output scale
6. try a tiled window without `--float`: it must refuse
7. repeat with `--float`: conversion must be explicit
8. close the target window just before a command and verify a clean error
9. disconnect/reconnect a secondary output and rerun `doctor`
10. validate dual-output setups with different scales and a rotated output

## Geometry observations to capture

For a failing case save:

```bash
niri msg --json outputs
niri msg --json windows
RUST_LOG=niri_zones=debug niri-zones doctor
```

Do not claim exact apply/verify semantics are stable until this matrix has been exercised on a real compositor. In particular, application minimum sizes and asynchronous configure responses can legitimately make requested and observed sizes differ.
