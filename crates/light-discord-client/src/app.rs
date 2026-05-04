use crate::{
    fonts,
    net::ClientEvent,
    net::NetworkHandle,
    voice::{VoiceSession, VoiceShared},
};
use eframe::egui;
use light_discord_core::{
    AuditLogSummary, ChatMessage, ClientFrame, ServerFrame, UserSummary, VoiceUser,
};
use light_discord_platform::{
    available_audio_devices, platform_info, AudioDeviceInfo, AudioDeviceList, AudioDeviceSelection,
    PlatformInfo,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Login,
    Register,
    Session,
    Dev,
}

pub struct LightDiscordApp {
    server_addr: String,
    display_name: String,
    password: String,
    invite_code: String,
    session_token: String,
    auth_mode: AuthMode,
    status: String,
    channel_id: String,
    voice_room_id: String,
    input: String,
    connected: bool,
    is_admin: bool,
    user_id: Option<String>,
    udp_voice_addr: Option<String>,
    users: Vec<UserSummary>,
    voice_users: Vec<VoiceUser>,
    messages: Vec<ChatMessage>,
    audit_entries: Vec<AuditLogSummary>,
    invite_note: String,
    latest_invite_code: String,
    audio_devices: AudioDeviceList,
    selected_input_device_id: Option<String>,
    selected_output_device_id: Option<String>,
    audio_device_status: String,
    network: Option<NetworkHandle>,
    voice: VoiceSession,
    voice_shared: Arc<VoiceShared>,
    voice_muted: bool,
    voice_deafened: bool,
    platform: PlatformInfo,
}

