use light_discord_core::{
    decode_voice_packet_binary, encode_voice_packet_binary, VoicePacket, VOICE_CODEC_OPUS,
};
use light_discord_platform::{
    drain_frames, duck_mic_against_remote, frame_rms, resample_linear_mono, AudioBackend,
    AudioDeviceSelection, AudioFrame, CpalAudioBackend, HighPassFilter, JitterBuffer, JitterPop,
    NoiseGate, OPUS_FRAME_SAMPLES, OPUS_MAX_PACKET_BYTES, OPUS_SAMPLE_RATE,
};
use opus::{Application, Channels, Decoder, Encoder};
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How often we send an empty heartbeat packet so the relay learns our address
/// even when the microphone is muted or unavailable.
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);
/// Read timeout used to keep the receive loop responsive to stop signals.
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(20);
/// How long after the last detected speech a user is still considered the
/// "active speaker" in the UI. Prevents flicker between syllables.
const ACTIVE_HOLD: Duration = Duration::from_millis(350);
/// Jitter buffer target depth in 20 ms packets (~60 ms of pre-roll).
const JITTER_TARGET_DEPTH: usize = 3;
/// Jitter buffer hard cap (~400 ms before we start dropping the oldest).
const JITTER_MAX_DEPTH: usize = 20;
/// Tell the Opus encoder to anticipate this much packet loss when configuring
/// FEC. Higher values make Opus reserve more bits for redundancy.
const OPUS_EXPECTED_PACKET_LOSS_PCT: i32 = 10;
/// Opus encoder bitrate target (bits/s). 32 kbps is plenty for mono speech and
/// keeps headroom for FEC overhead in the binary UDP envelope.
const OPUS_BITRATE_BPS: i32 = 32_000;

/// Open the noise gate at this RMS amplitude (i16 units, 0..32768).
const NOISE_GATE_OPEN_RMS: f32 = 350.0;
/// Close the noise gate when the signal drops below this amplitude.
const NOISE_GATE_CLOSE_RMS: f32 = 150.0;
/// Hangover length: keep the gate open for ~6 frames (120 ms) after the last
/// loud frame so word tails are not chopped.
const NOISE_GATE_HANGOVER_FRAMES: u32 = 6;

/// Below this remote-playback RMS we don't bother attenuating the mic.
const ECHO_DUCK_THRESHOLD_RMS: f32 = 600.0;
/// Mic gain while remote audio is loud. 0.5 = -6 dB attenuation.
const ECHO_DUCK_GAIN: f32 = 0.5;

enum VoiceCommand {
    Stop,
}

/// Runtime-mutable state shared between the voice worker thread and the UI.
///
/// The UI updates `muted` and `deafened`; the worker updates the active-speaker
/// table. Both sides are short-lived locks (one HashMap insert / read).
#[derive(Default)]
pub struct VoiceShared {
    pub muted: AtomicBool,
    pub deafened: AtomicBool,
    /// Last instant each user emitted audible audio, including the local
    /// user (the worker marks the local user_id active whenever a frame
    /// passes the noise gate).
    speakers: Mutex<HashMap<String, Instant>>,
}

impl VoiceShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.deafened.store(deafened, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened.load(Ordering::Relaxed)
    }

    /// Returns true when `user_id` was recorded as active within the hold
    /// window. Stale entries are not pruned here because the map stays small.
    pub fn is_active(&self, user_id: &str) -> bool {
        let Ok(guard) = self.speakers.lock() else {
            return false;
        };
        match guard.get(user_id) {
            Some(t) => t.elapsed() < ACTIVE_HOLD,
            None => false,
        }
    }

    fn mark_active(&self, user_id: &str) {
        if let Ok(mut guard) = self.speakers.lock() {
            guard.insert(user_id.to_owned(), Instant::now());
        }
    }
}

#[derive(Default)]
pub struct VoiceSession {
    stop_tx: Option<Sender<VoiceCommand>>,
    worker: Option<JoinHandle<()>>,
    selected_devices: AudioDeviceSelection,
    shared: Option<Arc<VoiceShared>>,
}

