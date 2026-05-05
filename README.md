# light-discord

A lightweight Discord-like Rust application scaffold for Windows and Linux. It uses a native desktop UI through `egui`/`eframe` and does not use Chromium or Electron.

## What Exists

- `light-discord-core`: shared protocol types for chat, presence, voice packets, and screen sharing.
- `light-discord-auth`: password hashing, invite code, and session token primitives.
- `light-discord-storage`: PostgreSQL persistence and development memory storage.
- `light-discord-server`: TCP chat/control server and UDP voice packet relay.
- `light-discord-client`: native desktop client with connection, channel chat, online users, voice room join/leave, and screen sharing.
- `light-discord-platform`: Windows/Linux-specific boundary for audio, screen capture, notifications, and packaging work.

The current voice implementation has room state, UDP relay plumbing, native input/output device selection, and a production-style voice path: captured microphone audio is resampled to 48 kHz mono, run through a high-pass + RMS noise gate + cheap mic-ducking DSP path. Frames where the noise gate is closed are not transmitted (suppressed at the source). Open-gate frames are encoded as 20 ms Opus frames (with in-band FEC enabled) and sent through the UDP relay as `VoicePacket`s with `codec="opus"`. On the receiving side a per-remote jitter buffer uses Opus PLC and FEC to mask packet loss. Mute and deafen toggles in the client gate the microphone and remote playback respectively, while still keeping the voice-room heartbeat alive so the UDP relay continues to learn the client's address. The voice-user list highlights the active speaker(s) — including the local user — only for actually transmitted/audible frames.

This is voice quality suitable for friends-only deployments, not a Discord-replacement-grade stack: there is no real acoustic echo cancellation (only a simple mic-ducking heuristic), no adaptive bitrate, no SRTP/encryption, and no Opus DTX (silence suppression relies on the RMS noise gate, not the codec). libopus is built and linked statically through `audiopus_sys`'s `static` feature so Windows and Linux builds do not require a system libopus.

Chat messages are persisted when `LD_DATABASE_URL` points at PostgreSQL. User-deleted messages are hidden from normal channel history and written to the admin-only audit log with a body snapshot. Visible chat history and audit log retention default to 30 days.

Authentication is enforced when PostgreSQL is configured. The server supports invite registration, password login, session resume, admin invite creation, and admin audit log reads. In memory-only development mode, dev login is enabled by default; set `LD_DEV_AUTH=0` to disable it.

## Run

Install Rust stable. This workspace was verified with Rust 1.95.0.

Linux builds need ALSA development headers (for `cpal`), xcap screen capture
dependencies (for display/window enumeration), and CMake plus a C toolchain (for
the bundled libopus that `audiopus_sys` builds statically).
Run the setup script to install them automatically:

```bash
scripts/setup-linux-dev-deps.sh
```

The script detects your distro, installs the packages with sudo if needed, and verifies the result.
If you prefer to install manually, the packages are:

| Distro | Command |
|--------|---------|
| Debian / Ubuntu | `sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev cmake build-essential libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev` |
| Fedora / RHEL / CentOS / Rocky / Alma | `sudo dnf install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make clang-devel libxcb-devel libXrandr-devel dbus-devel pipewire-devel wayland-devel mesa-libEGL-devel mesa-libgbm-devel libxkbcommon-devel` |
| Arch / Manjaro | `sudo pacman -Sy --needed --noconfirm pkgconf alsa-lib cmake base-devel clang libxcb libxrandr dbus pipewire wayland mesa libxkbcommon` |
| openSUSE / SLES | `sudo zypper --non-interactive install pkgconf-pkg-config alsa-devel cmake gcc gcc-c++ make clang-devel libxcb-devel libXrandr-devel dbus-1-devel pipewire-devel wayland-devel Mesa-libEGL-devel Mesa-libgbm-devel libxkbcommon-devel` |

Windows builds need only the standard MSVC or GNU toolchain that the Rust
installer sets up; CMake comes bundled with most Visual Studio installs and
`audiopus_sys` will use it to build libopus from source.

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

