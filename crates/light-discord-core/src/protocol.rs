use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type UserId = String;
pub type ChannelId = String;
pub type RoomId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSummary {
    pub user_id: UserId,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceUser {
    pub user_id: UserId,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub display_name: String,
    pub body: String,
    pub unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditLogSummary {
    pub id: String,
    pub action: String,
    pub actor_user_id: String,
    pub target_user_id: Option<String>,
    pub target_message_id: Option<String>,
    pub channel_id: Option<String>,
    pub message_body_snapshot: Option<String>,
    pub unix_ms: u64,
}

impl ChatMessage {
    pub fn new(
        id: impl Into<String>,
        channel_id: impl Into<ChannelId>,
        user_id: impl Into<UserId>,
        display_name: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: channel_id.into(),
            user_id: user_id.into(),
            display_name: display_name.into(),
            body: body.into(),
            unix_ms: now_unix_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    Hello {
        display_name: String,
    },
    Register {
        invite_code: String,
        display_name: String,
        password: String,
    },
    Login {
        display_name: String,
        password: String,
    },
    ResumeSession {
        session_token: String,
    },
    JoinChannel {
        channel_id: ChannelId,
    },
    SendMessage {
        channel_id: ChannelId,
        body: String,
    },
    DeleteMessage {
        message_id: String,
    },
    AdminListAuditLog {
        limit: usize,
    },
    AdminCreateInvite {
        note: String,
    },
    JoinVoice {
        room_id: RoomId,
    },
    LeaveVoice,
    VoiceHeartbeat {
        room_id: RoomId,
    },
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Welcome {
        user_id: UserId,
        server_name: String,
        default_channel: ChannelId,
        udp_voice_addr: String,
        session_token: Option<String>,
        is_admin: bool,
    },
    InviteCreated {
        code: String,
    },
    AuditLog {
        entries: Vec<AuditLogSummary>,
    },
    ChannelJoined {
        channel_id: ChannelId,
    },
    UserList {
        users: Vec<UserSummary>,
    },
    Message(ChatMessage),
    MessageDeleted {
        message_id: String,
        channel_id: ChannelId,
        deleted_by: UserId,
        unix_ms: u64,
    },
    VoiceState {
        room_id: RoomId,
        users: Vec<VoiceUser>,
    },
    Error {
        message: String,
    },
}

/// Codec identifier carried in `VoicePacket::codec`.
pub const VOICE_CODEC_PCM_S16LE: &str = "pcm_s16le";
pub const VOICE_CODEC_OPUS: &str = "opus";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoicePacket {
    pub user_id: UserId,
    pub room_id: RoomId,
    pub sequence: u64,
    pub sample_rate: u32,
    pub channels: u16,
    /// Codec identifier. Defaults to `pcm_s16le` for backward compatibility
    /// with old senders that did not include this field.
    #[serde(default = "default_voice_codec")]
    pub codec: String,
    /// Samples per channel encoded in `payload`. Useful for Opus PLC where the
    /// receiver needs to know the frame size up front. Zero means "unspecified"
    /// (e.g. heartbeats or legacy raw PCM payloads).
    #[serde(default)]
    pub frame_samples: u32,
    pub payload: Vec<u8>,
}

fn default_voice_codec() -> String {
    VOICE_CODEC_PCM_S16LE.to_owned()
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_frame_round_trips_as_tagged_json() {
        let frame = ClientFrame::SendMessage {
            channel_id: "general".to_owned(),
            body: "hello".to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"send_message\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn server_frame_round_trips_message_payload() {
        let message = ChatMessage::new("msg-1", "general", "user-1", "alice", "hello");
        let frame = ServerFrame::Message(message.clone());

        let json = serde_json::to_string(&frame).unwrap();
        let decoded = serde_json::from_str::<ServerFrame>(&json).unwrap();

        assert_eq!(decoded, ServerFrame::Message(message));
    }

    #[test]
    fn voice_packet_round_trips_binary_payload() {
        let packet = VoicePacket {
            user_id: "user-1".to_owned(),
            room_id: "voice-general".to_owned(),
            sequence: 7,
            sample_rate: 48_000,
            channels: 1,
            codec: VOICE_CODEC_OPUS.to_owned(),
            frame_samples: 960,
            payload: vec![1, 2, 3, 4],
        };

        let json = serde_json::to_vec(&packet).unwrap();
        let decoded = serde_json::from_slice::<VoicePacket>(&json).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn voice_packet_legacy_payload_defaults_to_pcm_codec() {
        // Old clients did not emit `codec` or `frame_samples`. The new fields
        // must be serde-defaulted so the relay/server keeps parsing them.
        let json = br#"{"user_id":"u","room_id":"r","sequence":1,"sample_rate":48000,"channels":1,"payload":[]}"#;
        let decoded = serde_json::from_slice::<VoicePacket>(json).unwrap();
        assert_eq!(decoded.codec, VOICE_CODEC_PCM_S16LE);
        assert_eq!(decoded.frame_samples, 0);
    }

    #[test]
    fn delete_message_frame_round_trips() {
        let frame = ClientFrame::DeleteMessage {
            message_id: "msg-1".to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"delete_message\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn login_frame_round_trips() {
        let frame = ClientFrame::Login {
            display_name: "alice".to_owned(),
            password: "secret".to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"login\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }
}