impl VoiceSession {
    pub fn start(
        &mut self,
        udp_addr: String,
        user_id: String,
        room_id: String,
        selected_devices: AudioDeviceSelection,
        shared: Arc<VoiceShared>,
    ) {
        self.stop();
        self.selected_devices = selected_devices.clone();
        self.shared = Some(Arc::clone(&shared));

        let (stop_tx, stop_rx) = mpsc::channel::<VoiceCommand>();
        let worker = thread::spawn(move || {
            run_voice_worker(
                udp_addr,
                user_id,
                room_id,
                selected_devices,
                stop_rx,
                shared,
            );
        });

        self.stop_tx = Some(stop_tx);
        self.worker = Some(worker);
    }

    pub fn is_running(&self) -> bool {
        self.stop_tx.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(VoiceCommand::Stop);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.selected_devices = AudioDeviceSelection::default();
        self.shared = None;
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Per-remote-user decoder + jitter buffer + activity tracker.
struct RemoteVoice {
    decoder: Decoder,
    jitter: JitterBuffer,
}

impl RemoteVoice {
    fn new() -> Result<Self, opus::Error> {
        let decoder = Decoder::new(OPUS_SAMPLE_RATE, Channels::Mono)?;
        Ok(Self {
            decoder,
            jitter: JitterBuffer::new(JITTER_TARGET_DEPTH, JITTER_MAX_DEPTH),
        })
    }
}

fn run_voice_worker(
    udp_addr: String,
    user_id: String,
    room_id: String,
    selected_devices: AudioDeviceSelection,
    stop_rx: mpsc::Receiver<VoiceCommand>,
    shared: Arc<VoiceShared>,
) {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(err) => {
            eprintln!("voice worker: failed to bind UDP socket: {err}");
            return;
        }
    };
    if let Err(err) = socket.connect(&udp_addr) {
        eprintln!("voice worker: failed to connect to {udp_addr}: {err}");
        return;
    }
    if let Err(err) = socket.set_read_timeout(Some(SOCKET_READ_TIMEOUT)) {
        eprintln!("voice worker: failed to set socket read timeout: {err}");
    }

    let mut encoder = match build_encoder() {
        Ok(enc) => enc,
        Err(err) => {
            eprintln!("voice worker: failed to create Opus encoder: {err}");
            return;
        }
    };

    let (capture_tx, capture_rx) = mpsc::channel::<AudioFrame>();
    let (playback_tx, playback_rx) = mpsc::channel::<AudioFrame>();

    let mut backend = CpalAudioBackend::new();
    if let Err(err) = backend.start_capture(
        selected_devices.input_device_id.as_deref(),
        capture_tx.clone(),
    ) {
        eprintln!("voice worker: capture stream unavailable: {err:#}");
    }
    if let Err(err) =
        backend.start_playback(selected_devices.output_device_id.as_deref(), playback_rx)
    {
        eprintln!("voice worker: playback stream unavailable: {err:#}");
    }
    drop(capture_tx);

    let mut sequence: u64 = 0;
    let mut capture_buffer: Vec<i16> = Vec::new();
    let mut last_heartbeat = Instant::now();
    let mut recv_buf = vec![0_u8; 64 * 1024];

    let mut highpass = HighPassFilter::new(OPUS_SAMPLE_RATE, 100.0);
    let mut noise_gate = NoiseGate::new(
        NOISE_GATE_OPEN_RMS,
        NOISE_GATE_CLOSE_RMS,
        NOISE_GATE_HANGOVER_FRAMES,
    );
    // Cross-frame estimate of how loud remote playback was recently. Used to
    // attenuate mic input as a poor man's echo suppressor.
    let mut last_remote_rms: f32 = 0.0;

    let mut remotes: HashMap<String, RemoteVoice> = HashMap::new();

    loop {
        if matches!(stop_rx.try_recv(), Ok(VoiceCommand::Stop)) {
            break;
        }

        // ===== Capture path =====
        loop {
            match capture_rx.try_recv() {
                Ok(frame) => {
                    // Capture is already mono i16 (the cpal backend downmixes
                    // to mono). Resample to 48 kHz so Opus has a fixed input.
                    let resampled = if frame.sample_rate == OPUS_SAMPLE_RATE {
                        frame.pcm
                    } else {
                        resample_linear_mono(&frame.pcm, frame.sample_rate, OPUS_SAMPLE_RATE)
                    };
                    capture_buffer.extend(resampled);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        // Slice into 20 ms Opus frames (960 samples mono @ 48 kHz).
        let frames = drain_frames(&mut capture_buffer, OPUS_FRAME_SAMPLES);
        for mut frame in frames {
            // Always fade out DC offset, even when muted, so the gate state
            // stays continuous when mute is toggled off again.
            highpass.process(&mut frame);

            let muted = shared.is_muted();
            let (gate_open, _rms) = noise_gate.process(&frame);

            // Apply cheap echo ducking when remote audio was recently loud.
            duck_mic_against_remote(
                &mut frame,
                last_remote_rms,
                ECHO_DUCK_THRESHOLD_RMS,
                ECHO_DUCK_GAIN,
            );

            // Suppress the frame when muted or the noise gate is closed.
            // Heartbeats still flow via the timer below so the relay keeps
            // learning our UDP address.
            if muted || !gate_open {
                continue;
            }

            shared.mark_active(&user_id);

            let mut out = vec![0_u8; OPUS_MAX_PACKET_BYTES];
            let written = match encoder.encode(&frame, &mut out) {
                Ok(n) => n,
                Err(err) => {
                    eprintln!("voice worker: opus encode error: {err}");
                    continue;
                }
            };
            out.truncate(written);

            let packet = VoicePacket {
                user_id: user_id.clone(),
                room_id: room_id.clone(),
                sequence,
                sample_rate: OPUS_SAMPLE_RATE,
                channels: 1,
                codec: VOICE_CODEC_OPUS.to_owned(),
                frame_samples: OPUS_FRAME_SAMPLES as u32,
                payload: out,
            };
            if let Ok(bytes) = encode_voice_packet_binary(&packet) {
                let _ = socket.send(&bytes);
            }
            sequence = sequence.wrapping_add(1);
            last_heartbeat = Instant::now();
        }

        // ===== Heartbeat =====
        // Send a keep-alive so the relay learns our UDP address even when
        // muted or the noise gate is closed. Heartbeats carry the current
        // sequence but do NOT advance it; only actual audio frames do.
        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            let packet = VoicePacket {
                user_id: user_id.clone(),
                room_id: room_id.clone(),
                sequence,
                sample_rate: OPUS_SAMPLE_RATE,
                channels: 1,
                codec: VOICE_CODEC_OPUS.to_owned(),
                frame_samples: 0,
                payload: Vec::new(),
            };
            if let Ok(bytes) = encode_voice_packet_binary(&packet) {
                let _ = socket.send(&bytes);
            }
            last_heartbeat = Instant::now();
        }

        // ===== Receive path =====
        // Drain a few packets per loop iteration so we don't fall behind.
        let deafened = shared.is_deafened();
        for _ in 0..16 {
            match socket.recv(&mut recv_buf) {
                Ok(len) => {
                    let Ok(packet) = decode_voice_packet_binary(&recv_buf[..len]) else {
                        continue;
                    };
                    if packet.user_id == user_id {
                        // Echo of our own packet; ignore.
                        continue;
                    }
                    if packet.payload.is_empty() {
                        // Heartbeat - nothing to play, nothing to mark active.
                        continue;
                    }
                    if deafened {
                        // Drop incoming audio entirely while deafened, but
                        // still consume from the socket so the OS buffer
                        // doesn't back up.
                        continue;
                    }
                    handle_remote_packet(&packet, &mut remotes, &playback_tx, &shared);
                }
                Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    break;
                }
                Err(err) => {
                    eprintln!("voice worker: socket recv error: {err}");
                    thread::sleep(Duration::from_millis(20));
                    break;
                }
            }
        }

        // ===== Drain jitter buffers =====
        if !deafened {
            let mut total_remote_rms: f32 = 0.0;
            let mut emitted = 0;
            for (peer_id, remote) in remotes.iter_mut() {
                while let Some(pcm) = pop_decoded_frame(remote, false) {
                    if !pcm.is_empty() {
                        let rms = frame_rms(&pcm);
                        if rms > NOISE_GATE_OPEN_RMS {
                            shared.mark_active(peer_id);
                        }
                        total_remote_rms += rms;
                        emitted += 1;
                        let _ = playback_tx.send(AudioFrame {
                            sample_rate: OPUS_SAMPLE_RATE,
                            channels: 1,
                            pcm,
                        });
                    }
                }
            }
            // Smooth the per-loop estimate so a single loud frame doesn't
            // permanently duck the mic.
            let avg = if emitted > 0 {
                total_remote_rms / emitted as f32
            } else {
                0.0
            };
            last_remote_rms = (last_remote_rms * 0.6) + (avg * 0.4);
        } else {
            last_remote_rms = 0.0;
        }

        // Avoid spinning when there is nothing to do.
        thread::sleep(Duration::from_millis(2));
    }

    backend.stop();
    drop(playback_tx);
}

fn build_encoder() -> Result<Encoder, opus::Error> {
    let mut enc = Encoder::new(OPUS_SAMPLE_RATE, Channels::Mono, Application::Voip)?;
    let _ = enc.set_bitrate(opus::Bitrate::Bits(OPUS_BITRATE_BPS));
    let _ = enc.set_inband_fec(true);
    let _ = enc.set_packet_loss_perc(OPUS_EXPECTED_PACKET_LOSS_PCT);
    Ok(enc)
}

fn handle_remote_packet(
    packet: &VoicePacket,
    remotes: &mut HashMap<String, RemoteVoice>,
    playback_tx: &mpsc::Sender<AudioFrame>,
    shared: &VoiceShared,
) {
    // Unknown codecs (e.g. raw PCM from a legacy peer) are passed straight
    // through to playback so cross-version sessions still work, but they don't
    // benefit from the jitter buffer / PLC.
    if packet.codec != VOICE_CODEC_OPUS {
        if let Some(pcm) = decode_legacy_pcm(packet) {
            if !pcm.is_empty() {
                if frame_rms(&pcm) > NOISE_GATE_OPEN_RMS {
                    shared.mark_active(&packet.user_id);
                }
                let _ = playback_tx.send(AudioFrame {
                    sample_rate: packet.sample_rate.max(8_000),
                    channels: packet.channels.max(1),
                    pcm,
                });
            }
        }
        return;
    }

    let entry = match remotes.entry(packet.user_id.clone()) {
        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
        std::collections::hash_map::Entry::Vacant(e) => match RemoteVoice::new() {
            Ok(rv) => e.insert(rv),
            Err(err) => {
                eprintln!(
                    "voice worker: failed to create remote decoder for {}: {err}",
                    packet.user_id
                );
                return;
            }
        },
    };
    entry.jitter.push(packet.sequence, packet.payload.clone());
}

fn pop_decoded_frame(remote: &mut RemoteVoice, force_drain: bool) -> Option<Vec<i16>> {
    let pop = remote.jitter.pop(force_drain);
    let mut out = vec![0_i16; OPUS_FRAME_SAMPLES];
    match pop {
        JitterPop::Empty => None,
        JitterPop::Packet { payload, .. } => {
            // Decode the actual packet.
            match remote.decoder.decode(&payload, &mut out, false) {
                Ok(samples) => {
                    out.truncate(samples);
                    Some(out)
                }
                Err(err) => {
                    eprintln!("voice worker: opus decode error: {err}");
                    None
                }
            }
        }
        JitterPop::Lost { next_payload, .. } => {
            // Try Opus in-band FEC if we have the next packet, else PLC
            // (decode with empty input).
            let result = match next_payload.as_deref() {
                Some(next) => remote.decoder.decode(next, &mut out, true),
                None => remote.decoder.decode(&[], &mut out, false),
            };
            match result {
                Ok(samples) => {
                    out.truncate(samples);
                    Some(out)
                }
                Err(err) => {
                    eprintln!("voice worker: opus PLC/FEC error: {err}");
                    None
                }
            }
        }
    }
}

/// Best-effort decode of a legacy raw-PCM `VoicePacket` (codec == "pcm_s16le"
/// or unset on the wire). Resamples to 48 kHz mono for playback so the rest
/// of the pipeline stays uniform.
fn decode_legacy_pcm(packet: &VoicePacket) -> Option<Vec<i16>> {
    if packet.payload.is_empty() || !packet.payload.len().is_multiple_of(2) {
        return None;
    }
    let mut samples = Vec::with_capacity(packet.payload.len() / 2);
    for chunk in packet.payload.chunks_exact(2) {
        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let mono = if packet.channels > 1 {
        // Average down to mono.
        let ch = packet.channels as usize;
        let frames = samples.len() / ch;
        let mut out = Vec::with_capacity(frames);
        for f in 0..frames {
            let start = f * ch;
            let mut acc: i32 = 0;
            for c in 0..ch {
                acc += samples[start + c] as i32;
            }
            out.push((acc / ch as i32) as i16);
        }
        out
    } else {
        samples
    };
    let resampled = if packet.sample_rate == OPUS_SAMPLE_RATE || packet.sample_rate == 0 {
        mono
    } else {
        resample_linear_mono(&mono, packet.sample_rate, OPUS_SAMPLE_RATE)
    };
    Some(resampled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_discord_core::VOICE_CODEC_PCM_S16LE;

    #[test]
    fn voice_session_default_is_idle() {
        let session = VoiceSession::default();
        assert!(!session.is_running());
    }

    #[test]
    fn voice_shared_mute_and_deafen_round_trip() {
        let s = VoiceShared::new();
        assert!(!s.is_muted() && !s.is_deafened());
        s.set_muted(true);
        s.set_deafened(true);
        assert!(s.is_muted() && s.is_deafened());
    }

    #[test]
    fn voice_shared_active_returns_false_for_unknown_user() {
        let s = VoiceShared::new();
        assert!(!s.is_active("nobody"));
    }

    #[test]
    fn voice_shared_mark_active_sets_recent_window() {
        let s = VoiceShared::new();
        s.mark_active("u1");
        assert!(s.is_active("u1"));
    }

    #[test]
    fn legacy_pcm_decode_handles_mono_48k() {
        let samples: Vec<i16> = (0..96).map(|i| (i * 100) as i16).collect();
        let mut payload = Vec::with_capacity(samples.len() * 2);
        for s in &samples {
            payload.extend_from_slice(&s.to_le_bytes());
        }
        let packet = VoicePacket {
            user_id: "u".to_owned(),
            room_id: "r".to_owned(),
            sequence: 0,
            sample_rate: OPUS_SAMPLE_RATE,
            channels: 1,
            codec: VOICE_CODEC_PCM_S16LE.to_owned(),
            frame_samples: 0,
            payload,
        };
        let decoded = decode_legacy_pcm(&packet).unwrap();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn legacy_pcm_decode_rejects_empty_or_odd_payload() {
        let packet = VoicePacket {
            user_id: "u".to_owned(),
            room_id: "r".to_owned(),
            sequence: 0,
            sample_rate: OPUS_SAMPLE_RATE,
            channels: 1,
            codec: VOICE_CODEC_PCM_S16LE.to_owned(),
            frame_samples: 0,
            payload: vec![1],
        };
        assert!(decode_legacy_pcm(&packet).is_none());

        let packet_empty = VoicePacket {
            payload: Vec::new(),
            ..packet
        };
        assert!(decode_legacy_pcm(&packet_empty).is_none());
    }

    #[test]
    fn opus_round_trip_preserves_silence_length() {
        // A pure encode/decode round-trip across the same configuration must
        // succeed and return exactly OPUS_FRAME_SAMPLES samples per frame.
        let mut enc = build_encoder().expect("encoder");
        let mut dec = Decoder::new(OPUS_SAMPLE_RATE, Channels::Mono).expect("decoder");
        let frame = vec![0_i16; OPUS_FRAME_SAMPLES];
        let mut packet = vec![0_u8; OPUS_MAX_PACKET_BYTES];
        let written = enc.encode(&frame, &mut packet).expect("encode");
        let mut out = vec![0_i16; OPUS_FRAME_SAMPLES];
        let samples = dec
            .decode(&packet[..written], &mut out, false)
            .expect("decode");
        assert_eq!(samples, OPUS_FRAME_SAMPLES);
    }
}