impl LightDiscordApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let status = match fonts::configure_japanese_fonts(&cc.egui_ctx) {
            Some(path) => format!("offline / font: {}", path.display()),
            None => "offline / Japanese font not found".to_owned(),
        };
        let (
            audio_devices,
            selected_input_device_id,
            selected_output_device_id,
            audio_device_status,
        ) = load_audio_devices();

        Self {
            server_addr: "127.0.0.1:41610".to_owned(),
            display_name: whoami_fallback(),
            password: String::new(),
            invite_code: String::new(),
            session_token: String::new(),
            auth_mode: AuthMode::Login,
            status,
            channel_id: "general".to_owned(),
            voice_room_id: "voice-general".to_owned(),
            input: String::new(),
            connected: false,
            is_admin: false,
            user_id: None,
            udp_voice_addr: None,
            users: Vec::new(),
            voice_users: Vec::new(),
            messages: Vec::new(),
            audit_entries: Vec::new(),
            invite_note: String::new(),
            latest_invite_code: String::new(),
            audio_devices,
            selected_input_device_id,
            selected_output_device_id,
            audio_device_status,
            network: None,
            voice: VoiceSession::default(),
            voice_shared: VoiceShared::new(),
            voice_muted: false,
            voice_deafened: false,
            platform: platform_info(),
        }
    }

    fn connect(&mut self) {
        if self.connected {
            return;
        }
        self.status = "connecting".to_owned();
        self.messages.clear();
        self.users.clear();
        self.voice_users.clear();
        self.audit_entries.clear();
        self.latest_invite_code.clear();
        self.network = Some(NetworkHandle::connect(
            self.server_addr.clone(),
            self.auth_frame(),
        ));
    }

    fn disconnect(&mut self) {
        if let Some(network) = &self.network {
            network.shutdown();
        }
        self.network = None;
        self.connected = false;
        self.is_admin = false;
        self.user_id = None;
        self.udp_voice_addr = None;
        self.users.clear();
        self.voice_users.clear();
        self.voice.stop();
        self.status = "offline".to_owned();
    }

    fn auth_frame(&self) -> ClientFrame {
        match self.auth_mode {
            AuthMode::Login => ClientFrame::Login {
                display_name: self.display_name.clone(),
                password: self.password.clone(),
            },
            AuthMode::Register => ClientFrame::Register {
                invite_code: self.invite_code.clone(),
                display_name: self.display_name.clone(),
                password: self.password.clone(),
            },
            AuthMode::Session => ClientFrame::ResumeSession {
                session_token: self.session_token.clone(),
            },
            AuthMode::Dev => ClientFrame::Hello {
                display_name: self.display_name.clone(),
            },
        }
    }

    fn send_message(&mut self) {
        let body = self.input.trim().to_owned();
        if body.is_empty() {
            return;
        }

        if let Some(network) = &self.network {
            network.send(ClientFrame::SendMessage {
                channel_id: self.channel_id.clone(),
                body,
            });
            self.input.clear();
        }
    }

    fn delete_message(&mut self, message_id: String) {
        if let Some(network) = &self.network {
            network.send(ClientFrame::DeleteMessage { message_id });
        }
    }

    fn request_audit_log(&mut self) {
        if let Some(network) = &self.network {
            network.send(ClientFrame::AdminListAuditLog { limit: 50 });
        }
    }

    fn create_invite(&mut self) {
        if let Some(network) = &self.network {
            network.send(ClientFrame::AdminCreateInvite {
                note: self.invite_note.clone(),
            });
        }
    }

    fn join_voice(&mut self) {
        let Some(network) = &self.network else {
            return;
        };
        let Some(user_id) = self.user_id.clone() else {
            return;
        };
        let Some(udp_addr) = self.udp_voice_addr.clone() else {
            return;
        };

        network.send(ClientFrame::JoinVoice {
            room_id: self.voice_room_id.clone(),
        });
        // Push current toggle state into the shared bag before the worker
        // starts so it picks up the user's previous mute/deafen choice.
        self.voice_shared.set_muted(self.voice_muted);
        self.voice_shared.set_deafened(self.voice_deafened);
        self.voice.start(
            udp_addr,
            user_id,
            self.voice_room_id.clone(),
            AudioDeviceSelection {
                input_device_id: self.selected_input_device_id.clone(),
                output_device_id: self.selected_output_device_id.clone(),
            },
            Arc::clone(&self.voice_shared),
        );
    }

    fn leave_voice(&mut self) {
        if let Some(network) = &self.network {
            network.send(ClientFrame::LeaveVoice);
        }
        self.voice.stop();
    }

    fn poll_network(&mut self) {
        let events = match &self.network {
            Some(network) => network.poll(),
            None => return,
        };

        for event in events {
            match event {
                ClientEvent::Connected => {
                    self.status = "connected".to_owned();
                    self.connected = true;
                }
                ClientEvent::Disconnected(reason) => {
                    self.status = reason;
                    self.connected = false;
                    self.voice.stop();
                }
                ClientEvent::Error(message) => {
                    self.status = message;
                }
                ClientEvent::Server(frame) => self.apply_server_frame(frame),
            }
        }
    }

    fn refresh_audio_devices(&mut self) {
        let previous_input = self.selected_input_device_id.clone();
        let previous_output = self.selected_output_device_id.clone();

        match available_audio_devices() {
            Ok(devices) => {
                self.selected_input_device_id =
                    select_existing_or_first(previous_input, &devices.inputs);
                self.selected_output_device_id =
                    select_existing_or_first(previous_output, &devices.outputs);
                let input_count = devices.inputs.len();
                let output_count = devices.outputs.len();
                self.audio_devices = devices;
                self.audio_device_status =
                    format!("audio devices: {input_count} input / {output_count} output");
            }
            Err(err) => {
                self.audio_devices = AudioDeviceList::default();
                self.selected_input_device_id = None;
                self.selected_output_device_id = None;
                self.audio_device_status = format!("audio devices unavailable: {err:#}");
            }
        }
    }

    fn apply_server_frame(&mut self, frame: ServerFrame) {
        match frame {
            ServerFrame::Welcome {
                user_id,
                default_channel,
                udp_voice_addr,
                session_token,
                is_admin,
                ..
            } => {
                self.user_id = Some(user_id);
                self.is_admin = is_admin;
                self.channel_id = default_channel;
                self.udp_voice_addr = Some(udp_voice_addr);
                if let Some(session_token) = session_token {
                    self.session_token = session_token;
                }
                self.connected = true;
                self.status = "connected".to_owned();
            }
            ServerFrame::InviteCreated { code } => {
                self.latest_invite_code = code;
                self.status = "invite created".to_owned();
            }
            ServerFrame::AuditLog { entries } => {
                self.audit_entries = entries;
                self.status = "audit log loaded".to_owned();
            }
            ServerFrame::ChannelJoined { channel_id } => {
                self.channel_id = channel_id;
                self.messages.clear();
            }
            ServerFrame::UserList { users } => {
                self.users = users;
            }
            ServerFrame::Message(message) => {
                if message.channel_id == self.channel_id {
                    self.messages.push(message);
                }
            }
            ServerFrame::MessageDeleted {
                message_id,
                channel_id,
                ..
            } => {
                if channel_id == self.channel_id {
                    self.messages.retain(|message| message.id != message_id);
                }
            }
            ServerFrame::VoiceState { room_id, users } => {
                if room_id == self.voice_room_id {
                    self.voice_users = users;
                }
            }
            ServerFrame::Error { message } => {
                self.status = message;
            }
        }
    }
}

