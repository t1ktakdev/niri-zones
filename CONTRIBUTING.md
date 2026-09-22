# Contributing

Contributions are welcome.

## Development setup

Use Rust 1.86 or newer and run inside Linux. Live integration checks require a running Niri session.

Before opening a pull request, run:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

## Project boundaries

- Keep core geometry compositor-independent.
- Keep preview geometry and applied geometry on the same `zones-core` path.
- Do not add global input interception hacks.
- Do not silently convert tiled windows to floating.
- Treat Niri runtime window IDs as session-local identifiers.
- Prefer event-driven behavior over polling.

For live compositor changes, test with a disposable window first and include the Niri version, output topology and relevant `niri msg` observations in the pull request.
