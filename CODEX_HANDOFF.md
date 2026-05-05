# Codex Handoff

Last updated: 2026-05-05.

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

## AI Workflow Preference

The user wants Codex to handle overall design, task decomposition, sequencing, review, verification decisions, and integration. Actual code-writing implementation work should be delegated to Claude Code workers when the tooling is available. When a request spans multiple implementation areas, split it into appropriately small Claude Code prompts and restart Claude Code between prompts to reset context and reduce token usage. Git operations should also be delegated to Claude Code when practical, including `git status`, `git add`, `git commit`, and `git push`; Codex should decide the checkpoint/scope and review the resulting repository state. Select the Claude Code model according to the task complexity rather than using one fixed model for every implementation task.

If Claude Code delegation is blocked by the environment, document the blocker clearly before falling back to direct implementation. Codex should still review diffs, run relevant checks, and preserve task-sized commits/pushes.

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

- Voice now runs through Opus 48 kHz mono 20 ms frames (960 samples per packet) with in-band FEC enabled and a `set_packet_loss_perc` hint of 10%. libopus is built statically via `audiopus_sys`'s `static` feature so no system libopus is required on Windows or Linux.
- UDP voice payloads are encoded with `encode_voice_packet_binary` and decoded with `decode_voice_packet_binary` (defined in `light-discord-core`). The binary format has a magic/version header, length-prefixed `user_id` and `room_id`, then `sequence`/`sample_rate`/`channels`/`codec`/`frame_samples` fixed fields, followed by the raw Opus payload bytes. The server parses the binary header to extract `user_id`/`room_id` for room routing and forwards the original binary datagram unchanged; it does not decode the Opus payload. TCP chat/control remains newline-delimited JSON.
- `CpalAudioBackend` (in `light-discord-platform`) still owns the cpal capture/playback streams. Capture is downmixed to mono. Playback resamples and channel-adapts incoming 48 kHz mono frames to whatever the output device wants.
- `light-discord-platform/src/voice.rs` provides the pure DSP/protocol helpers: a linear mono resampler, a single-pole high-pass filter, an RMS noise gate with hangover, a per-remote-user `JitterBuffer` that emits packets in sequence and reports `JitterPop::Lost { next_payload }` so the decoder can use Opus FEC, plus the cheap mic-ducking helper. All of these have unit tests.
- The client voice worker (`crates/light-discord-client/src/voice.rs`) wires capture -> highpass -> noise gate -> echo duck -> Opus encode -> UDP send, and UDP recv -> jitter buffer -> Opus decode (with PLC/FEC for losses) -> playback. Closed-gate frames are suppressed and not transmitted (Opus DTX is not used). Heartbeat packets carry the current sequence number but do not advance it; only transmitted audio frames increment the sequence. A `VoiceShared` (`Arc<...>`) carries the mute/deafen toggles and the active-speaker timestamps shared with the UI; active-speaker is only marked for actually transmitted frames.
- The egui client now exposes `Mute mic` and `Deafen` toggles in the Voice panel and highlights the active speaker(s) in the voice user list — including the local user.
- Limitations: simple mic-ducking is *not* AEC. There is no SRTP, no DTX, no adaptive bitrate. The server reads the binary header for routing and relays the datagram unchanged; it does not decode Opus payloads.

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
- Select `Voice` `Input` / `Output` devices in the left sidebar if audio devices are available.

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

- No password reset or account management UI.
- No roles/permissions UI beyond basic admin flag.
- No TLS/reverse proxy automation yet.
- Voice runs Opus 48 kHz mono 20 ms with PLC/FEC and a jitter buffer; mute/deafen toggles and active-speaker highlighting are wired up. Closed-gate frames are suppressed at the source (Opus DTX is not used). Echo handling is a simple ducking heuristic, not full AEC. UDP datagrams use the binary codec (`encode_voice_packet_binary`/`decode_voice_packet_binary`). No SRTP/encryption, no adaptive bitrate.
- Native packaging for Windows/Linux has not been implemented.
- Docker CLI is not installed in the current container, though Docker Compose files exist for host-side use.

## Japanese Font Rendering

Japanese text initially appeared garbled because egui's default fonts do not cover Japanese glyphs. The client now loads a Japanese-capable system font at startup.

Implementation:

- `crates/light-discord-client/src/fonts.rs` discovers and registers Japanese fonts.
- `LIGHT_DISCORD_FONT_PATH` can force a specific `.ttf`, `.otf`, or `.ttc` file.
- Linux candidates include Noto CJK, IPA, and Takao font paths.
- Windows candidates include Meiryo, Yu Gothic, and MS Gothic under `C:\Windows\Fonts`.

