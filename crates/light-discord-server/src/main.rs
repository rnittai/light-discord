use anyhow::{Context, Result};
use light_discord_auth::{
    hash_password, hash_token, new_invite_code, new_session_token, verify_password,
};
use light_discord_core::{
    decode_voice_packet_binary, ChannelId, ChatMessage, ClientFrame, RoomId, ServerFrame, UserId,
    UserSummary, VoiceUser,
};
use light_discord_storage::{CreateAccountResult, DeleteMessageResult, Storage};
use std::{
    collections::{HashMap, HashSet},
    env,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, RwLock},
    time,
};
use uuid::Uuid;

type ServerTx = mpsc::UnboundedSender<ServerFrame>;

#[derive(Clone)]
struct AppState {
    inner: Arc<RwLock<InnerState>>,
    storage: Storage,
}

struct InnerState {
    users: HashMap<UserId, UserConnection>,
    voice_rooms: HashMap<RoomId, HashSet<UserId>>,
    voice_addrs: HashMap<UserId, SocketAddr>,
}

struct UserConnection {
    display_name: String,
    is_admin: bool,
    tx: ServerTx,
}

struct AuthConfig {
    allow_dev_auth: bool,
}

struct AuthenticatedUser {
    user_id: String,
    display_name: String,
    is_admin: bool,
    session_token: Option<String>,
}

