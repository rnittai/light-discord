# Architecture

Light Discord is split into four Rust crates so Windows and Linux support can share most logic without hiding OS-specific behavior.

## Common Rust Crates

- `light-discord-core`: shared protocol types for chat, presence, voice room state, and UDP voice packets.
- `light-discord-auth`: password hashing, invite code, and session token primitives for invite-only accounts.
- `light-discord-storage`: PostgreSQL-backed persistence boundary plus a memory backend for local development and tests.
- `light-discord-server`: TCP newline-delimited JSON chat/control server plus UDP voice packet relay. The server parses the binary voice header (via `decode_voice_packet_binary`) to read `user_id`/`room_id` for routing, then forwards the original binary datagram unchanged without decoding the Opus payload.
- `light-discord-client`: native desktop UI using `egui`/`eframe`; no Chromium or Electron.

## Platform-Specific Boundary

- `light-discord-platform`: OS capability boundary for audio, screen capture, notifications, packaging hints, and future native integrations.
- `src/os/linux.rs`: Linux-specific integration notes for PipeWire/PulseAudio/ALSA, xcap screen capture, freedesktop notifications, and packaging.
- `src/os/windows.rs`: Windows-specific integration notes for WASAPI, xcap screen capture, toast notifications, and MSI/portable packaging.

The current voice path implements room membership, binary-framed UDP packet relay, and `cpal` device enumeration for native input/output selection. UDP voice payloads are encoded and decoded with `encode_voice_packet_binary`/`decode_voice_packet_binary` (magic/version header, length-prefixed `user_id`/`room_id`, sequence/sample_rate/channels/codec/frame_samples fields, and raw payload bytes). TCP chat/control remains newline-delimited JSON. The audio backend trait is present; `CpalAudioBackend` in `light-discord-platform` provides the full Opus 48 kHz capture/playback pipeline with jitter buffering.

Screen sharing is implemented as an SFU-style MVP relay over the existing TCP JSON control connection. Platform capture uses `xcap` on Linux/Windows to enumerate displays and non-minimized windows. The client sends one downscaled frame stream to the server, and the server selectively forwards `ScreenShareFrame` messages to other connected clients without echoing frames back to the sender. The protocol carries mode (`text` or `game`), resolution cap (`1080p` or `720p`), target FPS, requested codec preferences, negotiated active codec, and transport metadata. Clients advertise AV1/VP9/JPEG preferences, but the current native encoder path only produces JPEG frames, so the server negotiates JPEG as the active fallback until a real AV1/VP9 video encoder and binary media transport are added. Text mode uses low FPS and higher JPEG quality for readability; game mode allows 30 FPS or 60 FPS with lower JPEG quality. This MVP is suitable for friend/self-hosted validation but not production use; future work should move to a dedicated binary/video transport with proper codec, rate control, and encryption support.

## Persistence and Audit Boundary

`light-discord-storage` owns message retention, soft deletion, and audit logging. The server calls this crate instead of writing SQL directly. This keeps the self-hosted MVP simple while leaving room to move to managed PostgreSQL, sharded storage, or a service boundary later.

Visible chat history is capped at 30 days by default. User-deleted messages are hidden from channel history but copied into `audit_log` with the original body snapshot, actor, target user, channel, and timestamp.

`light-discord-auth` owns password hashing and token hashing. `light-discord-server` performs protocol-level authentication, but it stores account and session state through `light-discord-storage`.

Authentication modes:

- `Register`: invite code + display name + password.
- `Login`: display name + password.
- `ResumeSession`: previously issued session token.
- `Hello`: development-only display-name login. Enabled by default only when the server runs without PostgreSQL.

Session Token Persistence:

The client saves session tokens after successful Login or Register. On startup, it loads the saved token for the default server (127.0.0.1:41610) and auto-selects `Session` mode if found. Storage is per-server using a SHA-256-derived key. The preferred storage uses OS credential stores (Windows Credential Store, Linux keyutils + Secret Service when available); fallback is a restricted local file with mode `0600` on Unix. Paths: Linux `$XDG_CONFIG_HOME/light-discord/session-tokens` or `$HOME/.config/light-discord/session-tokens`; Windows `%APPDATA%\LightDiscord\session-tokens` or `%USERPROFILE%\AppData\Roaming\LightDiscord\session-tokens`. Override the root with `LIGHT_DISCORD_CONFIG_DIR` for tests/dev.

Admin-only protocol:

- `AdminCreateInvite`: creates a one-time invite code.
- `AdminListAuditLog`: returns recent audit events.

Screen sharing protocol:

- `ScreenShareStarted`: sent by server to other connected clients when a client starts broadcasting; includes the broadcasting user's ID, selected mode, resolution cap, FPS target, requested codecs, negotiated codec, and relay transport.
- `ScreenShareStopped`: sent by server to other connected clients when a client stops broadcasting.
- `ScreenShareFrame`: contains a base64-encoded JPEG frame today; sent by the sharing client and relayed by the server to other connected clients. The server validates dimensions, transport, and negotiated codec metadata, but does not decode the frame body.

## Runtime Ports

- TCP chat/control: `127.0.0.1:41610` by default, override with `LD_TCP_BIND`.
- UDP voice relay: `127.0.0.1:41611` by default, override with `LD_UDP_BIND`.
