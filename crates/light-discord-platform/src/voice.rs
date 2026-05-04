//! Pure-Rust voice DSP utilities and an Opus codec wrapper.
//!
//! This module is intentionally self-contained from `audio.rs` so the helpers
//! (resampling, noise gate, jitter buffer) can be unit tested without touching
//! cpal or the audio device APIs.

use std::collections::BTreeMap;

/// Target Opus sample rate. Opus officially supports 8/12/16/24/48 kHz; we
/// always run at 48 kHz to keep the wire format predictable.
pub const OPUS_SAMPLE_RATE: u32 = 48_000;
/// 20 ms frame at 48 kHz = 960 samples per channel.
pub const OPUS_FRAME_SAMPLES: usize = 960;
/// Generous Opus output buffer. The codec rarely produces more than ~400
/// bytes per 20 ms frame at typical bitrates, but the spec allows up to
/// 1275 bytes per frame.
pub const OPUS_MAX_PACKET_BYTES: usize = 1_500;

/// Linear (4-tap nearest-neighbour) resampler for monaural i16 PCM.
///
/// This is intentionally simple - Opus expects 8/12/16/24/48 kHz, and we always
/// target 48 kHz, so the only requirement is a stable, allocation-light
/// converter that runs on every platform without extra dependencies. The
/// quality is good enough for voice; we do *not* claim broadcast-quality
/// resampling.
///
/// Rules:
/// - `samples` is mono, `i16` PCM.
/// - Returns an empty `Vec` when either rate is zero or the input is empty.
/// - When `src_rate == dst_rate`, the input is copied through unchanged.
pub fn resample_linear_mono(samples: &[i16], src_rate: u32, dst_rate: u32) -> Vec<i16> {
    if src_rate == 0 || dst_rate == 0 || samples.is_empty() {
        return Vec::new();
    }
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    let src_len = samples.len() as u64;
    let out_len = ((src_len * dst_rate as u64) / src_rate as u64) as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(out_len);
    let step_num = src_rate as u64;
    let step_den = dst_rate as u64;
    for i in 0..out_len {
        // Position in source, in fixed-point: idx = i * src/dst.
        let pos_num = (i as u64) * step_num;
        let src_index = (pos_num / step_den) as usize;
        let frac_num = pos_num - (src_index as u64) * step_den;
        let s0 = samples[src_index] as i32;
        let s1 = if src_index + 1 < samples.len() {
            samples[src_index + 1] as i32
        } else {
            s0
        };
        // Linear interpolation between s0 and s1 by fraction frac_num/step_den.
        let interp = s0 + ((s1 - s0) * frac_num as i32) / (step_den as i32).max(1);
        out.push(interp.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    out
}

/// Single-pole high-pass filter that removes DC and very-low-frequency noise
/// (HVAC rumble, mic body thumps). Stateful so it can run continuously across
/// frames.
#[derive(Debug, Clone)]
pub struct HighPassFilter {
    alpha: f32,
    last_in: f32,
    last_out: f32,
}

impl HighPassFilter {
    /// `cutoff_hz` is approximate; values around 80-120 Hz remove rumble
    /// without thinning out voice.
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let sr = sample_rate.max(1) as f32;
        let cutoff = cutoff_hz.max(1.0);
        // Standard 1st-order high-pass coefficient.
        // alpha = RC / (RC + dt), dt = 1/sr, RC = 1/(2*pi*cutoff)
        let rc = 1.0 / (std::f32::consts::TAU * cutoff);
        let dt = 1.0 / sr;
        let alpha = rc / (rc + dt);
        Self {
            alpha: alpha.clamp(0.0, 0.999),
            last_in: 0.0,
            last_out: 0.0,
        }
    }

    pub fn process(&mut self, samples: &mut [i16]) {
        for s in samples.iter_mut() {
            let x = *s as f32;
            let y = self.alpha * (self.last_out + x - self.last_in);
            self.last_in = x;
            self.last_out = y;
            *s = y.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }

    pub fn reset(&mut self) {
        self.last_in = 0.0;
        self.last_out = 0.0;
    }
}

/// Energy-based noise gate / voice activity detector with hangover.
///
/// `process()` is called per frame and returns `true` while the signal is
/// considered "active" (above the open threshold, or within the hangover
/// window after the last loud frame). Hangover prevents ugly clipping at the
/// tail of a word.
#[derive(Debug, Clone)]
pub struct NoiseGate {
    open_threshold_rms: f32,
    close_threshold_rms: f32,
    hangover_frames: u32,
    remaining_hangover: u32,
    is_open: bool,
}

impl NoiseGate {
    /// `open_rms` is the RMS amplitude above which the gate opens.
    /// `close_rms` is the (lower) amplitude at which the gate closes once the
    /// hangover window expires; should be <= `open_rms` to add hysteresis.
    pub fn new(open_rms: f32, close_rms: f32, hangover_frames: u32) -> Self {
        Self {
            open_threshold_rms: open_rms,
            close_threshold_rms: close_rms.min(open_rms),
            hangover_frames,
            remaining_hangover: 0,
            is_open: false,
        }
    }

    /// Returns `(is_active, rms)` for diagnostic / UI use.
    pub fn process(&mut self, frame: &[i16]) -> (bool, f32) {
        let rms = frame_rms(frame);
        if rms >= self.open_threshold_rms {
            self.is_open = true;
            self.remaining_hangover = self.hangover_frames;
        } else if self.is_open {
            if rms < self.close_threshold_rms {
                if self.remaining_hangover == 0 {
                    self.is_open = false;
                } else {
                    self.remaining_hangover = self.remaining_hangover.saturating_sub(1);
                }
            } else {
                // Between thresholds: keep open and refresh hangover so brief
                // dips in the middle of speech don't close the gate.
                self.remaining_hangover = self.hangover_frames;
            }
        }
        (self.is_open, rms)
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }
}

/// RMS amplitude of a frame in the same units as i16 (0..32768).
pub fn frame_rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let mut acc: f64 = 0.0;
    for &s in frame {
        let v = s as f64;
        acc += v * v;
    }
    (acc / frame.len() as f64).sqrt() as f32
}

/// Attenuate `mic_frame` in place when the most recent remote playback frame
/// was loud - a poor man's echo suppressor that cuts the microphone gain when
/// the speakers are likely producing audio that the microphone could pick up.
///
/// This is *not* AEC. Real AEC needs an adaptive filter against a reference
/// playback signal. This is intentionally conservative: we just multiply mic
/// samples by `gain` when remote energy is above the supplied threshold.
pub fn duck_mic_against_remote(mic_frame: &mut [i16], remote_rms: f32, threshold: f32, gain: f32) {
    if remote_rms < threshold || gain >= 1.0 {
        return;
    }
    let g = gain.clamp(0.0, 1.0);
    for s in mic_frame.iter_mut() {
        let v = *s as f32 * g;
        *s = v.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}

/// Per-remote-user reorder + loss buffer.
///
/// Packets arriving over UDP can be out-of-order, duplicated, or missing. The
/// jitter buffer holds a small window of decoded-or-not packets keyed by
/// `sequence` and emits them in order, returning `JitterPop::Lost` for gaps
/// once they are deemed unrecoverable. The caller (the decoder) decides how
/// to handle losses: Opus offers PLC (decode with empty input) and FEC
/// (decode with the *next* packet, asking it to recover the previous frame).
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    /// Target depth (in packets). The buffer waits until it has at least this
    /// many packets before draining, except when forced.
    target_depth: usize,
    /// Hard cap. Older packets are dropped if the buffer grows past this.
    max_depth: usize,
    /// Sequence number of the next packet we expect to emit.
    next_expected: Option<u64>,
    pending: BTreeMap<u64, Vec<u8>>,
    /// Once we've hit `target_depth` we keep draining until empty so a brief
    /// dip in arrival rate doesn't re-arm the pre-roll wait.
    primed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JitterPop {
    /// A packet payload is ready for decode.
    Packet { sequence: u64, payload: Vec<u8> },
    /// A packet at this sequence is known to be lost - the decoder should run
    /// PLC. `next_payload` is the *following* packet, if any, useful for
    /// Opus in-band FEC.
    Lost {
        sequence: u64,
        next_payload: Option<Vec<u8>>,
    },
    /// Buffer is empty / not yet ready to drain.
    Empty,
}

impl JitterBuffer {
    pub fn new(target_depth: usize, max_depth: usize) -> Self {
        Self {
            target_depth: target_depth.max(1),
            max_depth: max_depth.max(target_depth.max(1)),
            next_expected: None,
            pending: BTreeMap::new(),
            primed: false,
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Push a packet into the buffer. Stale packets (older than what we have
    /// already emitted) are dropped silently.
    pub fn push(&mut self, sequence: u64, payload: Vec<u8>) {
        if let Some(next) = self.next_expected {
            if sequence < next {
                return;
            }
        }
        self.pending.insert(sequence, payload);
        // Hard cap: drop the oldest entries first.
        while self.pending.len() > self.max_depth {
            if let Some((&k, _)) = self.pending.iter().next() {
                self.pending.remove(&k);
            }
        }
    }

    /// Try to emit one packet (or signal loss). Returns `Empty` if the buffer
    /// has fewer than `target_depth` packets and `force_drain` is false.
    pub fn pop(&mut self, force_drain: bool) -> JitterPop {
        if self.pending.is_empty() {
            self.primed = false;
            return JitterPop::Empty;
        }
        if self.pending.len() >= self.target_depth {
            self.primed = true;
        }
        if !force_drain && !self.primed {
            return JitterPop::Empty;
        }
        // Initialise the expected sequence from the first packet we ever see.
        let first_key = *self.pending.keys().next().unwrap();
        let expected = self.next_expected.unwrap_or(first_key);

        if let Some(payload) = self.pending.remove(&expected) {
            self.next_expected = Some(expected.wrapping_add(1));
            return JitterPop::Packet {
                sequence: expected,
                payload,
            };
        }
        // We have a hole. Look for the next available packet so the caller
        // can decode it with FEC=true (recover the lost frame).
        let next_payload = self
            .pending
            .iter()
            .next()
            .map(|(_, payload)| payload.clone());
        self.next_expected = Some(expected.wrapping_add(1));
        JitterPop::Lost {
            sequence: expected,
            next_payload,
        }
    }
}

/// Convenience helper: split a buffer of i16 samples into fixed-size frames,
/// returning the consumed count. Any remaining samples (less than `frame`)
/// stay in `buffer`.
pub fn drain_frames(buffer: &mut Vec<i16>, frame: usize) -> Vec<Vec<i16>> {
    if frame == 0 || buffer.len() < frame {
        return Vec::new();
    }
    let mut out = Vec::new();
    while buffer.len() >= frame {
        let chunk: Vec<i16> = buffer.drain(..frame).collect();
        out.push(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_passthrough_when_rates_match() {
        let pcm = vec![1_i16, 2, 3, 4];
        assert_eq!(resample_linear_mono(&pcm, 48_000, 48_000), pcm);
    }

    #[test]
    fn resample_handles_zero_inputs() {
        assert!(resample_linear_mono(&[], 48_000, 48_000).is_empty());
        assert!(resample_linear_mono(&[1, 2, 3], 0, 48_000).is_empty());
        assert!(resample_linear_mono(&[1, 2, 3], 48_000, 0).is_empty());
    }

    #[test]
    fn resample_44100_to_48000_grows_buffer() {
        // 441 samples in -> ~480 samples out (one 10ms frame ratio).
        let pcm: Vec<i16> = (0..441).map(|i| i as i16).collect();
        let out = resample_linear_mono(&pcm, 44_100, 48_000);
        assert_eq!(out.len(), 480);
        // Endpoints should roughly bracket the input range.
        assert_eq!(out[0], 0);
    }

    #[test]
    fn resample_downsample_shrinks_buffer() {
        let pcm: Vec<i16> = (0..960).map(|i| i as i16).collect();
        let out = resample_linear_mono(&pcm, 48_000, 24_000);
        assert_eq!(out.len(), 480);
    }

    #[test]
    fn highpass_removes_dc_offset() {
        let mut pcm = vec![5_000_i16; 4_800]; // 100 ms of constant DC at 48 kHz.
        let mut hp = HighPassFilter::new(48_000, 80.0);
        hp.process(&mut pcm);
        // After a few hundred ms, the running output of a high-pass on DC
        // should be near zero.
        let tail_rms = frame_rms(&pcm[pcm.len() - 480..]);
        assert!(tail_rms < 100.0, "tail rms was {tail_rms}");
    }

    #[test]
    fn noise_gate_opens_on_loud_frame_and_closes_after_hangover() {
        // open at rms >= 500, close below 200, 2-frame hangover.
        let mut gate = NoiseGate::new(500.0, 200.0, 2);
        let silent = vec![0_i16; 960];
        let loud = vec![3_000_i16; 960];

        let (open, _) = gate.process(&silent);
        assert!(!open, "gate should start closed");

        let (open, _) = gate.process(&loud);
        assert!(open, "gate should open on loud frame");

        // Hangover keeps it open for two more silent frames.
        assert!(gate.process(&silent).0);
        assert!(gate.process(&silent).0);
        // Third silent frame: hangover exhausted, gate closes.
        assert!(!gate.process(&silent).0);
    }

    #[test]
    fn jitter_buffer_emits_in_order_after_target_depth() {
        let mut jb = JitterBuffer::new(3, 8);
        jb.push(1, vec![1]);
        jb.push(0, vec![0]);
        // Below target depth (2 < 3) nothing comes out yet.
        assert!(matches!(jb.pop(false), JitterPop::Empty));

        // Add one more so we're at target depth.
        jb.push(2, vec![2]);

        // Now popping returns packets in sequence order.
        for expected_seq in 0..3 {
            match jb.pop(false) {
                JitterPop::Packet { sequence, payload } => {
                    assert_eq!(sequence, expected_seq);
                    assert_eq!(payload, vec![expected_seq as u8]);
                }
                other => panic!("unexpected at seq={expected_seq}: {other:?}"),
            }
        }
        assert!(matches!(jb.pop(false), JitterPop::Empty));
    }

    #[test]
    fn jitter_buffer_reports_loss_with_next_packet_for_fec() {
        let mut jb = JitterBuffer::new(1, 8);
        // Insert seq 0 then 2 - seq 1 is lost.
        jb.push(0, vec![0]);
        jb.push(2, vec![2]);

        match jb.pop(true) {
            JitterPop::Packet { sequence, .. } => assert_eq!(sequence, 0),
            other => panic!("unexpected: {other:?}"),
        }
        match jb.pop(true) {
            JitterPop::Lost {
                sequence,
                next_payload,
            } => {
                assert_eq!(sequence, 1);
                assert_eq!(next_payload, Some(vec![2]));
            }
            other => panic!("unexpected: {other:?}"),
        }
        match jb.pop(true) {
            JitterPop::Packet { sequence, payload } => {
                assert_eq!(sequence, 2);
                assert_eq!(payload, vec![2]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn jitter_buffer_drops_stale_packets() {
        let mut jb = JitterBuffer::new(1, 8);
        jb.push(5, vec![5]);
        match jb.pop(true) {
            JitterPop::Packet { sequence, .. } => assert_eq!(sequence, 5),
            other => panic!("unexpected: {other:?}"),
        }
        // Now push a packet with sequence < next_expected. It must be dropped.
        jb.push(3, vec![3]);
        assert!(jb.is_empty());
    }

    #[test]
    fn jitter_buffer_caps_size_dropping_oldest() {
        let mut jb = JitterBuffer::new(1, 3);
        for seq in 0..5 {
            jb.push(seq, vec![seq as u8]);
        }
        assert_eq!(jb.len(), 3);
        // The two oldest entries (seq 0, 1) should have been dropped.
        match jb.pop(true) {
            JitterPop::Packet { sequence, .. } => assert_eq!(sequence, 2),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn duck_mic_only_attenuates_when_remote_loud() {
        let mut mic = vec![10_000_i16; 8];
        duck_mic_against_remote(
            &mut mic, /*remote_rms*/ 100.0, /*threshold*/ 500.0, 0.5,
        );
        assert_eq!(mic, vec![10_000_i16; 8]);

        duck_mic_against_remote(&mut mic, 1_000.0, 500.0, 0.5);
        assert_eq!(mic, vec![5_000_i16; 8]);
    }

    #[test]
    fn drain_frames_leaves_partial_remainder() {
        let mut buf: Vec<i16> = (0..2_500).collect();
        let frames = drain_frames(&mut buf, 960);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 960);
        assert_eq!(frames[1].len(), 960);
        assert_eq!(buf.len(), 2_500 - 1_920);
    }
}
