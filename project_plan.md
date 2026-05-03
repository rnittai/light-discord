# Project Plan

## Goal

Create a lightweight Discord-like application in Rust for Windows and Linux without Chromium or Electron. The first milestone is a native desktop client, shared protocol crate, chat/control server, UDP voice relay, explicit OS-specific abstraction points, and service-ready persistence/auth boundaries.

## Milestone 1: Scaffolded MVP

- Create a Cargo workspace with separate core, server, client, and platform crates.
- Add auth and storage crates so friend-use self-hosted mode can grow into a hosted service.
- Implement newline-delimited JSON over TCP for login, chat, presence, and voice room control.
- Implement UDP voice packet relay and room membership state.
- Build a native `egui`/`eframe` client for server connection, text chat, user list, and voice room join/leave.
- Isolate OS-specific audio/notification/packaging concerns in `light-discord-platform`.
- Persist chat messages when `LD_DATABASE_URL` is configured.
- Soft-delete user messages and write deleted-message body snapshots to admin-only audit logs.
- Enforce register/login/session flows for PostgreSQL deployments.
- Keep development display-name login only for local memory storage.
- Add admin-only audit log read and invite creation protocol operations.
- Document requirement questions before production work.

## Milestone 2: Production Voice

- Add `cpal` microphone capture and speaker playback behind `AudioBackend`.
- Encode/decode voice packets with Opus.
- Add jitter buffer, packet loss handling, mute/deafen, input device selection, and output device selection.

## Milestone 3: Real Product Features

- Add TLS, channel permissions, moderation, native notifications, OS packages, and hosted-service operational tooling.
