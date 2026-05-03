# Codex Handoff

Last updated: 2026-05-04.

This file summarizes the conversation, decisions, implementation state, and operational assumptions so a fresh Codex session can continue without rereading the whole chat.

## User Goal

Build a lightweight Discord-like app with text chat and voice chat.

Core direction:

- Rust-first implementation.
- Windows and Linux support.
- No Chromium or Electron.
- Start as a self-hosted app for friends.
- Keep the architecture easy to evolve into a real hosted service later.
- Separate common code from OS-specific code.

## Important User Permissions

The user explicitly allowed broad autonomy for this development environment.

- The current Docker container is disposable and may be used freely.
- Destructive operations inside this Docker container are allowed when useful.
- Installing/removing packages, starting/stopping services, resetting local DBs, and deleting generated local data are allowed.
- Git operations may be done autonomously at reasonable checkpoints, including `git commit` and `git push`.
- Destructive git operations are allowed for this dedicated development repository or AI-managed branches if needed.

Still treat these as protected unless the user explicitly asks:

- Production deployments.
- Production databases/storage/cloud/billing resources.
- Secrets or credentials that could be exposed outside the workspace.
- Protected production branches or release tags outside this disposable development context.

This policy is also documented in `docs/development-operations.md`.

## AutoAI Status

The broad AutoAI workflow was attempted multiple times. It consistently blocked because Claude workers were invoked as root with:

```text
--dangerously-skip-permissions cannot be used with root/sudo privileges for security reasons
```

Because of that, the implementation was completed manually in this repo.

## Current Architecture

Cargo workspace crates:

- `crates/light-discord-core`: shared protocol types.
- `crates/light-discord-auth`: password hashing, invite/session token helpers.
- `crates/light-discord-storage`: PostgreSQL persistence plus in-memory development storage.
- `crates/light-discord-server`: TCP chat/control server and UDP voice relay.
- `crates/light-discord-client`: native `egui`/`eframe` desktop client.
- `crates/light-discord-platform`: OS-specific boundary for audio/platform integrations.

Important docs:

- `README.md`: overview and quick use.
- `docs/how-to-use-ja.md`: current Japanese usage guide.
- `docs/requirements-ja.md`: requirements and product direction.
- `docs/architecture.md`: architecture notes.
- `docs/development-operations.md`: operational permissions and git workflow.

## Product Decisions Made

Authentication:

- Friend-use MVP uses invite + password login.
- Passwords are hashed with Argon2id.
- Session tokens are returned to clients and stored server-side only as hashes.
- PostgreSQL deployments require login/session auth.
- Memory-only development mode allows display-name `Dev` login by default.
- `LD_DEV_AUTH=1` is local-only and must not be used for public servers.

Persistence:

- PostgreSQL is the intended self-hosted/production storage.
- In-memory storage remains only for local development.
- Visible chat history is capped at 30 days.
- User-deleted messages are hidden from normal channel history.
- Deleted-message snapshots are stored in admin-only audit logs.
- MVP audit retention is also capped at 30 days.

Admin:

- Initial admin can be created with `LD_BOOTSTRAP_ADMIN_NAME` and `LD_BOOTSTRAP_ADMIN_PASSWORD`.
- Admin can create invite codes.
- Admin can read audit logs.

Voice:

- Current voice support is room membership and UDP packet relay plumbing.
- Real microphone/speaker audio, Opus, jitter buffer, mute/deafen, device selection, and noise/echo handling are future work.

## Current How To Use

Development mode without PostgreSQL:

```bash
cargo run -p light-discord-server
cargo run -p light-discord-client
```

In the client:

- Select `Dev`.
- Enter any display name.
- Press `Connect`.

