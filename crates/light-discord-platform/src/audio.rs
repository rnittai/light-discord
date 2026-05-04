use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_INPUT_DEVICE_ID: &str = "default-input";
pub const DEFAULT_OUTPUT_DEVICE_ID: &str = "default-output";

/// Maximum number of i16 samples kept in the playback queue. At 48 kHz stereo
/// this is roughly 1 second of audio; older samples are dropped if the writer
/// outpaces the audio device.
const PLAYBACK_QUEUE_CAP_SAMPLES: usize = 48_000 * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// Number of backend entries collapsed into this one (1 = no grouping).
    pub grouped_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDeviceList {
    pub inputs: Vec<AudioDeviceInfo>,
    pub outputs: Vec<AudioDeviceInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDeviceSelection {
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: Vec<i16>,
}

pub trait AudioBackend: Send {
    fn backend_name(&self) -> &'static str;
    fn start_capture(&mut self, _device_id: Option<&str>, _tx: Sender<AudioFrame>) -> Result<()>;
    fn start_playback(&mut self, _device_id: Option<&str>, _rx: Receiver<AudioFrame>)
        -> Result<()>;
    fn stop(&mut self);
}

pub fn available_audio_devices() -> Result<AudioDeviceList> {
    let host = cpal::default_host();
    let default_input = host.default_input_device();
    let default_output = host.default_output_device();

    let inputs = collect_devices(
        AudioDeviceKind::Input,
        host.input_devices()
            .context("failed to enumerate input audio devices")?,
        default_input.as_ref(),
    );
    let outputs = collect_devices(
        AudioDeviceKind::Output,
        host.output_devices()
            .context("failed to enumerate output audio devices")?,
        default_output.as_ref(),
    );

    Ok(AudioDeviceList { inputs, outputs })
}

fn collect_devices<I>(
    kind: AudioDeviceKind,
    devices: I,
    default_device: Option<&cpal::Device>,
) -> Vec<AudioDeviceInfo>
where
    I: IntoIterator<Item = cpal::Device>,
{
    let default_name = default_device.and_then(device_name);
    let default_id = default_device.and_then(device_id);
    let mut infos = Vec::new();

    if let Some(name) = &default_name {
        infos.push(AudioDeviceInfo {
            id: kind.default_device_id().to_owned(),
            name: format!("System default ({name})"),
            is_default: false,
            grouped_count: 1,
        });
    }

    for (index, device) in devices.into_iter().enumerate() {
        let name = device_name(&device).unwrap_or_else(|| format!("Unnamed {:?} {index}", kind));
        let id = device_id(&device).unwrap_or_else(|| format!("{}:{index}", kind.id_prefix()));
        let is_default = default_id.as_deref() == Some(id.as_str());
        infos.push(AudioDeviceInfo {
            id,
            name,
            is_default,
            grouped_count: 1,
        });
    }

    deduplicate_devices(infos)
}

/// Removes duplicate and same-named entries from a raw device list.
///
/// System-default alias entries (id == DEFAULT_*_DEVICE_ID) pass through unchanged.
/// For concrete devices:
///
///   1. Entries with the same id are collapsed into one (prefer is_default).
///   2. Entries with the same name are then grouped into one (prefer is_default, keep id).
///
/// First-seen ordering is preserved. `grouped_count` accumulates across both phases.
pub(crate) fn deduplicate_devices(devices: Vec<AudioDeviceInfo>) -> Vec<AudioDeviceInfo> {
    let (system_defaults, concrete): (Vec<_>, Vec<_>) = devices
        .into_iter()
        .partition(|d| d.id == DEFAULT_INPUT_DEVICE_ID || d.id == DEFAULT_OUTPUT_DEVICE_ID);

    // Phase 1: deduplicate by id
    let mut id_deduped: Vec<AudioDeviceInfo> = Vec::new();
    let mut id_map: HashMap<String, usize> = HashMap::new();
    for device in concrete {
        if let Some(&idx) = id_map.get(&device.id) {
            id_deduped[idx].grouped_count += device.grouped_count;
            if device.is_default && !id_deduped[idx].is_default {
                id_deduped[idx].is_default = true;
            }
        } else {
            let idx = id_deduped.len();
            id_map.insert(device.id.clone(), idx);
            id_deduped.push(device);
        }
    }

    // Phase 2: deduplicate by name
    let mut name_deduped: Vec<AudioDeviceInfo> = Vec::new();
    let mut name_map: HashMap<String, usize> = HashMap::new();
    for device in id_deduped {
        if let Some(&idx) = name_map.get(&device.name) {
            name_deduped[idx].grouped_count += device.grouped_count;
            if device.is_default && !name_deduped[idx].is_default {
                name_deduped[idx].is_default = true;
                name_deduped[idx].id = device.id;
            }
        } else {
            let idx = name_deduped.len();
            name_map.insert(device.name.clone(), idx);
            name_deduped.push(device);
        }
    }

    let mut result = system_defaults;
    result.extend(name_deduped);
    result
}

fn device_name(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().trim().to_owned())
        .filter(|name| !name.is_empty())
}