impl AppState {
    fn new(storage: Storage) -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerState {
                users: HashMap::new(),
                voice_rooms: HashMap::new(),
                voice_addrs: HashMap::new(),
            })),
            storage,
        }
    }

    async fn register_connection(
        &self,
        user_id: UserId,
        display_name: String,
        is_admin: bool,
        tx: ServerTx,
    ) {
        let mut inner = self.inner.write().await;
        inner.users.insert(
            user_id,
            UserConnection {
                display_name,
                is_admin,
                tx,
            },
        );
    }

    async fn disconnect_user(&self, user_id: &str) {
        let changed_rooms = {
            let mut inner = self.inner.write().await;
            inner.users.remove(user_id);
            inner.voice_addrs.remove(user_id);

            let mut changed_rooms = Vec::new();
            for (room_id, members) in &mut inner.voice_rooms {
                if members.remove(user_id) {
                    changed_rooms.push(room_id.clone());
                }
            }
            changed_rooms
        };

        self.broadcast_user_list().await;
        for room_id in changed_rooms {
            self.broadcast_voice_state(&room_id).await;
        }
    }

    async fn send_to(&self, user_id: &str, frame: ServerFrame) {
        let tx = {
            let inner = self.inner.read().await;
            inner.users.get(user_id).map(|user| user.tx.clone())
        };

        if let Some(tx) = tx {
            let _ = tx.send(frame);
        }
    }

    async fn broadcast(&self, frame: ServerFrame) {
        let targets = {
            let inner = self.inner.read().await;
            inner
                .users
                .values()
                .map(|user| user.tx.clone())
                .collect::<Vec<_>>()
        };

        for tx in targets {
            let _ = tx.send(frame.clone());
        }
    }

    async fn broadcast_user_list(&self) {
        let users = {
            let inner = self.inner.read().await;
            inner
                .users
                .iter()
                .map(|(user_id, user)| UserSummary {
                    user_id: user_id.clone(),
                    display_name: user.display_name.clone(),
                })
                .collect::<Vec<_>>()
        };

        self.broadcast(ServerFrame::UserList { users }).await;
    }

    async fn send_channel_history(&self, user_id: &str, channel_id: &str) {
        let messages = match self.storage.recent_messages(channel_id, 200).await {
            Ok(messages) => messages,
            Err(err) => {
                self.send_to(
                    user_id,
                    ServerFrame::Error {
                        message: format!("failed to load channel history: {err}"),
                    },
                )
                .await;
                Vec::new()
            }
        };

        self.send_to(
            user_id,
            ServerFrame::ChannelJoined {
                channel_id: channel_id.to_owned(),
            },
        )
        .await;

        for message in messages {
            self.send_to(user_id, ServerFrame::Message(message)).await;
        }
    }

    async fn send_chat_message(&self, user_id: &str, channel_id: ChannelId, body: String) {
        let display_name = {
            let inner = self.inner.read().await;
            inner
                .users
                .get(user_id)
                .map(|user| user.display_name.clone())
        };

        let Some(display_name) = display_name else {
            return;
        };

        let trimmed = body.trim();
        if trimmed.is_empty() {
            return;
        }

        let message = ChatMessage::new(
            Uuid::new_v4().to_string(),
            channel_id.clone(),
            user_id.to_owned(),
            display_name,
            trimmed.to_owned(),
        );

        if let Err(err) = self.storage.save_message(&message).await {
            self.send_to(
                user_id,
                ServerFrame::Error {
                    message: format!("failed to save message: {err}"),
                },
            )
            .await;
            return;
        }

        self.broadcast(ServerFrame::Message(message)).await;
    }

    async fn delete_chat_message(&self, user_id: &str, message_id: String) {
        let actor_is_admin = self.is_admin(user_id).await;
        match self
            .storage
            .soft_delete_message(&message_id, user_id, actor_is_admin)
            .await
        {
            Ok(DeleteMessageResult::Deleted(deleted)) => {
                self.broadcast(ServerFrame::MessageDeleted {
                    message_id: deleted.message_id,
                    channel_id: deleted.channel_id,
                    deleted_by: deleted.deleted_by_user_id,
                    unix_ms: deleted.deleted_at_unix_ms,
                })
                .await;
            }
            Ok(DeleteMessageResult::Forbidden) => {
                self.send_to(
                    user_id,
                    ServerFrame::Error {
                        message: "you can only delete your own messages".to_owned(),
                    },
                )
                .await;
            }
            Ok(DeleteMessageResult::NotFound) => {
                self.send_to(
                    user_id,
                    ServerFrame::Error {
                        message: "message not found".to_owned(),
                    },
                )
                .await;
            }
            Err(err) => {
                self.send_to(
                    user_id,
                    ServerFrame::Error {
                        message: format!("failed to delete message: {err}"),
                    },
                )
                .await;
            }
        }
    }

    async fn create_invite(&self, user_id: &str, note: String) {
        if !self.is_admin(user_id).await {
            self.send_to(
                user_id,
                ServerFrame::Error {
                    message: "admin permission required".to_owned(),
                },
            )
            .await;
            return;
        }

        let code = new_invite_code();
        let code_hash = hash_token(&code);
        if let Err(err) = self
            .storage
            .create_invite(&code_hash, Some(user_id), note.trim())
            .await
        {
            self.send_to(
                user_id,
                ServerFrame::Error {
                    message: format!("failed to create invite: {err}"),
                },
            )
            .await;
            return;
        }

        self.send_to(user_id, ServerFrame::InviteCreated { code })
            .await;
    }

    async fn send_audit_log(&self, user_id: &str, limit: usize) {
        if !self.is_admin(user_id).await {
            self.send_to(
                user_id,
                ServerFrame::Error {
                    message: "admin permission required".to_owned(),
                },
            )
            .await;
            return;
        }

        match self.storage.audit_log(limit.clamp(1, 200)).await {
            Ok(entries) => {
                self.send_to(user_id, ServerFrame::AuditLog { entries })
                    .await
            }
            Err(err) => {
                self.send_to(
                    user_id,
                    ServerFrame::Error {
                        message: format!("failed to load audit log: {err}"),
                    },
                )
                .await;
            }
        }
    }

    async fn is_admin(&self, user_id: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .users
            .get(user_id)
            .map(|user| user.is_admin)
            .unwrap_or(false)
    }

    async fn join_voice(&self, user_id: &str, room_id: RoomId) {
        {
            let mut inner = self.inner.write().await;
            for members in inner.voice_rooms.values_mut() {
                members.remove(user_id);
            }
            inner
                .voice_rooms
                .entry(room_id.clone())
                .or_default()
                .insert(user_id.to_owned());
        }

        self.broadcast_voice_state(&room_id).await;
    }

    async fn leave_voice(&self, user_id: &str) {
        let changed_rooms = {
            let mut inner = self.inner.write().await;
            let mut changed_rooms = Vec::new();
            for (room_id, members) in &mut inner.voice_rooms {
                if members.remove(user_id) {
                    changed_rooms.push(room_id.clone());
                }
            }
            changed_rooms
        };

        for room_id in changed_rooms {
            self.broadcast_voice_state(&room_id).await;
        }
    }

    async fn broadcast_voice_state(&self, room_id: &str) {
        let users = {
            let inner = self.inner.read().await;
            inner
                .voice_rooms
                .get(room_id)
                .into_iter()
                .flat_map(|members| members.iter())
                .filter_map(|user_id| {
                    inner.users.get(user_id).map(|user| VoiceUser {
                        user_id: user_id.clone(),
                        display_name: user.display_name.clone(),
                    })
                })
                .collect::<Vec<_>>()
        };

        self.broadcast(ServerFrame::VoiceState {
            room_id: room_id.to_owned(),
            users,
        })
        .await;
    }

    async fn remember_voice_addr(&self, user_id: &str, addr: SocketAddr) {
        let mut inner = self.inner.write().await;
        inner.voice_addrs.insert(user_id.to_owned(), addr);
    }

    async fn voice_targets(&self, room_id: &str, sender_id: &str) -> Vec<SocketAddr> {
        let inner = self.inner.read().await;
        let Some(members) = inner.voice_rooms.get(room_id) else {
            return Vec::new();
        };

        members
            .iter()
            .filter(|user_id| user_id.as_str() != sender_id)
            .filter_map(|user_id| inner.voice_addrs.get(user_id).copied())
            .collect()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let tcp_bind = env::var("LD_TCP_BIND").unwrap_or_else(|_| "127.0.0.1:41610".to_owned());
    let udp_bind = env::var("LD_UDP_BIND").unwrap_or_else(|_| "127.0.0.1:41611".to_owned());
    let storage = Storage::from_env().await?;
    storage.purge_expired().await?;
    bootstrap_auth(&storage).await?;
    let auth_config = Arc::new(AuthConfig {
        allow_dev_auth: allow_dev_auth(&storage),
    });
    tokio::spawn(run_retention(storage.clone()));

    let state = AppState::new(storage.clone());
    let udp_socket = Arc::new(
        UdpSocket::bind(&udp_bind)
            .await
            .with_context(|| format!("failed to bind UDP voice relay on {udp_bind}"))?,
    );
    let udp_addr = udp_socket
        .local_addr()
        .context("failed to read UDP voice relay address")?;

    tokio::spawn(run_voice_relay(udp_socket, state.clone()));

    let listener = TcpListener::bind(&tcp_bind)
        .await
        .with_context(|| format!("failed to bind TCP chat server on {tcp_bind}"))?;
    println!(
        "light-discord server listening on {tcp_bind}, voice UDP on {udp_addr}, storage={}",
        storage.backend_name()
    );

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        tokio::spawn(handle_client(
            stream,
            peer_addr,
            state.clone(),
            udp_addr.to_string(),
            auth_config.clone(),
        ));
    }
}

