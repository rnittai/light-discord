use anyhow::Result;
use std::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: Vec<i16>,
}

pub trait AudioBackend: Send {
    fn backend_name(&self) -> &'static str;
    fn start_capture(&mut self, _tx: Sender<AudioFrame>) -> Result<()>;
    fn start_playback(&mut self, _rx: Receiver<AudioFrame>) -> Result<()>;
    fn stop(&mut self);
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

    fn start_capture(&mut self, _tx: Sender<AudioFrame>) -> Result<()> {
        self.running = true;
        Ok(())
    }

    fn start_playback(&mut self, _rx: Receiver<AudioFrame>) -> Result<()> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }
}
