use crate::{
    fonts,
    net::ClientEvent,
    net::NetworkHandle,
    screen_share::{start_screen_share, ScreenShareEvent, ScreenShareSession, ScreenShareSettings},
    voice::{VoiceSession, VoiceShared},
};
use base64::Engine;
use eframe::egui;
use light_discord_core::{
    AuditLogSummary, ChatMessage, ClientFrame, ScreenShareMode, ScreenShareResolution, ServerFrame,
    UserId, UserSummary, VoiceUser, SCREEN_SHARE_CODEC_JPEG,
};
use light_discord_platform::{
    available_audio_devices, available_screen_sources, delete_session_token, load_session_token,
    platform_info, save_session_token, AudioDeviceInfo, AudioDeviceList, AudioDeviceSelection,
    PlatformInfo, ScreenShareSource,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Login,
    Register,
    Session,
    Dev,
}

struct RemoteScreenShare {
    display_name: String,
    source_name: String,
    width: u32,
    height: u32,
    mode: ScreenShareMode,
    resolution: ScreenShareResolution,
    target_fps: u32,
    codec: String,
    transport: String,
    latest_frame: Option<egui::ColorImage>,
    latest_sequence: Option<u64>,
    texture: Option<egui::TextureHandle>,
    status: String,
}

pub struct LightDiscordApp {
    server_addr: String,
    display_name: String,
    password: String,
    invite_code: String,
    session_token: String,
    session_token_status: String,
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
    screen_sources: Vec<ScreenShareSource>,
    selected_source_id: Option<String>,
    screen_share_mode: ScreenShareMode,
    screen_share_resolution: ScreenShareResolution,
    screen_share_game_fps: u32,
    screen_share_status: String,
    local_screen_share: Option<ScreenShareSession>,
    remote_screen_shares: HashMap<UserId, RemoteScreenShare>,
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

        let (screen_sources, screen_share_status) = load_screen_sources();

        let default_server = "127.0.0.1:41610";
        let (auth_mode, session_token, session_token_status) =
            load_default_session_token(default_server);