async fn handle_client(
    stream: TcpStream,
    peer_addr: SocketAddr,
    state: AppState,
    udp_voice_addr: String,
    auth_config: Arc<AuthConfig>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut writer = BufWriter::new(writer);

    let Some(first_line) = lines.next_line().await? else {
        return Ok(());
    };
    let auth_frame = serde_json::from_str::<ClientFrame>(&first_line)
        .with_context(|| format!("invalid hello frame from {peer_addr}"))?;

    let authenticated = match authenticate_first_frame(auth_frame, &state, &auth_config).await {
        Ok(authenticated) => authenticated,
        Err(message) => {
            write_server_frame(&mut writer, &ServerFrame::Error { message }).await?;
            return Ok(());
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerFrame>();

    state
        .register_connection(
            authenticated.user_id.clone(),
            authenticated.display_name.clone(),
            authenticated.is_admin,
            tx.clone(),
        )
        .await;

    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let line = serde_json::to_string(&frame)?;
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
        Ok::<(), anyhow::Error>(())
    });

    let _ = tx.send(ServerFrame::Welcome {
        user_id: authenticated.user_id.clone(),
        server_name: "Light Discord Local".to_owned(),
        default_channel: "general".to_owned(),
        udp_voice_addr,
        session_token: authenticated.session_token.clone(),
        is_admin: authenticated.is_admin,
    });
    state.broadcast_user_list().await;
    state
        .send_channel_history(&authenticated.user_id, "general")
        .await;
    state
        .broadcast(ServerFrame::Message(ChatMessage::new(
            Uuid::new_v4().to_string(),
            "general",
            "server",
            "server",
            format!("{} joined", authenticated.display_name),
        )))
        .await;

    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<ClientFrame>(&line) {
            Ok(
                ClientFrame::Hello { .. }
                | ClientFrame::Register { .. }
                | ClientFrame::Login { .. }
                | ClientFrame::ResumeSession { .. },
            ) => {
                state
                    .send_to(
                        &authenticated.user_id,
                        ServerFrame::Error {
                            message: "authentication has already been processed".to_owned(),
                        },
                    )
                    .await;
            }
            Ok(ClientFrame::JoinChannel { channel_id }) => {
                state
                    .send_channel_history(&authenticated.user_id, &channel_id)
                    .await;
            }
            Ok(ClientFrame::SendMessage { channel_id, body }) => {
                state
                    .send_chat_message(&authenticated.user_id, channel_id, body)
                    .await;
            }
            Ok(ClientFrame::DeleteMessage { message_id }) => {
                state
                    .delete_chat_message(&authenticated.user_id, message_id)
                    .await;
            }
            Ok(ClientFrame::AdminListAuditLog { limit }) => {
                state.send_audit_log(&authenticated.user_id, limit).await;
            }
            Ok(ClientFrame::AdminCreateInvite { note }) => {
                state.create_invite(&authenticated.user_id, note).await;
            }
            Ok(ClientFrame::JoinVoice { room_id }) => {
                state.join_voice(&authenticated.user_id, room_id).await;
            }
            Ok(ClientFrame::LeaveVoice) => {
                state.leave_voice(&authenticated.user_id).await;
            }
            Ok(ClientFrame::VoiceHeartbeat { room_id }) => {
                state.join_voice(&authenticated.user_id, room_id).await;
            }
            Ok(ClientFrame::Disconnect) => break,
            Err(err) => {
                state
                    .send_to(
                        &authenticated.user_id,
                        ServerFrame::Error {
                            message: format!("invalid client frame: {err}"),
                        },
                    )
                    .await;
            }
        }
    }

    state.disconnect_user(&authenticated.user_id).await;
    writer_task.abort();
    println!(
        "{} disconnected from {peer_addr}",
        authenticated.display_name
    );
    Ok(())
}

