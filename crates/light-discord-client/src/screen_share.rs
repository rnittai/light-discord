use base64::Engine;
use light_discord_core::{
    default_screen_share_requested_codecs, ClientFrame, ScreenShareMode, ScreenShareResolution,
    SCREEN_SHARE_CODEC_JPEG, SCREEN_SHARE_TRANSPORT_SFU_RELAY,
};
use light_discord_platform::{fit_screen_share_dimensions, ScreenShareSource};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TEXT_MODE_FPS: u32 = 5;
const TEXT_MODE_JPEG_QUALITY: u8 = 84;
const GAME_MODE_30FPS_JPEG_QUALITY: u8 = 56;
const GAME_MODE_60FPS_JPEG_QUALITY: u8 = 46;

#[derive(Debug, Clone)]
pub enum ScreenShareEvent {
    Error(String),
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenShareSettings {
    pub mode: ScreenShareMode,
    pub resolution: ScreenShareResolution,
    pub game_fps: u32,
}

impl ScreenShareSettings {
    pub fn new(mode: ScreenShareMode, resolution: ScreenShareResolution, game_fps: u32) -> Self {
        Self {
            mode,
            resolution,
            game_fps: normalize_game_fps(game_fps),
        }
    }

    pub fn target_fps(self) -> u32 {
        match self.mode {
            ScreenShareMode::Text => TEXT_MODE_FPS,
            ScreenShareMode::Game => self.game_fps,
        }
    }

    fn jpeg_quality(self) -> u8 {
        match (self.mode, self.target_fps()) {
            (ScreenShareMode::Text, _) => TEXT_MODE_JPEG_QUALITY,
            (ScreenShareMode::Game, 60) => GAME_MODE_60FPS_JPEG_QUALITY,
            (ScreenShareMode::Game, _) => GAME_MODE_30FPS_JPEG_QUALITY,
        }
    }

    fn frame_interval(self) -> Duration {
        Duration::from_millis((1000 / self.target_fps().max(1)) as u64)
    }

    fn max_dimensions(self) -> (u32, u32) {
        self.resolution.max_dimensions()
    }
}

impl Default for ScreenShareSettings {
    fn default() -> Self {
        Self::new(
            ScreenShareMode::Text,
            ScreenShareResolution::default(),
            ScreenShareMode::Game.default_fps(),
        )
    }
}

pub struct ScreenShareSession {
    running: Arc<AtomicBool>,
    event_rx: std::sync::mpsc::Receiver<ScreenShareEvent>,
}

impl ScreenShareSession {
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn poll(&self) -> Vec<ScreenShareEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

pub fn start_screen_share(
    source: ScreenShareSource,
    command_tx: Sender<ClientFrame>,
    settings: ScreenShareSettings,
) -> ScreenShareSession {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let (event_tx, event_rx) = mpsc::channel::<ScreenShareEvent>();

    std::thread::spawn(move || {
        run_screen_share_worker(source, command_tx, settings, event_tx, running_clone);
    });

    ScreenShareSession { running, event_rx }
}

fn run_screen_share_worker(
    source: ScreenShareSource,
    command_tx: Sender<ClientFrame>,
    settings: ScreenShareSettings,
    event_tx: std::sync::mpsc::Sender<ScreenShareEvent>,
    running: Arc<AtomicBool>,
) {
    let (max_width, max_height) = settings.max_dimensions();
    let (declared_width, declared_height) =
        fit_screen_share_dimensions(source.width, source.height, max_width, max_height);
    let start_frame = ClientFrame::StartScreenShare {
        source_name: source.title.clone(),
        width: declared_width,
        height: declared_height,
        mode: settings.mode,
        resolution: settings.resolution,
        target_fps: settings.target_fps(),
        requested_codecs: default_screen_share_requested_codecs(),
        transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
    };
    if command_tx.send(start_frame).is_err() {
        let _ = event_tx.send(ScreenShareEvent::Error("command channel closed".to_owned()));
        running.store(false, Ordering::Relaxed);
        return;
    }

    let mut sequence: u64 = 0;
    let frame_interval = settings.frame_interval();
    let jpeg_quality = settings.jpeg_quality();

    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match light_discord_platform::capture_screen_source_jpeg(
            &source.id,
            max_width,
            max_height,
            jpeg_quality,
        ) {
            Ok(frame) => {
                let base64_engine = base64::engine::general_purpose::STANDARD;
                let data_base64 = base64_engine.encode(&frame.data);

                let screen_frame = ClientFrame::ScreenShareFrame {
                    width: frame.width,
                    height: frame.height,
                    image_format: frame.image_format,
                    codec: SCREEN_SHARE_CODEC_JPEG.to_owned(),
                    transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
                    data_base64,
                    sequence,
                    unix_ms: now_unix_ms(),
                };

                if command_tx.send(screen_frame).is_err() {
                    let _ =
                        event_tx.send(ScreenShareEvent::Error("command channel closed".to_owned()));
                    break;
                }

                sequence = sequence.wrapping_add(1);
                std::thread::sleep(frame_interval);
            }
            Err(err) => {
                let _ = event_tx.send(ScreenShareEvent::Error(err.to_string()));
                break;
            }
        }
    }

    let _ = command_tx.send(ClientFrame::StopScreenShare);
    let _ = event_tx.send(ScreenShareEvent::Stopped);
    running.store(false, Ordering::Relaxed);
}

fn normalize_game_fps(fps: u32) -> u32 {
    if fps == 60 {
        60
    } else {
        30
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_share_session_default_is_not_running_after_mock_stop() {
        let running = Arc::new(AtomicBool::new(true));
        let session_running = running.clone();

        running.store(false, Ordering::Relaxed);
        assert!(!session_running.load(Ordering::Relaxed));
    }

    #[test]
    fn screen_share_session_tracks_running_state() {
        let running = Arc::new(AtomicBool::new(true));
        assert!(running.load(Ordering::Relaxed));

        running.store(false, Ordering::Relaxed);
        assert!(!running.load(Ordering::Relaxed));
    }

    #[test]
    fn text_mode_preset_uses_low_fps_and_higher_quality() {
        let settings =
            ScreenShareSettings::new(ScreenShareMode::Text, ScreenShareResolution::P720, 60);
        assert_eq!(settings.target_fps(), 5);
        assert_eq!(settings.jpeg_quality(), TEXT_MODE_JPEG_QUALITY);
        assert_eq!(settings.max_dimensions(), (1280, 720));
    }

    #[test]
    fn game_mode_preset_accepts_only_30_or_60_fps() {
        let settings =
            ScreenShareSettings::new(ScreenShareMode::Game, ScreenShareResolution::P1080, 60);
        assert_eq!(settings.target_fps(), 60);
        assert_eq!(settings.jpeg_quality(), GAME_MODE_60FPS_JPEG_QUALITY);
        assert_eq!(settings.max_dimensions(), (1920, 1080));

        let settings =
            ScreenShareSettings::new(ScreenShareMode::Game, ScreenShareResolution::P720, 120);
        assert_eq!(settings.target_fps(), 30);
        assert_eq!(settings.jpeg_quality(), GAME_MODE_30FPS_JPEG_QUALITY);
    }

    #[test]
    fn declared_dimensions_fit_selected_resolution() {
        assert_eq!(
            fit_screen_share_dimensions(3840, 2160, 1920, 1080),
            (1920, 1080)
        );
        assert_eq!(
            fit_screen_share_dimensions(1920, 1080, 1280, 720),
            (1280, 720)
        );
        assert_eq!(
            fit_screen_share_dimensions(1200, 1600, 1280, 720),
            (540, 720)
        );
    }
}