        Self {
            server_addr: default_server.to_owned(),
            display_name: whoami_fallback(),
            password: String::new(),
            invite_code: String::new(),
            session_token,
            session_token_status,
            auth_mode,
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
            screen_sources,
            selected_source_id: None,
            screen_share_mode: ScreenShareMode::Text,
            screen_share_resolution: ScreenShareResolution::P720,
            screen_share_game_fps: 30,
            screen_share_status,
            local_screen_share: None,
            remote_screen_shares: HashMap::new(),
        }
    }

    fn connect(&mut self) {
        if self.connected {
            return;
        }

        if self.auth_mode == AuthMode::Session && self.session_token.trim().is_empty() {
            self.status = "session token is empty".to_owned();
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
        if let Some(session) = &self.local_screen_share {
            session.stop();
        }
        self.local_screen_share = None;
        self.remote_screen_shares.clear();
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
                    if let Some(session) = &self.local_screen_share {
                        session.stop();
                    }
                    self.local_screen_share = None;
                    self.remote_screen_shares.clear();
                }
                ClientEvent::Error(message) => {
                    self.status = message;
                }
                ClientEvent::Server(frame) => self.apply_server_frame(frame),
            }
        }

        self.poll_screen_share_events();
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

    fn refresh_screen_sources(&mut self) {
        let (screen_sources, screen_share_status) = load_screen_sources();
        self.screen_sources = screen_sources;
        self.screen_share_status = screen_share_status;
        self.selected_source_id = None;
    }

    fn start_screen_share(&mut self) {
        let source = match self.selected_source_id.as_ref() {
            Some(source_id) => self
                .screen_sources
                .iter()
                .find(|s| &s.id == source_id)
                .cloned(),
            None => None,
        };

        let Some(source) = source else {
            self.screen_share_status = "no source selected".to_owned();
            return;
        };

        let Some(network) = &self.network else {
            self.screen_share_status = "not connected".to_owned();
            return;
        };

        let settings = ScreenShareSettings::new(
            self.screen_share_mode,
            self.screen_share_resolution,
            self.screen_share_game_fps,
        );
        let session = start_screen_share(source, network.command_sender(), settings);
        self.local_screen_share = Some(session);
        self.screen_share_status = format!(
            "sharing started: {} / {} / {} fps",
            screen_mode_label(settings.mode),
            screen_resolution_label(settings.resolution),
            settings.target_fps()
        );
    }

    fn stop_screen_share(&mut self) {
        if let Some(session) = &self.local_screen_share {
            session.stop();
        }
        self.local_screen_share = None;
        self.screen_share_status = "sharing stopped".to_owned();
    }

    fn poll_screen_share_events(&mut self) {
        if let Some(session) = &self.local_screen_share {
            for event in session.poll() {
                match event {
                    ScreenShareEvent::Error(err) => {
                        self.screen_share_status = format!("error: {err}");
                        self.local_screen_share = None;
                    }
                    ScreenShareEvent::Stopped => {
                        self.screen_share_status = "sharing stopped".to_owned();
                        self.local_screen_share = None;
                    }
                }
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
                if let Some(ref token) = session_token {
                    self.session_token = token.clone();
                    self.save_and_update_session_token(token);
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
            ServerFrame::ScreenShareStarted {
                user_id,
                display_name,
                source_name,
                width,
                height,
                mode,
                resolution,
                target_fps,
                active_codec,
                transport,
                ..
            } => {
                self.remote_screen_shares.insert(
                    user_id,
                    RemoteScreenShare {
                        display_name,
                        source_name,
                        width,
                        height,
                        mode,
                        resolution,
                        target_fps,
                        codec: active_codec,
                        transport,
                        latest_frame: None,
                        latest_sequence: None,
                        texture: None,
                        status: "waiting for frame".to_owned(),
                    },
                );
            }
            ServerFrame::ScreenShareStopped { user_id } => {
                self.remote_screen_shares.remove(&user_id);
            }
            ServerFrame::ScreenShareFrame {
                user_id,
                display_name,
                width,
                height,
                mode,
                resolution,
                target_fps,
                codec,
                transport,
                sequence,
                data_base64,
                ..
            } => {
                let remote_share =
                    self.remote_screen_shares
                        .entry(user_id)
                        .or_insert_with(|| RemoteScreenShare {
                            display_name: display_name.clone(),
                            source_name: "Screen share".to_owned(),
                            width,
                            height,
                            mode,
                            resolution,
                            target_fps,
                            codec: codec.clone(),
                            transport: transport.clone(),
                            latest_frame: None,
                            latest_sequence: None,
                            texture: None,
                            status: "waiting for frame".to_owned(),
                        });

                if remote_share
                    .latest_sequence
                    .is_some_and(|latest| sequence <= latest)
                {
                    return;
                }

                remote_share.display_name = display_name;
                remote_share.width = width;
                remote_share.height = height;
                remote_share.mode = mode;
                remote_share.resolution = resolution;
                remote_share.target_fps = target_fps;
                remote_share.codec = codec.clone();
                remote_share.transport = transport;
                remote_share.latest_sequence = Some(sequence);
                if codec != SCREEN_SHARE_CODEC_JPEG {
                    remote_share.latest_frame = None;
                    remote_share.status = format!("unsupported codec: {codec}");
                    return;
                }
                match base64::engine::general_purpose::STANDARD.decode(&data_base64) {
                    Ok(decoded) => match decode_remote_screen_frame(&decoded) {
                        Ok(frame) => {
                            remote_share.latest_frame = Some(frame);
                            remote_share.status = format!("frame {sequence}");
                        }
                        Err(err) => {
                            remote_share.latest_frame = None;
                            remote_share.status = format!("decode failed: {err}");
                        }
                    },
                    Err(err) => {
                        remote_share.latest_frame = None;
                        remote_share.status = format!("base64 failed: {err}");
                    }
                }
            }
            ServerFrame::Error { message } => {
                self.status = message;
            }
        }
    }

    fn save_and_update_session_token(&mut self, token: &str) {
        match save_session_token(&self.server_addr, token) {
            Ok(store) => {
                self.session_token_status = format!("saved to {}", store_name(store));
            }
            Err(_) => {
                self.session_token_status = "failed to save token".to_owned();
            }
        }
    }

    fn load_session_token_for_server(&mut self) {
        match load_session_token(&self.server_addr) {
            Ok(Some(stored)) => {
                self.session_token = stored.token;
                self.session_token_status = format!("loaded from {}", store_name(stored.store));
            }
            Ok(None) => {
                self.session_token_status = "no saved token found".to_owned();
            }
            Err(_) => {
                self.session_token_status = "failed to load token".to_owned();
            }
        }
    }

    fn forget_session_token(&mut self) {
        match delete_session_token(&self.server_addr) {
            Ok(()) => {
                self.session_token.clear();
                self.session_token_status = "token deleted".to_owned();
            }
            Err(_) => {
                self.session_token_status = "failed to delete token".to_owned();
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
                    if !self.connected {
                        if ui.button("Load").clicked() {
                            self.load_session_token_for_server();
                        }
                        if ui.button("Forget").clicked() {
                            self.forget_session_token();
                        }
                    }
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
                if !self.session_token_status.is_empty() {
                    ui.small(format!("session: {}", self.session_token_status));
                }
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
                ui.heading("Screen");
                let sharing = self
                    .local_screen_share
                    .as_ref()
                    .is_some_and(|s| s.is_running());
                let source_options: Vec<_> = self
                    .screen_sources
                    .iter()
                    .map(|s| (s.id.clone(), s.title.clone()))
                    .collect();
                if !source_options.is_empty() {
                    egui::ComboBox::from_id_source("screen_source")
                        .selected_text(
                            self.selected_source_id
                                .as_ref()
                                .and_then(|id| {
                                    source_options
                                        .iter()
                                        .find(|(s_id, _)| s_id == id)
                                        .map(|(_, title)| title.as_str())
                                })
                                .unwrap_or("Select source"),
                        )
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for (id, title) in source_options {
                                ui.selectable_value(&mut self.selected_source_id, Some(id), title);
                            }
                        });
                } else {
                    ui.label("No sources");
                }
                ui.add_enabled_ui(!sharing, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Mode");
                        egui::ComboBox::from_id_source("screen_share_mode")
                            .selected_text(screen_mode_label(self.screen_share_mode))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.screen_share_mode,
                                    ScreenShareMode::Text,
                                    screen_mode_label(ScreenShareMode::Text),
                                );
                                ui.selectable_value(
                                    &mut self.screen_share_mode,
                                    ScreenShareMode::Game,
                                    screen_mode_label(ScreenShareMode::Game),
                                );
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Resolution");
                        egui::ComboBox::from_id_source("screen_share_resolution")
                            .selected_text(screen_resolution_label(self.screen_share_resolution))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.screen_share_resolution,
                                    ScreenShareResolution::P1080,
                                    screen_resolution_label(ScreenShareResolution::P1080),
                                );
                                ui.selectable_value(
                                    &mut self.screen_share_resolution,
                                    ScreenShareResolution::P720,
                                    screen_resolution_label(ScreenShareResolution::P720),
                                );
                            });
                    });
                    if self.screen_share_mode == ScreenShareMode::Game {
                        ui.horizontal(|ui| {
                            ui.label("FPS");
                            ui.selectable_value(&mut self.screen_share_game_fps, 30, "30");
                            ui.selectable_value(&mut self.screen_share_game_fps, 60, "60");
                        });
                    } else {
                        ui.small("5 FPS");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.connected && self.selected_source_id.is_some() && !sharing,
                            egui::Button::new("Share"),
                        )
                        .clicked()
                    {
                        self.start_screen_share();
                    }
                    if ui.add_enabled(sharing, egui::Button::new("Stop")).clicked() {
                        self.stop_screen_share();
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh_screen_sources();
                    }
                });
                ui.small(&self.screen_share_status);

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

            if !self.remote_screen_shares.is_empty() {
                ui.heading("Screen Shares");
                for (user_id, remote_share) in &mut self.remote_screen_shares {
                    ui.label(format!(
                        "{} - {}",
                        remote_share.display_name, remote_share.source_name
                    ));

                    if let Some(frame) = remote_share.latest_frame.take() {
                        if let Some(texture) = &mut remote_share.texture {
                            texture.set(frame, egui::TextureOptions::LINEAR);
                        } else {
                            remote_share.texture = Some(ctx.load_texture(
                                format!("screen_{user_id}"),
                                frame,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }

                    if let Some(texture) = &remote_share.texture {
                        let texture_size = texture.size_vec2();
                        let max_width = ui.available_width().max(1.0);
                        let scale = (max_width / texture_size.x).min(1.0);
                        let display_size = texture_size * scale;
                        ui.image((texture.id(), display_size));
                    } else {
                        ui.label(&remote_share.status);
                    }
                    ui.small(format!(
                        "{}x{} / {} / {} fps / {} / {} / {} / {}",
                        remote_share.width,
                        remote_share.height,
                        screen_mode_label(remote_share.mode),
                        remote_share.target_fps,
                        screen_resolution_label(remote_share.resolution),
                        remote_share.codec,
                        remote_share.transport,
                        remote_share.status
                    ));
                    ui.separator();
                }
            }

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

fn load_default_session_token(server_addr: &str) -> (AuthMode, String, String) {
    match load_session_token(server_addr) {
        Ok(Some(stored)) => {
            let status = format!("loaded from {}", store_name(stored.store));
            (AuthMode::Session, stored.token, status)
        }
        Ok(None) => (AuthMode::Login, String::new(), String::new()),
        Err(_) => (AuthMode::Login, String::new(), String::new()),
    }
}

fn store_name(store: light_discord_platform::SessionTokenStore) -> &'static str {
    match store {
        light_discord_platform::SessionTokenStore::Keyring => "keyring",
        light_discord_platform::SessionTokenStore::File => "file",
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

fn screen_mode_label(mode: ScreenShareMode) -> &'static str {
    match mode {
        ScreenShareMode::Text => "Text",
        ScreenShareMode::Game => "Game",
    }
}

fn screen_resolution_label(resolution: ScreenShareResolution) -> &'static str {
    match resolution {
        ScreenShareResolution::P1080 => "1080p",
        ScreenShareResolution::P720 => "720p",
    }
}

fn whoami_fallback() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "guest".to_owned())
}

fn load_screen_sources() -> (Vec<ScreenShareSource>, String) {
    match available_screen_sources() {
        Ok(sources) => {
            let count = sources.len();
            let status = if count > 0 {
                format!("sources: {}", count)
            } else {
                "no sources available".to_owned()
            };
            (sources, status)
        }
        Err(err) => (Vec::new(), format!("sources unavailable: {err:#}")),
    }
}

fn decode_remote_screen_frame(frame_data: &[u8]) -> Result<egui::ColorImage, image::ImageError> {
    let img = image::load_from_memory(frame_data)?;
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba_img.as_raw(),
    ))
}