async fn authenticate_first_frame(
    frame: ClientFrame,
    state: &AppState,
    auth_config: &AuthConfig,
) -> Result<AuthenticatedUser, String> {
    match frame {
        ClientFrame::Hello { display_name } if auth_config.allow_dev_auth => {
            let display_name = normalize_display_name(display_name);
            let user_id = Uuid::new_v4().to_string();
            state
                .storage
                .register_ephemeral_user(&user_id, &display_name)
                .await
                .map_err(|err| format!("failed to create dev user: {err}"))?;
            Ok(AuthenticatedUser {
                user_id,
                display_name,
                is_admin: false,
                session_token: None,
            })
        }
        ClientFrame::Hello { .. } => Err("login required; dev auth is disabled".to_owned()),
        ClientFrame::Register {
            invite_code,
            display_name,
            password,
        } => {
            let display_name = normalize_display_name(display_name);
            validate_password(&password)?;
            let password_hash = hash_password(&password)
                .map_err(|err| format!("failed to hash password: {err}"))?;
            let invite_hash = hash_token(invite_code.trim());
            match state
                .storage
                .create_user_with_invite(&display_name, &password_hash, &invite_hash)
                .await
                .map_err(|err| format!("registration failed: {err}"))?
            {
                CreateAccountResult::Created(account) => authenticated_with_session(state, account)
                    .await
                    .map_err(|err| format!("session creation failed: {err}")),
                CreateAccountResult::DisplayNameTaken => {
                    Err("display name is already taken".to_owned())
                }
                CreateAccountResult::InvalidInvite => Err("invite code is invalid".to_owned()),
            }
        }
        ClientFrame::Login {
            display_name,
            password,
        } => {
            let Some(account) = state
                .storage
                .find_user_by_display_name(&display_name)
                .await
                .map_err(|err| format!("login failed: {err}"))?
            else {
                return Err("invalid display name or password".to_owned());
            };
            let Some(password_hash) = &account.password_hash else {
                return Err("account cannot use password login".to_owned());
            };
            if !verify_password(&password, password_hash) {
                return Err("invalid display name or password".to_owned());
            }

            authenticated_with_session(state, account)
                .await
                .map_err(|err| format!("session creation failed: {err}"))
        }
        ClientFrame::ResumeSession { session_token } => {
            let Some(session) = state
                .storage
                .validate_session(&hash_token(session_token.trim()))
                .await
                .map_err(|err| format!("session validation failed: {err}"))?
            else {
                return Err("session is invalid".to_owned());
            };

            Ok(AuthenticatedUser {
                user_id: session.user_id,
                display_name: session.display_name,
                is_admin: session.is_admin,
                session_token: None,
            })
        }
        _ => Err("first frame must authenticate".to_owned()),
    }
}