This Docker container has `fonts-noto-cjk` installed for verification, and the detected path is:

```text
/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc
```

## Voice Device Selection

The client now enumerates native audio devices through `cpal`:

- `crates/light-discord-platform/src/audio.rs` exposes `available_audio_devices`, `AudioDeviceList`, `AudioDeviceSelection`, and the new `CpalAudioBackend` that implements the `AudioBackend` trait. `default-input` / `default-output` aliases route to the system default device; concrete ids are parsed via `cpal::DeviceId::from_str` with a string-comparison fallback.
- Pure helpers in the same file (`adapt_channels`, `cap_playback_queue`, `encode_pcm_le`, `decode_pcm_le`) are unit tested.
- `crates/light-discord-client/src/app.rs` shows `Input` and `Output` combo boxes in the `Voice` section.
- `crates/light-discord-client/src/voice.rs` runs the actual voice MVP. Its worker thread owns a `CpalAudioBackend`, opens a UDP socket with a 20 ms read timeout, resamples captured audio to 48 kHz and slices it into 20 ms Opus frames (960 samples), encodes each with the Opus encoder, and sends it as a `VoicePacket`. Received Opus packets from remote users are fed into per-user jitter buffers, then Opus-decoded with PLC or in-band FEC for losses, and sent to the playback queue. Own echoed packets and empty heartbeats are discarded. An empty heartbeat packet is sent every 500 ms when nothing else has gone out so the relay still learns this client's address.

Linux build dependencies installed in this Docker container for verification:

```bash
apt-get install -y libasound2-dev cmake build-essential
```

`cmake` and a C/C++ toolchain are required because `audiopus_sys` builds the
bundled libopus from source when the `static` feature is enabled.

A setup script handles this automatically on developer machines:

```bash
scripts/setup-linux-dev-deps.sh
```

The script supports Debian/Ubuntu, Fedora/RHEL/CentOS/Rocky/Alma, Arch/Manjaro, and openSUSE/SLES.
It re-executes the install command via sudo when not running as root, then verifies that
`pkg-config --exists alsa`, `cmake`, and a C compiler are all present before exiting.
README.md and docs/how-to-use-ja.md both point to this script as the recommended first
step for Linux builds.

## Session Token Persistence

The client now persists session tokens through `light-discord-platform`:

- `crates/light-discord-platform/src/session_token.rs` exposes `load_session_token`,
  `save_session_token`, and `delete_session_token`.
- Tokens are stored per server address using a SHA-256-derived key. The raw server
  address is not used as a keyring username or fallback filename.
- Windows uses Windows Credential Store through `keyring` when available.
- Linux uses keyutils + Secret Service persistent storage through `keyring` when
  available.
- If keyring storage is unavailable, the fallback file lives under
  `$XDG_CONFIG_HOME/light-discord/session-tokens` or
  `$HOME/.config/light-discord/session-tokens` on Linux, and
  `%APPDATA%\LightDiscord\session-tokens` or
  `%USERPROFILE%\AppData\Roaming\LightDiscord\session-tokens` on Windows.
  `LIGHT_DISCORD_CONFIG_DIR` overrides the root for tests and development.
- Unix fallback token files are written with mode `0600`.
- `crates/light-discord-client/src/app.rs` loads the default server token at startup,
  auto-selects `Session` mode when one is found, saves tokens returned by Login/Register,
  and provides `Load` / `Forget` controls in Session mode.

## Good Next Tasks

Suggested next development steps:

1. Add a small admin/account management UI.
2. Screen sharing with full-screen and window capture selection.
3. Add role/channel permission model.
4. Add TLS/reverse-proxy deployment guide for self-hosting.
5. Real AEC (with playback reference signal), DTX, and adaptive bitrate.
6. Add PostgreSQL cleanup/migration tests and reset helpers for local DB.
7. Add packaging scripts for Windows and Linux.

## Git Workflow Going Forward

The user wants Codex to commit and push autonomously at appropriate checkpoints.

Recommended behavior:

- Inspect `git status` before changes.
- Keep commits coherent and task-sized.
- Run relevant verification before committing.
- Ask Claude Code to run git operations when available, including commit and push.
- Use a concise commit message.
- Push to the configured remote when successful.
- If push fails, keep the local commit and report the exact failure.
