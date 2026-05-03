# Task 004: Storage and Audit

Task ID: 004-storage-audit

Goal: Add service-ready persistence with deleted-message audit logging.

Status: approved

Expected changes:
- Add `light-discord-storage`.
- Add PostgreSQL schema/migration for users, guilds, channels, messages, and audit logs.
- Add a development memory backend.
- Save messages through the storage boundary.
- Soft-delete messages from user-facing history and save deleted message snapshots to audit logs.

Verification commands:
- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo check --workspace`

Risk notes:
- Hosted-service audit log retention and legal policy must be finalized before public launch.

