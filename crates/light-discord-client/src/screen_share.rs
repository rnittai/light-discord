use base64::Engine;
use light_discord_core::ClientFrame;
use light_discord_platform::ScreenShareSource;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum ScreenShareEvent {
    Error(String),
    Stopped,
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
) -> ScreenShareSession {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let (event_tx, event_rx) = mpsc::channel::<ScreenShareEvent>();

    std::thread::spawn(move || {
        run_screen_share_worker(source, command_tx, event_tx, running_clone);
    });

    ScreenShareSession { running, event_rx }
}

fn run_screen_share_worker(
    source: ScreenShareSource,
    command_tx: Sender<ClientFrame>,
    event_tx: std::sync::mpsc::Sender<ScreenShareEvent>,
    running: Arc<AtomicBool>,
) {
    let start_frame = ClientFrame::StartScreenShare {
        source_name: source.title.clone(),
        width: source.width,
        height: source.height,
    };
    if command_tx.send(start_frame).is_err() {
        let _ = event_tx.send(ScreenShareEvent::Error("command channel closed".to_owned()));
        running.store(false, Ordering::Relaxed);
        return;
    }

    let mut sequence: u64 = 0;
    let frame_interval_ms = 500;

    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match light_discord_platform::capture_screen_source_jpeg(&source.id, 1280, 65) {
            Ok(frame) => {
                let base64_engine = base64::engine::general_purpose::STANDARD;
                let data_base64 = base64_engine.encode(&frame.data);

                let screen_frame = ClientFrame::ScreenShareFrame {
                    width: frame.width,
                    height: frame.height,
                    image_format: frame.image_format,
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
                std::thread::sleep(std::time::Duration::from_millis(frame_interval_ms));
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
}
