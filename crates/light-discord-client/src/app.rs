use crate::{net::ClientEvent, net::NetworkHandle, voice::VoiceSession};
use eframe::egui;
use light_discord_core::{
    AuditLogSummary, ChatMessage, ClientFrame, ServerFrame, UserSummary, VoiceUser,
};
use light_discord_platform::{platform_info, PlatformInfo};

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
    network: Option<NetworkHandle>,
    voice: VoiceSession,
    platform: PlatformInfo,
}

impl LightDiscordApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            server_addr: "127.0.0.1:41610".to_owned(),
            display_name: whoami_fallback(),
            password: String::new(),
            invite_code: String::new(),
            session_token: String::new(),
            auth_mode: AuthMode::Login,
            status: "offline".to_owned(),
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
            network: None,
            voice: VoiceSession::default(),
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
        self.voice
            .start(udp_addr, user_id, self.voice_room_id.clone());
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
                ui.horizontal(|ui| {
                    if ui.button("Join").clicked() {
                        self.join_voice();
                    }
                    if ui.button("Leave").clicked() {
                        self.leave_voice();
                    }
                });
                for user in &self.voice_users {
                    ui.label(format!("- {}", user.display_name));
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

fn whoami_fallback() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "guest".to_owned())
}
