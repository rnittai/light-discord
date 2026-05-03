# Task 001: Scaffold

Task ID: 001-scaffold

Goal: Create the Rust workspace and native app/server split.

Status: approved

Expected changes:
- Add `light-discord-core`, `light-discord-server`, `light-discord-client`, and `light-discord-platform`.
- Keep shared protocol types in the core crate.
- Keep OS-specific extension points in the platform crate.

Verification commands:
- `cargo fmt --all`
- `cargo check --workspace`

Notes:
- Local verification requires Rust toolchain installation.

