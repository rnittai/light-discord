use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

pub const DEFAULT_INPUT_DEVICE_ID: &str = "default-input";
pub const DEFAULT_OUTPUT_DEVICE_ID: &str = "default-output";

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
}