async fn authenticated_with_session(
    state: &AppState,
    account: light_discord_storage::Account,
) -> Result<AuthenticatedUser> {
    let session_token = new_session_token();
    let session = state
        .storage
        .create_session(&hash_token(&session_token), &account.user_id)
        .await?;
    Ok(AuthenticatedUser {
        user_id: session.user_id,
        display_name: session.display_name,
        is_admin: session.is_admin,
        session_token: Some(session_token),
    })
}

async fn write_server_frame(
    writer: &mut BufWriter<tokio::net::tcp::OwnedWriteHalf>,
    frame: &ServerFrame,
) -> Result<()> {
    let line = serde_json::to_string(frame)?;
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn bootstrap_auth(storage: &Storage) -> Result<()> {
    let Ok(password) = env::var("LD_BOOTSTRAP_ADMIN_PASSWORD") else {
        return Ok(());
    };
    if password.trim().is_empty() {
        return Ok(());
    }

    validate_password(&password).map_err(anyhow::Error::msg)?;
    let display_name = normalize_display_name(
        env::var("LD_BOOTSTRAP_ADMIN_NAME").unwrap_or_else(|_| "admin".to_owned()),
    );
    let password_hash = hash_password(&password)?;
    let admin = storage
        .ensure_bootstrap_admin(&display_name, &password_hash)
        .await?;

    if let Ok(invite_code) = env::var("LD_BOOTSTRAP_INVITE_CODE") {
        if !invite_code.trim().is_empty() {
            if let Err(err) = storage
                .create_invite(
                    &hash_token(invite_code.trim()),
                    Some(&admin.user_id),
                    "bootstrap invite",
                )
                .await
            {
                eprintln!("failed to create bootstrap invite: {err}");
            }
        }
    }

    println!("bootstrap admin ready: {display_name}");
    Ok(())
}

fn allow_dev_auth(storage: &Storage) -> bool {
    match env::var("LD_DEV_AUTH") {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => !storage.is_persistent(),
    }
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        Err("password must be at least 8 characters".to_owned())
    } else {
        Ok(())
    }
}

async fn run_voice_relay(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let (len, from) = socket.recv_from(&mut buf).await?;
        let packet = match decode_voice_packet_binary(&buf[..len]) {
            Ok(packet) => packet,
            Err(_) => continue,
        };

        state.remember_voice_addr(&packet.user_id, from).await;
        let targets = state.voice_targets(&packet.room_id, &packet.user_id).await;
        for target in targets {
            let _ = socket.send_to(&buf[..len], target).await;
        }
    }
}

async fn run_retention(storage: Storage) {
    let mut interval = time::interval(Duration::from_secs(60 * 60));
    loop {
        interval.tick().await;
        if let Err(err) = storage.purge_expired().await {
            eprintln!("retention purge failed: {err}");
        }
    }
}

fn normalize_display_name(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "guest".to_owned()
    } else {
        trimmed.chars().take(32).collect()
    }
}