impl eframe::App for LightDiscordApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_network();

        egui::TopBottomPanel::top("connection_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Server");
                ui.add_enabled(
                    !self.connected,
                    egui::TextEdit::singleline(&mut self.server_addr).desired_width(160.0),
                );
                ui.add_enabled_ui(!self.connected, |ui| {
                    ui.selectable_value(&mut self.auth_mode, AuthMode::Login, "Login");
                    ui.selectable_value(&mut self.auth_mode, AuthMode::Register, "Register");
                    ui.selectable_value(&mut self.auth_mode, AuthMode::Session, "Session");
                    ui.selectable_value(&mut self.auth_mode, AuthMode::Dev, "Dev");
                });
                ui.label("Name");
                ui.add_enabled(
                    !self.connected && self.auth_mode != AuthMode::Session,
                    egui::TextEdit::singleline(&mut self.display_name).desired_width(120.0),
                );
                if matches!(self.auth_mode, AuthMode::Login | AuthMode::Register) {
                    ui.add_enabled(
                        !self.connected,
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .hint_text("Password")
                            .desired_width(120.0),
                    );
                }
                if self.auth_mode == AuthMode::Register {
                    ui.add_enabled(
                        !self.connected,
                        egui::TextEdit::singleline(&mut self.invite_code)
                            .hint_text("Invite")
                            .desired_width(160.0),
                    );
                }
                if self.auth_mode == AuthMode::Session {
                    ui.add_enabled(
                        !self.connected,
                        egui::TextEdit::singleline(&mut self.session_token)
                            .hint_text("Session token")
                            .desired_width(220.0),
                    );
                }

                if self.connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                } else if ui.button("Connect").clicked() {
                    self.connect();
                }

                ui.separator();
                let role = if self.is_admin { "admin" } else { "user" };
                ui.label(format!("{} / {} / {}", self.status, self.platform.os, role));
            });
        });

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Channels");
                if ui
                    .selectable_label(self.channel_id == "general", "# general")
                    .clicked()
                {
                    self.channel_id = "general".to_owned();
                    if let Some(network) = &self.network {
                        network.send(ClientFrame::JoinChannel {
                            channel_id: self.channel_id.clone(),
                        });
                    }
                }

                ui.separator();
                ui.heading("Voice");
                let voice_running = self.voice.is_running();
                ui.add_enabled_ui(!voice_running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Input");
                        device_combo(
                            ui,
                            "voice_input_device",
                            &mut self.selected_input_device_id,
                            &self.audio_devices.inputs,
                            "No input devices",
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Output");
                        device_combo(
                            ui,
                            "voice_output_device",
                            &mut self.selected_output_device_id,
                            &self.audio_devices.outputs,
                            "No output devices",
                        );
                    });
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.connected && !voice_running, egui::Button::new("Join"))
                        .clicked()
                    {
                        self.join_voice();
                    }
                    if ui
                        .add_enabled(voice_running, egui::Button::new("Leave"))
                        .clicked()
                    {
                        self.leave_voice();
                    }
                    if ui
                        .add_enabled(!voice_running, egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.refresh_audio_devices();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.toggle_value(&mut self.voice_muted, "Mute mic").changed() {
                        self.voice_shared.set_muted(self.voice_muted);
                    }
                    if ui
                        .toggle_value(&mut self.voice_deafened, "Deafen")
                        .changed()
                    {
                        // Deafening implies you can't hear anything; we also
                        // mute the mic to match user expectations from other
                        // voice apps.
                        self.voice_shared.set_deafened(self.voice_deafened);
                        if self.voice_deafened && !self.voice_muted {
                            self.voice_muted = true;
                            self.voice_shared.set_muted(true);
                        }
                    }
                });
                ui.small(&self.audio_device_status);
                let local_user_id = self.user_id.as_deref();
                for user in &self.voice_users {
                    let active = self.voice_shared.is_active(&user.user_id);
                    let is_local = local_user_id == Some(user.user_id.as_str());
                    let mut text = egui::RichText::new(format!(
                        "{} {}",
                        if active { "*" } else { "-" },
                        user.display_name
                    ));
                    if active {
                        text = text.color(egui::Color32::from_rgb(80, 200, 120)).strong();
                    }
                    if is_local {
                        text = text.italics();
                    }
                    ui.label(text);
                }

                ui.separator();
                ui.heading("Online");
                for user in &self.users {
                    ui.label(&user.display_name);
                }

                if self.is_admin {
                    ui.separator();
                    ui.heading("Admin");
                    ui.horizontal(|ui| {
                        if ui.button("Audit").clicked() {
                            self.request_audit_log();
                        }
                        if ui.button("Invite").clicked() {
                            self.create_invite();
                        }
                    });
                    ui.text_edit_singleline(&mut self.invite_note);
                    if !self.latest_invite_code.is_empty() {
                        ui.label(&self.latest_invite_code);
                    }
                }
            });

        egui::TopBottomPanel::bottom("message_composer")
            .resizable(false)
            .exact_height(48.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(format!("# {}", self.channel_id));
                    let response = ui.add_enabled(
                        self.connected,
                        egui::TextEdit::singleline(&mut self.input)
                            .hint_text("Message")
                            .desired_width(f32::INFINITY),
                    );
                    let pressed_enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(self.connected, egui::Button::new("Send"))
                        .clicked()
                        || (self.connected && pressed_enter)
                    {
                        self.send_message();
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("# {}", self.channel_id));
            ui.separator();

            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut delete_requested = None;
                    let own_user_id = self.user_id.as_deref();

                    for message in &self.messages {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&message.display_name);
                            ui.label(&message.body);
                            if own_user_id == Some(message.user_id.as_str())
                                && ui.small_button("Delete").clicked()
                            {
                                delete_requested = Some(message.id.clone());
                            }
                        });
                    }

                    if let Some(message_id) = delete_requested {
                        self.delete_message(message_id);
                    }
                });

            ui.separator();
            if self.is_admin && !self.audit_entries.is_empty() {
                ui.collapsing("Audit log", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for entry in &self.audit_entries {
                                ui.horizontal_wrapped(|ui| {
                                    ui.strong(&entry.action);
                                    ui.label(&entry.actor_user_id);
                                    if let Some(message_id) = &entry.target_message_id {
                                        ui.label(message_id);
                                    }
                                    if let Some(snapshot) = &entry.message_body_snapshot {
                                        ui.label(snapshot);
                                    }
                                });
                            }
                        });
                });
                ui.separator();
            }
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn load_audio_devices() -> (AudioDeviceList, Option<String>, Option<String>, String) {
    match available_audio_devices() {
        Ok(devices) => {
            let selected_input = select_existing_or_first(None, &devices.inputs);
            let selected_output = select_existing_or_first(None, &devices.outputs);
            let status = format!(
                "audio devices: {} input / {} output",
                devices.inputs.len(),
                devices.outputs.len()
            );
            (devices, selected_input, selected_output, status)
        }
        Err(err) => (
            AudioDeviceList::default(),
            None,
            None,
            format!("audio devices unavailable: {err:#}"),
        ),
    }
}

