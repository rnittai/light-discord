# light-discord

A lightweight Discord-like Rust application scaffold for Windows and Linux. It uses a native desktop UI through `egui`/`eframe` and does not use Chromium or Electron.

## What Exists

- `light-discord-core`: shared protocol types for chat, presence, and voice packets.
- `light-discord-auth`: password hashing, invite code, and session token primitives.
- `light-discord-storage`: PostgreSQL persistence and development memory storage.
- `light-discord-server`: TCP chat/control server and UDP voice packet relay.
- `light-discord-client`: native desktop client with connection, channel chat, online users, and voice room join/leave.
- `light-discord-platform`: Windows/Linux-specific boundary for audio, notification, and packaging work.

The current voice implementation has room state and UDP relay plumbing. Microphone capture, speaker playback, Opus encoding, jitter buffering, mute/deafen, and device selection are the next production voice tasks.

Chat messages are persisted when `LD_DATABASE_URL` points at PostgreSQL. User-deleted messages are hidden from normal channel history and written to the admin-only audit log with a body snapshot. Visible chat history and audit log retention default to 30 days.

Authentication is enforced when PostgreSQL is configured. The server supports invite registration, password login, session resume, admin invite creation, and admin audit log reads. In memory-only development mode, dev login is enabled by default; set `LD_DEV_AUTH=0` to disable it.

## Run

Install Rust stable. This workspace was verified with Rust 1.95.0.

Start the server:

```bash
cargo run -p light-discord-server
```

Start one or more clients:

```bash
cargo run -p light-discord-client
```

Default ports:

- Chat/control TCP: `127.0.0.1:41610`
- Voice relay UDP: `127.0.0.1:41611`

Override them with `LD_TCP_BIND` and `LD_UDP_BIND`.

Run with PostgreSQL for self-hosted testing:

```bash
cd deploy
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='share-this-once'
docker compose up --build
```

For a locally running PostgreSQL instance, set:

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
export LD_BOOTSTRAP_ADMIN_NAME=admin
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
cargo run -p light-discord-server
```

If `LD_DATABASE_URL` is not set, the server uses in-memory storage for development only.

PostgreSQL must be running somewhere the server can reach. It can be on the same host, another server, Docker Compose, or a managed PostgreSQL service. For a same-host Linux setup, use:

```bash
export LD_PG_DB=light_discord
export LD_PG_USER=light_discord
export LD_PG_PASSWORD='replace-with-a-long-random-password'
scripts/setup-postgres-linux.sh
```

Then start the server with the printed `LD_DATABASE_URL`.

The setup script accepts `LD_PG_*` for convenience. When it needs to re-run itself with `sudo`, it copies those values to `LIGHT_DISCORD_PG_*` first because sudo commonly strips `LD_*` variables for dynamic linker safety.

If PostgreSQL is not running on the default port, the script detects the active Debian/Ubuntu cluster port with `pg_lsclusters`. You can also force a port with `LD_PG_PORT`.

Manual package install examples:

```bash
# Debian / Ubuntu
sudo apt-get update
sudo apt-get install -y postgresql postgresql-client

# Fedora / RHEL-like
sudo dnf install -y postgresql-server postgresql-contrib
sudo postgresql-setup --initdb
sudo systemctl enable --now postgresql

# openSUSE
sudo zypper --non-interactive install postgresql-server postgresql-contrib
sudo systemctl enable --now postgresql
```

Check a configured database URL:

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
scripts/check-postgres.sh
```

## How To Use

For quick local development without PostgreSQL:

```bash
cargo run -p light-discord-server
cargo run -p light-discord-client
```

In the client, choose `Dev`, enter a display name, and press `Connect`. This mode is for local development only. It is enabled by default only when `LD_DATABASE_URL` is not set.

For self-hosted use with accounts:

1. Start the server with PostgreSQL and bootstrap credentials:

```bash
export LD_DATABASE_URL=postgres://light_discord:your-password@localhost:5432/light_discord
export LD_BOOTSTRAP_ADMIN_NAME=admin
export LD_BOOTSTRAP_ADMIN_PASSWORD='change-this-password'
export LD_BOOTSTRAP_INVITE_CODE='first-friend-invite'
export LD_DEV_AUTH=0
cargo run -p light-discord-server
```

2. Start the client:

```bash
cargo run -p light-discord-client
```

3. As the first admin, choose `Login`, use `admin` and `change-this-password`, then press `Connect`.

4. To invite a friend, use the admin panel in the left sidebar. Enter an invite note if needed, press `Invite`, and share the generated invite code.

5. A friend chooses `Register`, enters the invite code, display name, and password, then connects.

6. After registration, use `Login` with display name and password. The server returns a session token; the current client keeps it in memory for `Session` mode after disconnect.

7. To inspect deleted-message audit records, connect as admin and press `Audit` in the admin panel.

Current limitations:

- Session tokens are not persisted to disk by the client yet.
- Account management, password reset, role management, TLS setup, and production voice are still future work.
- The client UI is intentionally minimal and aimed at validating the backend flow first.

Run PostgreSQL integration tests when a database is available:

```bash
export LD_TEST_DATABASE_URL=postgres://light_discord:light_discord_dev_password@localhost:5432/light_discord
cargo test -p light-discord-storage --test postgres
```

When `LD_TEST_DATABASE_URL` is not set, the integration test exits successfully without touching a database. This is intentional for Docker-in-Docker environments that do not expose a Docker CLI or database service.

## Docs

- [Codex handoff](CODEX_HANDOFF.md)
- [Architecture](docs/architecture.md)
- [Development operations](docs/development-operations.md)
- [How to use](docs/how-to-use-ja.md)
- [Requirements](docs/requirements-ja.md)
- [Requirements checklist](docs/requirements-checklist.md)
