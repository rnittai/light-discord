# Task 006: Auth Session and Audit API

Task ID: 006-auth-session-audit-api

Goal: Enforce real login/session flows for persistent deployments and add admin audit/invite operations.

Status: approved

Expected changes:
- Add register, login, and session resume protocol frames.
- Store session token hashes server-side.
- Restrict audit log reads and invite creation to admin users.
- Keep display-name dev auth only for memory/local development.
- Add PostgreSQL integration test that runs only when `LD_TEST_DATABASE_URL` is set.

Verification commands:
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`

Risk notes:
- Public deployments must set strong bootstrap credentials and keep `LD_DEV_AUTH=0`.

