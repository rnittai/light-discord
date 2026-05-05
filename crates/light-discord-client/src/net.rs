use light_discord_core::{ClientFrame, ServerFrame};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

#[derive(Debug)]
pub enum ClientEvent {
    Connected,
    Disconnected(String),
    Server(ServerFrame),
    Error(String),
}

pub struct NetworkHandle {
    command_tx: Sender<ClientFrame>,
    event_rx: Receiver<ClientEvent>,
}

impl NetworkHandle {
    pub fn connect(addr: String, auth_frame: ClientFrame) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<ClientFrame>();
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>();

        thread::spawn(move || run_network(addr, auth_frame, command_rx, event_tx));

        Self {
            command_tx,
            event_rx,
        }
    }

    pub fn send(&self, frame: ClientFrame) {
        let _ = self.command_tx.send(frame);
    }

    pub fn command_sender(&self) -> Sender<ClientFrame> {
        self.command_tx.clone()
    }

    pub fn poll(&self) -> Vec<ClientEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn shutdown(&self) {
        self.send(ClientFrame::Disconnect);
    }
}

fn run_network(
    addr: String,
    auth_frame: ClientFrame,
    command_rx: Receiver<ClientFrame>,
    event_tx: Sender<ClientEvent>,
) {
    let stream = match TcpStream::connect(&addr) {
        Ok(stream) => stream,
        Err(err) => {
            let _ = event_tx.send(ClientEvent::Error(format!("connect failed: {err}")));
            return;
        }
    };

    if let Err(err) = stream.set_read_timeout(Some(Duration::from_millis(100))) {
        let _ = event_tx.send(ClientEvent::Error(format!(
            "read timeout setup failed: {err}"
        )));
        return;
    }

    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(err) => {
            let _ = event_tx.send(ClientEvent::Error(format!("stream clone failed: {err}")));
            return;
        }
    };
    let mut reader = BufReader::new(stream);

    if let Err(err) = write_frame(&mut writer, &auth_frame) {
        let _ = event_tx.send(ClientEvent::Error(format!("authentication failed: {err}")));
        return;
    }

    let _ = event_tx.send(ClientEvent::Connected);

    loop {
        while let Ok(frame) = command_rx.try_recv() {
            let is_disconnect = matches!(frame, ClientFrame::Disconnect);
            if let Err(err) = write_frame(&mut writer, &frame) {
                let _ = event_tx.send(ClientEvent::Error(format!("send failed: {err}")));
                return;
            }
            if is_disconnect {
                let _ = event_tx.send(ClientEvent::Disconnected("closed by client".to_owned()));
                return;
            }
        }

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = event_tx.send(ClientEvent::Disconnected("server closed".to_owned()));
                return;
            }
            Ok(_) => match serde_json::from_str::<ServerFrame>(&line) {
                Ok(frame) => {
                    let _ = event_tx.send(ClientEvent::Server(frame));
                }
                Err(err) => {
                    let _ = event_tx.send(ClientEvent::Error(format!("bad server frame: {err}")));
                }
            },
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                thread::sleep(Duration::from_millis(16));
            }
            Err(err) => {
                let _ = event_tx.send(ClientEvent::Disconnected(format!("network error: {err}")));
                return;
            }
        }
    }
}

fn write_frame(writer: &mut TcpStream, frame: &ClientFrame) -> std::io::Result<()> {
    let line = serde_json::to_string(frame).map_err(std::io::Error::other)?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}
