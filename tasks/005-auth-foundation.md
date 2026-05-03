# Task 005: Auth Foundation

Task ID: 005-auth-foundation

Goal: Add invite/password/session primitives without forcing full account UI into the first client.

Status: approved

Expected changes:
- Add `light-discord-auth`.
- Use Argon2id password hashing.
- Generate invite codes and session tokens.
- Keep the current display-name development connection path until account UI is implemented.

Verification commands:
- `cargo test --workspace`

Risk notes:
- The server must enforce real sessions before internet exposure.

