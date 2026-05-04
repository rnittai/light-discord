use light_discord_core::VoicePacket;
use light_discord_platform::{
    decode_pcm_le, encode_pcm_le, AudioBackend, AudioDeviceSelection, AudioFrame, CpalAudioBackend,
};
use std::{
    io::ErrorKind,
    net::UdpSocket,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Approximate target chunk length when slicing captured PCM into UDP packets.
const CHUNK_MILLIS: u32 = 20;
/// Hard cap on i16 samples per chunk regardless of device configuration.
/// Prevents unexpectedly large payloads if an unusual sample rate or channel
/// count is reported by the device. 1 920 samples = 3 840 raw PCM bytes before
/// JSON overhead, fitting comfortably in the 64 KiB relay buffer. Covers
/// 96 kHz mono 20 ms (1 920 samples) and 48 kHz mono 20 ms (960 samples).
const MAX_CHUNK_SAMPLES: usize = 1_920;
/// How often we send an empty heartbeat packet so the relay learns our address
/// even when the microphone is muted or unavailable.
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);
/// Read timeout used to keep the receive loop responsive to stop signals.
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(50);

enum VoiceCommand {
    Stop,
}

#[derive(Default)]
pub struct VoiceSession {
    stop_tx: Option<Sender<VoiceCommand>>,
    worker: Option<JoinHandle<()>>,
    selected_devices: AudioDeviceSelection,
}