PostgreSQL self-hosted mode:

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
export LD_BOOTSTRAP_ADMIN_NAME=admin
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='first-friend-invite'
export LD_DEV_AUTH=0
cargo run -p light-discord-server
```

Then:

```bash
cargo run -p light-discord-client
```

Client auth flow:

- Admin first logs in with `Login`, `admin`, and the bootstrap password.
- Admin creates invite codes from the left sidebar admin panel.
- Friends use `Register` with invite code, display name, and password.
- Later users can use `Login` or `Session`.

## PostgreSQL Setup

PostgreSQL must be reachable from `light-discord-server` for `LD_DATABASE_URL` mode.

Scripts:

- `scripts/setup-postgres-linux.sh`: installs/starts PostgreSQL and creates DB/user.
- `scripts/check-postgres.sh`: verifies `LD_DATABASE_URL`.

Important fix already made:

- `LD_*` environment variables are stripped by `sudo` because they are dynamic-linker-sensitive.
- `setup-postgres-linux.sh` accepts `LD_PG_*`, but before `sudo` re-exec it copies them to `LIGHT_DISCORD_PG_*`.
- The script also detects the active Debian/Ubuntu PostgreSQL cluster port with `pg_lsclusters` and prints the correct `LD_DATABASE_URL`.
- `LD_PG_PORT` can force a port.

Local container note:

- PostgreSQL 15 was installed in this Docker container during verification.
- In this container, the PostgreSQL cluster is currently online on port `5433`, not `5432`, because `5432` was already occupied.
- A throwaway test DB/user was created during verification: database `ld_test`, user `ld_user`, password `override`.
- Treat this as disposable local test data.

Verified local connection:

```bash
LD_DATABASE_URL='postgres://ld_user:override@localhost:5433/ld_test' scripts/check-postgres.sh
```

## Verification Commands Used

General:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

PostgreSQL integration test with local container DB:

```bash
LD_TEST_DATABASE_URL='postgres://ld_user:override@localhost:5433/ld_test' cargo test -p light-discord-storage --test postgres
LD_TEST_DATABASE_URL='postgres://ld_user:override@localhost:5433/ld_test' cargo test --workspace
```

Server smoke:

```bash
cargo run -p light-discord-server
```

Bootstrap-admin login smoke previously verified over raw TCP:

```bash
LD_BOOTSTRAP_ADMIN_PASSWORD='password123' LD_DEV_AUTH=0 target/debug/light-discord-server
```

Then sent a `login` frame and received `welcome` with `session_token` and `is_admin: true`.

## Recent Commits

Latest important commits on `main`:

```text
fa28028 Fix PostgreSQL setup env handoff
8809e5a Document PostgreSQL setup and dev operations
3437a2b Add project plan and milestone task definitions
675cfdc Add documentation and update README
cbe8122 Add Docker deployment configuration
ee98414 Add light-discord-client: native egui desktop client
2f89c5e Add light-discord-server: TCP chat/control server and UDP voice relay
```

At the time of this handoff, `main` was pushed to `origin/main`.

## Current Known Limitations

- Client session token is kept in memory only. It is not persisted to disk.
- No password reset or account management UI.
- No roles/permissions UI beyond basic admin flag.
- No TLS/reverse proxy automation yet.
- No real audio capture/playback yet.
- No Opus codec or jitter buffer yet.
- Native packaging for Windows/Linux has not been implemented.
- Docker CLI is not installed in the current container, though Docker Compose files exist for host-side use.

## Good Next Tasks

Suggested next development steps:

1. Persist client session token securely per OS through `light-discord-platform`.
2. Add a small admin/account management UI.
3. Add role/channel permission model.
4. Add TLS/reverse-proxy deployment guide for self-hosting.
5. Implement real voice: `cpal` audio backend, Opus encode/decode, jitter buffer.
6. Add PostgreSQL cleanup/migration tests and reset helpers for local DB.
7. Add packaging scripts for Windows and Linux.

## Git Workflow Going Forward

The user wants Codex to commit and push autonomously at appropriate checkpoints.

Recommended behavior:

- Inspect `git status` before changes.
- Keep commits coherent and task-sized.
- Run relevant verification before committing.
- Commit with a concise message.
- Push to the configured remote when successful.
- If push fails, keep the local commit and report the exact failure.

