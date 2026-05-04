# Architecture

Light Discord is split into four Rust crates so Windows and Linux support can share most logic without hiding OS-specific behavior.

## Common Rust Crates

- `light-discord-core`: shared protocol types for chat, presence, voice room state, and UDP voice packets.
- `light-discord-auth`: password hashing, invite code, and session token primitives for invite-only accounts.
- `light-discord-storage`: PostgreSQL-backed persistence boundary plus a memory backend for local development and tests.
- `light-discord-server`: TCP newline-delimited JSON chat/control server plus UDP voice packet relay.
- `light-discord-client`: native desktop UI using `egui`/`eframe`; no Chromium or Electron.

## Platform-Specific Boundary

- `light-discord-platform`: OS capability boundary for audio, notifications, packaging hints, and future native integrations.
- `src/os/linux.rs`: Linux-specific integration notes for PipeWire/PulseAudio/ALSA, freedesktop notifications, and packaging.
- `src/os/windows.rs`: Windows-specific integration notes for WASAPI, toast notifications, and MSI/portable packaging.

The current voice path implements room membership, UDP packet relay, and `cpal` device enumeration for native input/output selection. The audio backend trait is present, and the default implementation is a no-op so the workspace can be developed without committing to a codec or stream pipeline too early. A production voice implementation should add `cpal` capture/playback streams, Opus encoding, and jitter buffering behind this boundary.

## Persistence and Audit Boundary

`light-discord-storage` owns message retention, soft deletion, and audit logging. The server calls this crate instead of writing SQL directly. This keeps the self-hosted MVP simple while leaving room to move to managed PostgreSQL, sharded storage, or a service boundary later.

Visible chat history is capped at 30 days by default. User-deleted messages are hidden from channel history but copied into `audit_log` with the original body snapshot, actor, target user, channel, and timestamp.

`light-discord-auth` owns password hashing and token hashing. `light-discord-server` performs protocol-level authentication, but it stores account and session state through `light-discord-storage`.

Authentication modes:

- `Register`: invite code + display name + password.
- `Login`: display name + password.
- `ResumeSession`: previously issued session token.
- `Hello`: development-only display-name login. Enabled by default only when the server runs without PostgreSQL.

Admin-only protocol:

- `AdminCreateInvite`: creates a one-time invite code.
- `AdminListAuditLog`: returns recent audit events.

## Runtime Ports

- TCP chat/control: `127.0.0.1:41610` by default, override with `LD_TCP_BIND`.
- UDP voice relay: `127.0.0.1:41611` by default, override with `LD_UDP_BIND`.