6. After registration, use `Login` with display name and password. The server returns a session token; the client saves it persistently for `Session` mode.

7. To inspect deleted-message audit records, connect as admin and press `Audit` in the admin panel.

Voice device selection and controls:

- The `Voice` section lists `Input` and `Output` devices discovered through `cpal`.
- Use `Refresh` after plugging in or removing an audio device.
- `Join` starts the current voice room. The worker downmixes capture to mono, resamples to 48 kHz, runs a high-pass filter and RMS noise gate. Frames where the noise gate is closed are suppressed and not transmitted (Opus DTX is not used). Open-gate frames are encoded as 20 ms Opus frames with in-band FEC enabled and sent over UDP as binary-encoded `VoicePacket`s (via `encode_voice_packet_binary`). Incoming binary datagrams are decoded with `decode_voice_packet_binary`, routed through a per-remote jitter buffer (~60 ms target depth), and Opus-decoded with PLC/FEC for masking packet loss.
- `Mute mic` stops outgoing audio while keeping the voice-room heartbeat. `Deafen` stops remote playback (and implicitly mutes the mic).
- The voice user list shows a green `*` marker and name for users currently emitting audible audio, including yourself.

Screen sharing:

- The `Screen` section lists available displays and non-minimized windows.
- Use `Refresh` to update the list when displays or windows change.
- Select a display or window, press `Share` to start broadcasting your screen.
- Remote screen shares appear in the central panel above chat.
- Press `Stop` to end the broadcast.
- MVP transport sends downscaled JPEG frames over the existing TCP JSON control connection using base64 encoding. This is for friend/self-hosted validation, not production video. Future production work should move to a dedicated binary/video transport with better codec, rate control, and encryption.

Session token storage:

- The client saves session tokens after successful Login or Register when the server returns one.
- On startup, the client loads the saved token for the default server (127.0.0.1:41610) and auto-selects `Session` mode if found.
- In `Session` mode, `Load` and `Forget` buttons manage the token for the current server address.
- Storage is per-server using a SHA-256-derived key (raw address is not used as a filename/key).
- Preferred storage uses OS credential stores: Windows Credential Store on Windows; Linux keyutils + Secret Service when available.
- If keyring is unavailable, the client falls back to a restricted local file: Linux `$XDG_CONFIG_HOME/light-discord/session-tokens` or `$HOME/.config/light-discord/session-tokens`; Windows `%APPDATA%\LightDiscord\session-tokens` or `%USERPROFILE%\AppData\Roaming\LightDiscord\session-tokens`. Set `LIGHT_DISCORD_CONFIG_DIR` to override the root for tests/dev. Unix files are written with permissions `0600`.

Current limitations:

- Account management, password reset, role management, TLS setup are still future work.
- There is no SRTP/encryption, no Opus DTX (closed-gate frames are suppressed by the RMS noise gate at the source, not by the codec), no adaptive bitrate, and no real acoustic echo cancellation — only a simple mic-ducking heuristic that attenuates the microphone when remote playback is loud. The voice path is fine for friend-group calls but is not Discord-grade.
- Screen sharing over TCP using base64-encoded JPEG is MVP-only for friend/self-hosted validation; production use requires a dedicated binary/video transport.
- The client UI is intentionally minimal and aimed at validating the backend flow first.
- Screen capture and display require a graphical session; Docker containers without X11/Wayland forwarding cannot capture or display screens, though compilation and unit tests work normally.

Japanese text rendering:

The native client tries to load a Japanese-capable system font at startup. If Japanese text appears garbled, install a Japanese font or point the client at one explicitly:

```bash
# Debian / Ubuntu
sudo apt-get install -y fonts-noto-cjk

# Fedora
sudo dnf install -y google-noto-sans-cjk-fonts

# Arch
sudo pacman -S noto-fonts-cjk

# Explicit font file override
export LIGHT_DISCORD_FONT_PATH=/path/to/NotoSansCJK-Regular.ttc
cargo run -p light-discord-client
```

On Windows, the client checks common system fonts such as Meiryo and Yu Gothic under `C:\Windows\Fonts`.

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
