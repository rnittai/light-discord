use light_discord_core::VoicePacket;
use light_discord_platform::AudioDeviceSelection;
use std::{
    net::UdpSocket,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

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
        self.selected_devices = selected_devices;

        let (stop_tx, stop_rx) = mpsc::channel::<VoiceCommand>();
        let worker = thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(socket) => socket,
                Err(_) => return,
            };
            if socket.connect(udp_addr).is_err() {
                return;
            }

            let mut sequence = 0_u64;
            loop {
                if matches!(stop_rx.try_recv(), Ok(VoiceCommand::Stop)) {
                    return;
                }

                let packet = VoicePacket {
                    user_id: user_id.clone(),
                    room_id: room_id.clone(),
                    sequence,
                    sample_rate: 48_000,
                    channels: 1,
                    payload: Vec::new(),
                };

                if let Ok(bytes) = serde_json::to_vec(&packet) {
                    let _ = socket.send(&bytes);
                }
                sequence = sequence.wrapping_add(1);
                thread::sleep(Duration::from_millis(500));
            }
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