fn select_existing_or_first(
    previous: Option<String>,
    devices: &[AudioDeviceInfo],
) -> Option<String> {
    if let Some(previous) = previous {
        if devices.iter().any(|device| device.id == previous) {
            return Some(previous);
        }
    }

    devices.first().map(|device| device.id.clone())
}

fn device_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    selected: &mut Option<String>,
    devices: &[AudioDeviceInfo],
    empty_label: &'static str,
) {
    let selected_text = selected_device_label(selected.as_deref(), devices, empty_label);

    egui::ComboBox::from_id_source(id)
        .selected_text(selected_text)
        .width(160.0)
        .show_ui(ui, |ui| {
            if devices.is_empty() {
                ui.label(empty_label);
                return;
            }

            for device in devices {
                ui.selectable_value(
                    selected,
                    Some(device.id.clone()),
                    audio_device_label(device),
                );
            }
        });
}

fn selected_device_label(
    selected: Option<&str>,
    devices: &[AudioDeviceInfo],
    empty_label: &'static str,
) -> String {
    selected
        .and_then(|id| devices.iter().find(|device| device.id == id))
        .map(audio_device_label)
        .unwrap_or_else(|| empty_label.to_owned())
}

fn audio_device_label(device: &AudioDeviceInfo) -> String {
    match (device.is_default, device.grouped_count > 1) {
        (true, true) => format!(
            "{} (default, {} variants)",
            device.name, device.grouped_count
        ),
        (true, false) => format!("{} (default)", device.name),
        (false, true) => format!("{} ({} variants)", device.name, device.grouped_count),
        (false, false) => device.name.clone(),
    }
}

fn whoami_fallback() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "guest".to_owned())
}