impl VoiceSession {
    pub fn start(
        &mut self,
        udp_addr: String,
        user_id: String,
        room_id: String,
        selected_devices: AudioDeviceSelection,
    ) {
        self.stop();
        self.selected_devices = selected_devices.clone();

        let (stop_tx, stop_rx) = mpsc::channel::<VoiceCommand>();
        let worker = thread::spawn(move || {
            run_voice_worker(udp_addr, user_id, room_id, selected_devices, stop_rx);
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
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_voice_worker(
    udp_addr: String,
    user_id: String,
    room_id: String,
    selected_devices: AudioDeviceSelection,
    stop_rx: mpsc::Receiver<VoiceCommand>,
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
    let mut buffer: Vec<i16> = Vec::new();
    let mut current_sample_rate: u32 = 48_000;
    let mut current_channels: u16 = 1;
    let mut last_heartbeat = Instant::now();
    let mut recv_buf = vec![0_u8; 64 * 1024];

    loop {
        if matches!(stop_rx.try_recv(), Ok(VoiceCommand::Stop)) {
            break;
        }

        // Drain any captured PCM into the local buffer.
        loop {
            match capture_rx.try_recv() {
                Ok(frame) => {
                    current_sample_rate = frame.sample_rate;
                    current_channels = frame.channels;
                    buffer.extend(frame.pcm);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        // Slice the buffer into ~20ms chunks and send each as its own VoicePacket.
        let chunk_samples = chunk_sample_count(current_sample_rate, current_channels, CHUNK_MILLIS);
        if chunk_samples > 0 {
            while buffer.len() >= chunk_samples {
                let chunk: Vec<i16> = buffer.drain(..chunk_samples).collect();
                let payload = encode_pcm_le(&chunk);
                let packet = VoicePacket {
                    user_id: user_id.clone(),
                    room_id: room_id.clone(),
                    sequence,
                    sample_rate: current_sample_rate,
                    channels: current_channels,
                    payload,
                };
                if let Ok(bytes) = serde_json::to_vec(&packet) {
                    let _ = socket.send(&bytes);
                }
                sequence = sequence.wrapping_add(1);
                last_heartbeat = Instant::now();
            }
        }

        // Send a small heartbeat if we have not transmitted anything recently.
        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            let packet = VoicePacket {
                user_id: user_id.clone(),
                room_id: room_id.clone(),
                sequence,
                sample_rate: current_sample_rate,
                channels: current_channels,
                payload: Vec::new(),
            };
            if let Ok(bytes) = serde_json::to_vec(&packet) {
                let _ = socket.send(&bytes);
            }
            sequence = sequence.wrapping_add(1);
            last_heartbeat = Instant::now();
        }

        // Receive at most one packet per loop iteration and forward it to playback.
        match socket.recv(&mut recv_buf) {
            Ok(len) => {
                if let Ok(packet) = serde_json::from_slice::<VoicePacket>(&recv_buf[..len]) {
                    if packet.user_id != user_id && !packet.payload.is_empty() {
                        let pcm = decode_pcm_le(&packet.payload);
                        if !pcm.is_empty() {
                            let frame = AudioFrame {
                                sample_rate: packet.sample_rate,
                                channels: packet.channels.max(1),
                                pcm,
                            };
                            let _ = playback_tx.send(frame);
                        }
                    }
                }
            }
            Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(err) => {
                eprintln!("voice worker: socket recv error: {err}");
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    backend.stop();
    drop(playback_tx);
}

/// Number of i16 samples that make up roughly `millis` milliseconds at the
/// given sample rate and channel count. Returns 0 if either is zero.
///
/// The result is capped at `MAX_CHUNK_SAMPLES` and always a multiple of
/// `channels` so the caller can safely drain interleaved PCM without
/// stranding partial frames.
pub fn chunk_sample_count(sample_rate: u32, channels: u16, millis: u32) -> usize {
    if sample_rate == 0 || channels == 0 {
        return 0;
    }
    let per_channel = (sample_rate as u64 * millis as u64) / 1000;
    let count = (per_channel as usize).saturating_mul(channels as usize);
    let ch = channels as usize;
    // Cap at MAX_CHUNK_SAMPLES, rounding down to the nearest full frame.
    let max_aligned = (MAX_CHUNK_SAMPLES / ch) * ch;
    count.min(max_aligned)
}

/// Slice an interleaved PCM buffer into chunks no larger than `chunk_samples`.
/// The final element may be shorter than `chunk_samples` if the input does not
/// divide evenly. An empty input or a zero `chunk_samples` returns an empty
/// vector.
#[allow(dead_code)]
pub fn chunk_pcm(pcm: &[i16], chunk_samples: usize) -> Vec<Vec<i16>> {
    if chunk_samples == 0 || pcm.is_empty() {
        return Vec::new();
    }
    pcm.chunks(chunk_samples).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_sample_count_handles_typical_voice_config() {
        // 48 kHz mono, 20 ms -> 960 samples
        assert_eq!(chunk_sample_count(48_000, 1, 20), 960);
        // 48 kHz stereo, 20 ms -> 1920 samples
        assert_eq!(chunk_sample_count(48_000, 2, 20), 1920);
        // 44.1 kHz mono, 20 ms -> 882 samples
        assert_eq!(chunk_sample_count(44_100, 1, 20), 882);
    }

    #[test]
    fn chunk_sample_count_zero_inputs() {
        assert_eq!(chunk_sample_count(0, 1, 20), 0);
        assert_eq!(chunk_sample_count(48_000, 0, 20), 0);
        assert_eq!(chunk_sample_count(48_000, 1, 0), 0);
    }

    #[test]
    fn chunk_sample_count_capped_at_max() {
        // 192 kHz mono 20 ms = 3 840 samples -- exceeds cap, clamped to 1 920.
        assert_eq!(chunk_sample_count(192_000, 1, 20), 1_920);
        // An extreme rate whose raw product exceeds MAX_CHUNK_SAMPLES.
        let capped = chunk_sample_count(1_000_000, 1, 20);
        assert!(capped <= MAX_CHUNK_SAMPLES);
        assert!(capped > 0);
    }

    #[test]
    fn chunk_sample_count_cap_aligned_to_channels() {
        // Result must always be a multiple of channels.
        let count2 = chunk_sample_count(1_000_000, 2, 20);
        assert_eq!(count2 % 2, 0);
        let count3 = chunk_sample_count(1_000_000, 3, 20);
        assert_eq!(count3 % 3, 0);
    }

    #[test]
    fn chunk_pcm_evenly_divides() {
        let pcm: Vec<i16> = (0..6).collect();
        let chunks = chunk_pcm(&pcm, 2);
        assert_eq!(chunks, vec![vec![0, 1], vec![2, 3], vec![4, 5]]);
    }

    #[test]
    fn chunk_pcm_handles_remainder() {
        let pcm: Vec<i16> = (0..5).collect();
        let chunks = chunk_pcm(&pcm, 2);
        assert_eq!(chunks, vec![vec![0, 1], vec![2, 3], vec![4]]);
    }

    #[test]
    fn chunk_pcm_empty_inputs() {
        assert!(chunk_pcm(&[], 4).is_empty());
        assert!(chunk_pcm(&[1, 2, 3], 0).is_empty());
    }

    #[test]
    fn pcm_payload_round_trip_matches_platform_helpers() {
        let samples = vec![0_i16, 1, 2, -1, -2, 12345];
        let bytes = encode_pcm_le(&samples);
        let decoded = decode_pcm_le(&bytes);
        assert_eq!(decoded, samples);
    }

    #[test]
    fn voice_session_default_is_idle() {
        let session = VoiceSession::default();
        assert!(!session.is_running());
    }
}