fn device_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

impl AudioDeviceKind {
    fn id_prefix(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }

    fn default_device_id(self) -> &'static str {
        match self {
            Self::Input => DEFAULT_INPUT_DEVICE_ID,
            Self::Output => DEFAULT_OUTPUT_DEVICE_ID,
        }
    }
}

#[derive(Debug, Default)]
pub struct NoopAudioBackend {
    running: bool,
}

impl NoopAudioBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl AudioBackend for NoopAudioBackend {
    fn backend_name(&self) -> &'static str {
        "noop"
    }

    fn start_capture(&mut self, _device_id: Option<&str>, _tx: Sender<AudioFrame>) -> Result<()> {
        self.running = true;
        Ok(())
    }

    fn start_playback(
        &mut self,
        _device_id: Option<&str>,
        _rx: Receiver<AudioFrame>,
    ) -> Result<()> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

/// Real cpal-backed audio backend used by the production client.
pub struct CpalAudioBackend {
    input_stream: Option<cpal::Stream>,
    output_stream: Option<cpal::Stream>,
}

impl CpalAudioBackend {
    pub fn new() -> Self {
        Self {
            input_stream: None,
            output_stream: None,
        }
    }
}

impl Default for CpalAudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for CpalAudioBackend {
    fn backend_name(&self) -> &'static str {
        "cpal"
    }

    fn start_capture(&mut self, device_id: Option<&str>, tx: Sender<AudioFrame>) -> Result<()> {
        // Drop any prior stream first so we don't hold two input devices open.
        self.input_stream = None;

        let host = cpal::default_host();
        let device = resolve_input_device(&host, device_id)?;
        let supported = device
            .default_input_config()
            .context("failed to query default input config")?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let config: cpal::StreamConfig = supported.config();

        let err_callback = |err| eprintln!("voice capture stream error: {err}");

        let stream = match sample_format {
            SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::I8 => build_input_stream::<i8>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::U8 => build_input_stream::<u8>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::I32 => build_input_stream::<i32>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::U32 => build_input_stream::<u32>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            SampleFormat::F64 => build_input_stream::<f64>(
                &device,
                &config,
                sample_format,
                sample_rate,
                channels,
                tx,
                err_callback,
            ),
            other => {
                return Err(anyhow!(
                    "unsupported input sample format: {other:?} (sample_rate={sample_rate} channels={channels})"
                ));
            }
        }
        .with_context(|| format!("failed to build {sample_format:?} input stream"))?;

        stream
            .play()
            .context("failed to start input stream playback")?;
        self.input_stream = Some(stream);
        Ok(())
    }

    fn start_playback(&mut self, device_id: Option<&str>, rx: Receiver<AudioFrame>) -> Result<()> {
        self.output_stream = None;

        let host = cpal::default_host();
        let device = resolve_output_device(&host, device_id)?;
        let supported = device
            .default_output_config()
            .context("failed to query default output config")?;
        let sample_format = supported.sample_format();
        let device_sample_rate = supported.sample_rate();
        let device_channels = supported.channels();
        let config: cpal::StreamConfig = supported.config();

        let queue: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
        let queue_writer = Arc::clone(&queue);
        let queue_reader = Arc::clone(&queue);

        // Background thread: pull AudioFrames off the channel and push i16 samples
        // (already adapted to the device channel count) into the bounded queue.
        std::thread::spawn(move || {
            feed_playback_queue(rx, queue_writer, device_sample_rate, device_channels)
        });

        let err_callback = |err| eprintln!("voice playback stream error: {err}");

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_output_stream::<f32>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::I16 => {
                build_output_stream::<i16>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::U16 => {
                build_output_stream::<u16>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::I8 => {
                build_output_stream::<i8>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::U8 => {
                build_output_stream::<u8>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::I32 => {
                build_output_stream::<i32>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::U32 => {
                build_output_stream::<u32>(&device, &config, queue_reader, err_callback)
            }
            SampleFormat::F64 => {
                build_output_stream::<f64>(&device, &config, queue_reader, err_callback)
            }
            other => {
                return Err(anyhow!(
                    "unsupported output sample format: {other:?} (sample_rate={device_sample_rate} channels={device_channels})"
                ));
            }
        }
        .with_context(|| format!("failed to build {sample_format:?} output stream"))?;

        stream
            .play()
            .context("failed to start output stream playback")?;
        self.output_stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        self.input_stream = None;
        self.output_stream = None;
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    _sample_format: SampleFormat,
    sample_rate: u32,
    channels: u16,
    tx: Sender<AudioFrame>,
    err_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + Send + 'static,
    i16: FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _info: &cpal::InputCallbackInfo| {
            let i16_data: Vec<i16> = data.iter().map(|s| i16::from_sample(*s)).collect();
            // Downmix to mono regardless of device channel count so payloads
            // stay small and callers need not handle arbitrary channel widths.
            let pcm = adapt_channels(&i16_data, channels, 1);
            let frame = AudioFrame {
                sample_rate,
                channels: 1,
                pcm,
            };
            // Disconnected receiver simply means the worker is shutting down.
            let _ = tx.send(frame);
        },
        err_callback,
        Some(Duration::from_millis(200)),
    )
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<i16>>>,
    err_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<i16> + Send + 'static,
{
    device.build_output_stream(
        config,
        move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
            let mut guard = match queue.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            for slot in data.iter_mut() {
                let sample = guard.pop_front().unwrap_or(0);
                *slot = T::from_sample(sample);
            }
        },
        err_callback,
        Some(Duration::from_millis(200)),
    )
}

/// Prepare an `AudioFrame` for device playback.
///
/// Processing order (correct for arbitrary input rates and channel counts):
///   1. Downmix multi-channel input to mono.
///   2. Resample mono PCM from `frame.sample_rate` to `device_sample_rate`.
///   3. Expand mono to `device_channels`.
///
/// This keeps resampling operating on the smallest buffer (mono) and ensures
/// the output length matches what the device expects.
pub(crate) fn prepare_playback_frame(
    frame: &AudioFrame,
    device_sample_rate: u32,
    device_channels: u16,
) -> Vec<i16> {
    let mono = if frame.channels != 1 {
        adapt_channels(&frame.pcm, frame.channels, 1)
    } else {
        frame.pcm.clone()
    };
    let resampled = if frame.sample_rate == 0 || frame.sample_rate == device_sample_rate {
        mono
    } else {
        crate::voice::resample_linear_mono(&mono, frame.sample_rate, device_sample_rate)
    };
    if device_channels == 1 {
        resampled
    } else {
        adapt_channels(&resampled, 1, device_channels)
    }
}

fn feed_playback_queue(
    rx: Receiver<AudioFrame>,
    queue: Arc<Mutex<VecDeque<i16>>>,
    device_sample_rate: u32,
    device_channels: u16,
) {
    loop {
        match rx.recv() {
            Ok(frame) => {
                let prepared = prepare_playback_frame(&frame, device_sample_rate, device_channels);
                if let Ok(mut guard) = queue.lock() {
                    guard.extend(prepared);
                    let len = guard.len();
                    if len > PLAYBACK_QUEUE_CAP_SAMPLES {
                        let drop = len - PLAYBACK_QUEUE_CAP_SAMPLES;
                        guard.drain(0..drop);
                    }
                }
            }
            Err(_) => return,
        }
    }
}

/// Adapt a PCM buffer from `src_channels` to `dst_channels` interleaving.
///
/// Mono -> stereo duplicates each sample. Multi-channel -> mono averages the
/// frame samples. Otherwise we copy up to `min(src, dst)` channels per frame
/// and zero-fill the rest. `src_channels == 0` or `dst_channels == 0` returns
/// an empty buffer.
pub fn adapt_channels(pcm: &[i16], src_channels: u16, dst_channels: u16) -> Vec<i16> {
    if src_channels == 0 || dst_channels == 0 || pcm.is_empty() {
        return Vec::new();
    }
    if src_channels == dst_channels {
        return pcm.to_vec();
    }
    let src = src_channels as usize;
    let dst = dst_channels as usize;
    let frames = pcm.len() / src;
    let mut out = Vec::with_capacity(frames * dst);

    if src == 1 && dst > 1 {
        for &sample in &pcm[..frames] {
            for _ in 0..dst {
                out.push(sample);
            }
        }
        return out;
    }

    if dst == 1 {
        for f in 0..frames {
            let start = f * src;
            let mut acc: i32 = 0;
            for c in 0..src {
                acc += pcm[start + c] as i32;
            }
            out.push((acc / src as i32) as i16);
        }
        return out;
    }

    let copy = src.min(dst);
    for f in 0..frames {
        let start = f * src;
        for c in 0..dst {
            if c < copy {
                out.push(pcm[start + c]);
            } else {
                out.push(0);
            }
        }
    }
    out
}

/// Cap a queue to at most `cap` samples, dropping oldest values first.
pub fn cap_playback_queue(queue: &mut VecDeque<i16>, cap: usize) {
    if queue.len() > cap {
        let drop = queue.len() - cap;
        queue.drain(0..drop);
    }
}

fn resolve_input_device(host: &cpal::Host, device_id: Option<&str>) -> Result<cpal::Device> {
    match device_id {
        None | Some(DEFAULT_INPUT_DEVICE_ID) => host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device available")),
        Some(id) => {
            find_device_by_id(host, id).ok_or_else(|| anyhow!("input device id not found: {id}"))
        }
    }
}

fn resolve_output_device(host: &cpal::Host, device_id: Option<&str>) -> Result<cpal::Device> {
    match device_id {
        None | Some(DEFAULT_OUTPUT_DEVICE_ID) => host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device available")),
        Some(id) => {
            find_device_by_id(host, id).ok_or_else(|| anyhow!("output device id not found: {id}"))
        }
    }
}

fn find_device_by_id(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    if let Ok(parsed) = cpal::DeviceId::from_str(id) {
        if let Some(device) = host.device_by_id(&parsed) {
            return Some(device);
        }
    }
    // Fallback: scan all devices and compare the formatted id string.
    host.devices()
        .ok()?
        .find(|device| device.id().ok().map(|d| d.to_string()).as_deref() == Some(id))
}

// Encode/decode helpers for the wire payload (little-endian i16 PCM).
pub fn encode_pcm_le(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn decode_pcm_le(bytes: &[u8]) -> Vec<i16> {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, is_default: bool) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default,
            grouped_count: 1,
        }
    }

    fn sys_default(kind: AudioDeviceKind, default_name: &str) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: kind.default_device_id().to_owned(),
            name: format!("System default ({default_name})"),
            is_default: false,
            grouped_count: 1,
        }
    }

    #[test]
    fn empty_list_stays_empty() {
        assert!(deduplicate_devices(vec![]).is_empty());
    }

    #[test]
    fn no_duplicates_unchanged() {
        let devices = vec![dev("id1", "Mic A", false), dev("id2", "Mic B", false)];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].grouped_count, 1);
        assert_eq!(result[1].grouped_count, 1);
    }

    #[test]
    fn same_id_collapsed() {
        let devices = vec![
            dev("id1", "Mic A", false),
            dev("id1", "Mic A", false),
            dev("id1", "Mic A", false),
        ];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "id1");
        assert_eq!(result[0].grouped_count, 3);
    }

    #[test]
    fn same_name_grouped() {
        let devices = vec![
            dev("id1", "Mic A", false),
            dev("id2", "Mic A", false),
            dev("id3", "Mic A", false),
        ];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "id1");
        assert_eq!(result[0].grouped_count, 3);
    }

    #[test]
    fn prefer_is_default_over_first_seen_same_name() {
        let devices = vec![dev("id1", "Mic A", false), dev("id2", "Mic A", true)];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "id2");
        assert!(result[0].is_default);
        assert_eq!(result[0].grouped_count, 2);
    }

    #[test]
    fn prefer_is_default_over_first_seen_same_id() {
        let devices = vec![dev("id1", "Mic A", false), dev("id1", "Mic A", true)];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_default);
        assert_eq!(result[0].grouped_count, 2);
    }

    #[test]
    fn system_default_passes_through_unchanged() {
        let devices = vec![
            sys_default(AudioDeviceKind::Input, "Mic A"),
            dev("id1", "Mic A", true),
            dev("id2", "Mic A", false),
        ];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, DEFAULT_INPUT_DEVICE_ID);
        assert_eq!(result[0].grouped_count, 1);
        assert_eq!(result[1].id, "id1");
        assert_eq!(result[1].grouped_count, 2);
    }

    #[test]
    fn first_seen_ordering_preserved() {
        let devices = vec![
            dev("id1", "Device A", false),
            dev("id2", "Device B", false),
            dev("id3", "Device C", false),
            dev("id4", "Device A", false),
            dev("id5", "Device B", false),
        ];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "Device A");
        assert_eq!(result[1].name, "Device B");
        assert_eq!(result[2].name, "Device C");
        assert_eq!(result[0].grouped_count, 2);
        assert_eq!(result[1].grouped_count, 2);
        assert_eq!(result[2].grouped_count, 1);
    }

    #[test]
    fn mixed_id_and_name_dedup() {
        // id1 and id1 (same id) -> count=2, then id2 same name -> count=3
        let devices = vec![
            dev("id1", "Mic A", false),
            dev("id1", "Mic A", false),
            dev("id2", "Mic A", true),
        ];
        let result = deduplicate_devices(devices);
        assert_eq!(result.len(), 1);
        // id2 is_default should win in phase 2
        assert_eq!(result[0].id, "id2");
        assert!(result[0].is_default);
        assert_eq!(result[0].grouped_count, 3);
    }

    #[test]
    fn pcm_round_trips_little_endian() {
        let samples = vec![0_i16, 1, -1, 32_767, -32_768, 12345, -12345];
        let bytes = encode_pcm_le(&samples);
        assert_eq!(bytes.len(), samples.len() * 2);
        let decoded = decode_pcm_le(&bytes);
        assert_eq!(decoded, samples);
    }

    #[test]
    fn decode_ignores_trailing_odd_byte() {
        let mut bytes = encode_pcm_le(&[7_i16, -7]);
        bytes.push(0xFF);
        let decoded = decode_pcm_le(&bytes);
        assert_eq!(decoded, vec![7, -7]);
    }

    #[test]
    fn adapt_channels_mono_to_stereo() {
        let pcm = vec![1_i16, 2, 3];
        let out = adapt_channels(&pcm, 1, 2);
        assert_eq!(out, vec![1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn adapt_channels_stereo_to_mono_averages() {
        let pcm = vec![10_i16, 20, -10, -20];
        let out = adapt_channels(&pcm, 2, 1);
        assert_eq!(out, vec![15, -15]);
    }

    #[test]
    fn adapt_channels_passes_through_when_equal() {
        let pcm = vec![1_i16, 2, 3, 4];
        let out = adapt_channels(&pcm, 2, 2);
        assert_eq!(out, pcm);
    }

    #[test]
    fn adapt_channels_truncates_extra_dst_with_zero() {
        // 2-ch source -> 4-ch destination: copy first 2 ch, zero-fill the rest.
        let pcm = vec![1_i16, 2, 3, 4];
        let out = adapt_channels(&pcm, 2, 4);
        assert_eq!(out, vec![1, 2, 0, 0, 3, 4, 0, 0]);
    }

    #[test]
    fn adapt_channels_handles_empty_input() {
        let out = adapt_channels(&[], 2, 2);
        assert!(out.is_empty());
    }

    #[test]
    fn adapt_channels_zero_channels_returns_empty() {
        let pcm = vec![1_i16, 2, 3, 4];
        assert!(adapt_channels(&pcm, 0, 2).is_empty());
        assert!(adapt_channels(&pcm, 2, 0).is_empty());
    }

    #[test]
    fn cap_playback_queue_drops_oldest() {
        let mut queue: VecDeque<i16> = (0..10).collect();
        cap_playback_queue(&mut queue, 4);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.front().copied(), Some(6));
        assert_eq!(queue.back().copied(), Some(9));
    }

    #[test]
    fn cap_playback_queue_noop_when_under_cap() {
        let mut queue: VecDeque<i16> = (0..3).collect();
        cap_playback_queue(&mut queue, 4);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn prepare_playback_passthrough_when_rates_and_channels_match() {
        let frame = AudioFrame {
            sample_rate: 48_000,
            channels: 1,
            pcm: vec![1_i16, 2, 3, 4],
        };
        let out = prepare_playback_frame(&frame, 48_000, 1);
        assert_eq!(out, vec![1, 2, 3, 4]);
    }

    #[test]
    fn prepare_playback_resamples_44100_mono_to_48000_stereo() {
        // 441 mono samples at 44100 Hz -> ~480 mono -> ~960 stereo samples
        let pcm: Vec<i16> = (0..441).map(|i| (i * 10) as i16).collect();
        let frame = AudioFrame {
            sample_rate: 44_100,
            channels: 1,
            pcm,
        };
        let out = prepare_playback_frame(&frame, 48_000, 2);
        // 480 resampled mono frames * 2 channels = 960 samples
        assert_eq!(out.len(), 480 * 2);
        // Each pair must be equal (mono duplicated to stereo).
        for pair in out.chunks_exact(2) {
            assert_eq!(pair[0], pair[1]);
        }
    }

    #[test]
    fn prepare_playback_downmixes_stereo_before_resampling() {
        // Stereo 44100 Hz: 882 samples (441 frames * 2 channels)
        // L=1000, R=3000 -> mono avg 2000 per frame
        let pcm: Vec<i16> = (0..441).flat_map(|_| [1000_i16, 3000_i16]).collect();
        let frame = AudioFrame {
            sample_rate: 44_100,
            channels: 2,
            pcm,
        };
        let out = prepare_playback_frame(&frame, 48_000, 1);
        // All mono samples should be the average 2000 (may vary slightly at boundaries
        // due to interpolation, but the bulk should be 2000).
        let bulk = &out[1..out.len() - 1];
        for &s in bulk {
            assert!((s - 2000).abs() <= 1, "expected ~2000 but got {s}");
        }
    }

    #[test]
    fn prepare_playback_zero_sample_rate_treated_as_passthrough() {
        let frame = AudioFrame {
            sample_rate: 0,
            channels: 1,
            pcm: vec![100_i16, 200, 300],
        };
        let out = prepare_playback_frame(&frame, 48_000, 1);
        assert_eq!(out, vec![100, 200, 300]);
    }

    #[test]
    fn noop_backend_runs_and_stops() {
        let mut backend = NoopAudioBackend::new();
        let (tx_in, _rx_in) = std::sync::mpsc::channel();
        let (_tx_out, rx_out): (Sender<AudioFrame>, Receiver<AudioFrame>) =
            std::sync::mpsc::channel();
        backend.start_capture(None, tx_in).unwrap();
        backend.start_playback(None, rx_out).unwrap();
        assert!(backend.is_running());
        backend.stop();
        assert!(!backend.is_running());
    }
}
