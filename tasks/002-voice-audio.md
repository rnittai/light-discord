# Task 002: Production Voice Audio

Task ID: 002-voice-audio

Goal: Replace the no-op audio boundary with real capture/playback.

Status: pending

Expected changes:
- Add a `cpal` backend under `light-discord-platform`.
- Add Opus encode/decode and jitter buffering.
- Wire audio frames into the existing UDP relay path.

Verification commands:
- `cargo check --workspace --features audio`
- Manual two-client voice test on Windows and Linux.

Risk notes:
- Linux audio behavior differs between PipeWire, PulseAudio, and ALSA.
- Echo cancellation/noise suppression may require additional native libraries.

