use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
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
        });
    }

    infos
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
