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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareMode {
    Text,
    Game,
}

impl Default for ScreenShareMode {
    fn default() -> Self {
        Self::Text
    }
}

impl ScreenShareMode {
    pub fn default_fps(self) -> u32 {
        match self {
            Self::Text => 5,
            Self::Game => 30,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScreenShareResolution {
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

impl Default for ScreenShareResolution {
    fn default() -> Self {
        Self::P720
    }
}

impl ScreenShareResolution {
    pub fn max_dimensions(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P720 => (1280, 720),
        }
    }
}

pub const SCREEN_SHARE_CODEC_AV1: &str = "av1";
pub const SCREEN_SHARE_CODEC_VP9: &str = "vp9";
pub const SCREEN_SHARE_CODEC_JPEG: &str = "jpeg";
pub const SCREEN_SHARE_TRANSPORT_SFU_RELAY: &str = "sfu_relay";

pub fn default_screen_share_target_fps() -> u32 {
    ScreenShareMode::default().default_fps()
}

pub fn default_screen_share_requested_codecs() -> Vec<String> {
    [
        SCREEN_SHARE_CODEC_AV1,
        SCREEN_SHARE_CODEC_VP9,
        SCREEN_SHARE_CODEC_JPEG,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub fn default_screen_share_active_codec() -> String {
    SCREEN_SHARE_CODEC_JPEG.to_owned()
}

pub fn default_screen_share_transport() -> String {
    SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned()
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
    StartScreenShare {
        source_name: String,
        width: u32,
        height: u32,
        #[serde(default)]
        mode: ScreenShareMode,
        #[serde(default)]
        resolution: ScreenShareResolution,
        #[serde(default = "default_screen_share_target_fps")]
        target_fps: u32,
        #[serde(default = "default_screen_share_requested_codecs")]
        requested_codecs: Vec<String>,
        #[serde(default = "default_screen_share_transport")]
        transport: String,
    },
    StopScreenShare,
    ScreenShareFrame {
        width: u32,
        height: u32,
        image_format: String,
        #[serde(default = "default_screen_share_active_codec")]
        codec: String,
        #[serde(default = "default_screen_share_transport")]
        transport: String,
        data_base64: String,
        sequence: u64,
        unix_ms: u64,
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
    ScreenShareStarted {
        user_id: UserId,
        display_name: String,
        source_name: String,
        width: u32,
        height: u32,
        #[serde(default)]
        mode: ScreenShareMode,
        #[serde(default)]
        resolution: ScreenShareResolution,
        #[serde(default = "default_screen_share_target_fps")]
        target_fps: u32,
        #[serde(default = "default_screen_share_requested_codecs")]
        requested_codecs: Vec<String>,
        #[serde(default = "default_screen_share_active_codec")]
        active_codec: String,
        #[serde(default = "default_screen_share_transport")]
        transport: String,
    },
    ScreenShareStopped {
        user_id: UserId,
    },
    ScreenShareFrame {
        user_id: UserId,
        display_name: String,
        width: u32,
        height: u32,
        image_format: String,
        #[serde(default)]
        mode: ScreenShareMode,
        #[serde(default)]
        resolution: ScreenShareResolution,
        #[serde(default = "default_screen_share_target_fps")]
        target_fps: u32,
        #[serde(default = "default_screen_share_active_codec")]
        codec: String,
        #[serde(default = "default_screen_share_transport")]
        transport: String,
        data_base64: String,
        sequence: u64,
        unix_ms: u64,
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

// -- Binary UDP voice-packet codec -------------------------------------------
//
// Wire layout (all numerics little-endian):
//   [0..4]   magic  "LDVP"
//   [4]      version  0x01
//   [5..7]   user_id length  (u16)
//   [7..]    user_id  (UTF-8, up to 65535 bytes)
//   [..]     room_id length  (u16)
//   [..]     room_id  (UTF-8, up to 65535 bytes)
//   [..]     sequence  (u64)
//   [..]     sample_rate  (u32)
//   [..]     channels  (u16)
//   [..]     codec  (u8: 0=pcm_s16le, 1=opus)
//   [..]     frame_samples  (u32)
//   [..]     payload length  (u32)
//   [..]     payload bytes
//
// Packets with trailing bytes are rejected.

const MAGIC: &[u8; 4] = b"LDVP";
const WIRE_VERSION: u8 = 0x01;
const CODEC_BYTE_PCM_S16LE: u8 = 0x00;
const CODEC_BYTE_OPUS: u8 = 0x01;

/// Errors returned by [`encode_voice_packet_binary`] and
/// [`decode_voice_packet_binary`].
#[derive(Debug, PartialEq, Eq)]
pub enum VoicePacketBinaryError {
    InvalidMagic,
    UnsupportedVersion(u8),
    Truncated,
    UnsupportedCodecByte(u8),
    UnsupportedCodecString(String),
    InvalidUtf8 { field: &'static str },
    FieldTooLong { field: &'static str },
    TrailingData,
}

impl std::fmt::Display for VoicePacketBinaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid magic bytes"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported protocol version: {v}"),
            Self::Truncated => write!(f, "packet is truncated"),
            Self::UnsupportedCodecByte(b) => write!(f, "unsupported codec byte: {b:#04x}"),
            Self::UnsupportedCodecString(s) => write!(f, "unsupported codec string: {s:?}"),
            Self::InvalidUtf8 { field } => write!(f, "invalid UTF-8 in field {field}"),
            Self::FieldTooLong { field } => {
                write!(f, "field {field} exceeds maximum encoded length")
            }
            Self::TrailingData => write!(f, "unexpected trailing bytes after packet"),
        }
    }
}

impl std::error::Error for VoicePacketBinaryError {}

/// Encode a [`VoicePacket`] to the compact binary wire format.
pub fn encode_voice_packet_binary(packet: &VoicePacket) -> Result<Vec<u8>, VoicePacketBinaryError> {
    let user_id_bytes = packet.user_id.as_bytes();
    let room_id_bytes = packet.room_id.as_bytes();

    if user_id_bytes.len() > u16::MAX as usize {
        return Err(VoicePacketBinaryError::FieldTooLong { field: "user_id" });
    }
    if room_id_bytes.len() > u16::MAX as usize {
        return Err(VoicePacketBinaryError::FieldTooLong { field: "room_id" });
    }
    if packet.payload.len() > u32::MAX as usize {
        return Err(VoicePacketBinaryError::FieldTooLong { field: "payload" });
    }

    let codec_byte = match packet.codec.as_str() {
        VOICE_CODEC_PCM_S16LE => CODEC_BYTE_PCM_S16LE,
        VOICE_CODEC_OPUS => CODEC_BYTE_OPUS,
        other => {
            return Err(VoicePacketBinaryError::UnsupportedCodecString(
                other.to_owned(),
            ))
        }
    };

    let capacity = 4
        + 1
        + 2
        + user_id_bytes.len()
        + 2
        + room_id_bytes.len()
        + 8
        + 4
        + 2
        + 1
        + 4
        + 4
        + packet.payload.len();

    let mut buf = Vec::with_capacity(capacity);

    buf.extend_from_slice(MAGIC);
    buf.push(WIRE_VERSION);

    buf.extend_from_slice(&(user_id_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(user_id_bytes);

    buf.extend_from_slice(&(room_id_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(room_id_bytes);

    buf.extend_from_slice(&packet.sequence.to_le_bytes());
    buf.extend_from_slice(&packet.sample_rate.to_le_bytes());
    buf.extend_from_slice(&packet.channels.to_le_bytes());
    buf.push(codec_byte);
    buf.extend_from_slice(&packet.frame_samples.to_le_bytes());

    buf.extend_from_slice(&(packet.payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&packet.payload);

    Ok(buf)
}

/// Decode a binary wire packet into a [`VoicePacket`].
pub fn decode_voice_packet_binary(bytes: &[u8]) -> Result<VoicePacket, VoicePacketBinaryError> {
    let mut pos = 0usize;

    macro_rules! need {
        ($n:expr) => {
            if bytes.len() < pos + $n {
                return Err(VoicePacketBinaryError::Truncated);
            }
        };
    }

    need!(4);
    if &bytes[pos..pos + 4] != MAGIC {
        return Err(VoicePacketBinaryError::InvalidMagic);
    }
    pos += 4;

    need!(1);
    let version = bytes[pos];
    pos += 1;
    if version != WIRE_VERSION {
        return Err(VoicePacketBinaryError::UnsupportedVersion(version));
    }

    // user_id
    need!(2);
    let user_id_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
    pos += 2;
    need!(user_id_len);
    let user_id = std::str::from_utf8(&bytes[pos..pos + user_id_len])
        .map_err(|_| VoicePacketBinaryError::InvalidUtf8 { field: "user_id" })?
        .to_owned();
    pos += user_id_len;

    // room_id
    need!(2);
    let room_id_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
    pos += 2;
    need!(room_id_len);
    let room_id = std::str::from_utf8(&bytes[pos..pos + room_id_len])
        .map_err(|_| VoicePacketBinaryError::InvalidUtf8 { field: "room_id" })?
        .to_owned();
    pos += room_id_len;

    // sequence (u64)
    need!(8);
    let sequence = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
    pos += 8;

    // sample_rate (u32)
    need!(4);
    let sample_rate = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
    pos += 4;

    // channels (u16)
    need!(2);
    let channels = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
    pos += 2;

    // codec (u8)
    need!(1);
    let codec_byte = bytes[pos];
    pos += 1;
    let codec = match codec_byte {
        CODEC_BYTE_PCM_S16LE => VOICE_CODEC_PCM_S16LE.to_owned(),
        CODEC_BYTE_OPUS => VOICE_CODEC_OPUS.to_owned(),
        other => return Err(VoicePacketBinaryError::UnsupportedCodecByte(other)),
    };

    // frame_samples (u32)
    need!(4);
    let frame_samples = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
    pos += 4;

    // payload
    need!(4);
    let payload_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    need!(payload_len);
    let payload = bytes[pos..pos + payload_len].to_vec();
    pos += payload_len;

    if pos != bytes.len() {
        return Err(VoicePacketBinaryError::TrailingData);
    }

    Ok(VoicePacket {
        user_id,
        room_id,
        sequence,
        sample_rate,
        channels,
        codec,
        frame_samples,
        payload,
    })
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

    // -- Binary codec tests --------------------------------------------------

    fn opus_packet() -> VoicePacket {
        VoicePacket {
            user_id: "user-1".to_owned(),
            room_id: "voice-general".to_owned(),
            sequence: 42,
            sample_rate: 48_000,
            channels: 2,
            codec: VOICE_CODEC_OPUS.to_owned(),
            frame_samples: 960,
            payload: vec![0xAA, 0xBB, 0xCC, 0xDD],
        }
    }

    fn pcm_packet() -> VoicePacket {
        VoicePacket {
            user_id: "alice".to_owned(),
            room_id: "room-42".to_owned(),
            sequence: 1,
            sample_rate: 44_100,
            channels: 1,
            codec: VOICE_CODEC_PCM_S16LE.to_owned(),
            frame_samples: 0,
            payload: vec![0x01, 0x02, 0x03],
        }
    }

    #[test]
    fn binary_codec_opus_roundtrip() {
        let pkt = opus_packet();
        let encoded = encode_voice_packet_binary(&pkt).unwrap();
        // Must not be JSON
        assert_ne!(encoded[0], b'{');
        let decoded = decode_voice_packet_binary(&encoded).unwrap();
        assert_eq!(decoded, pkt);
    }

    #[test]
    fn binary_codec_pcm_roundtrip() {
        let pkt = pcm_packet();
        let encoded = encode_voice_packet_binary(&pkt).unwrap();
        let decoded = decode_voice_packet_binary(&encoded).unwrap();
        assert_eq!(decoded, pkt);
    }

    #[test]
    fn binary_codec_heartbeat_roundtrip() {
        let pkt = VoicePacket {
            user_id: "u".to_owned(),
            room_id: "r".to_owned(),
            sequence: 0,
            sample_rate: 48_000,
            channels: 1,
            codec: VOICE_CODEC_OPUS.to_owned(),
            frame_samples: 0,
            payload: vec![],
        };
        let encoded = encode_voice_packet_binary(&pkt).unwrap();
        let decoded = decode_voice_packet_binary(&encoded).unwrap();
        assert_eq!(decoded, pkt);
    }

    #[test]
    fn binary_codec_rejects_invalid_magic() {
        let mut encoded = encode_voice_packet_binary(&opus_packet()).unwrap();
        encoded[0] = b'X';
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::InvalidMagic)
        );
    }

    #[test]
    fn binary_codec_rejects_unsupported_version() {
        let mut encoded = encode_voice_packet_binary(&opus_packet()).unwrap();
        encoded[4] = 0xFF;
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::UnsupportedVersion(0xFF))
        );
    }

    #[test]
    fn binary_codec_rejects_truncated_packet() {
        let encoded = encode_voice_packet_binary(&opus_packet()).unwrap();
        // Try several truncation points
        for len in [0, 1, 4, 5, 6, 10] {
            let result = decode_voice_packet_binary(&encoded[..len]);
            assert_eq!(result, Err(VoicePacketBinaryError::Truncated), "len={len}");
        }
    }

    #[test]
    fn binary_codec_rejects_unsupported_codec_byte() {
        let pkt = opus_packet();
        let mut encoded = encode_voice_packet_binary(&pkt).unwrap();
        // Find the codec byte: after magic(4)+version(1)+uid_len(2)+uid+rid_len(2)+rid+seq(8)+sr(4)+ch(2)
        let uid_len = pkt.user_id.len();
        let rid_len = pkt.room_id.len();
        let codec_pos = 4 + 1 + 2 + uid_len + 2 + rid_len + 8 + 4 + 2;
        encoded[codec_pos] = 0xFF;
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::UnsupportedCodecByte(0xFF))
        );
    }

    #[test]
    fn binary_codec_rejects_unsupported_codec_string_on_encode() {
        let mut pkt = opus_packet();
        pkt.codec = "flac".to_owned();
        assert_eq!(
            encode_voice_packet_binary(&pkt),
            Err(VoicePacketBinaryError::UnsupportedCodecString(
                "flac".to_owned()
            ))
        );
    }

    #[test]
    fn binary_codec_rejects_invalid_utf8_user_id() {
        let pkt = opus_packet();
        let mut encoded = encode_voice_packet_binary(&pkt).unwrap();
        // Overwrite first byte of user_id with invalid UTF-8
        let uid_start = 4 + 1 + 2;
        encoded[uid_start] = 0xFF;
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::InvalidUtf8 { field: "user_id" })
        );
    }

    #[test]
    fn binary_codec_rejects_invalid_utf8_room_id() {
        let pkt = opus_packet();
        let mut encoded = encode_voice_packet_binary(&pkt).unwrap();
        let uid_len = pkt.user_id.len();
        let rid_start = 4 + 1 + 2 + uid_len + 2;
        encoded[rid_start] = 0xFF;
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::InvalidUtf8 { field: "room_id" })
        );
    }

    #[test]
    fn binary_codec_rejects_trailing_data() {
        let pkt = opus_packet();
        let mut encoded = encode_voice_packet_binary(&pkt).unwrap();
        encoded.push(0x00);
        assert_eq!(
            decode_voice_packet_binary(&encoded),
            Err(VoicePacketBinaryError::TrailingData)
        );
    }

    #[test]
    fn client_frame_start_screen_share_round_trips() {
        let frame = ClientFrame::StartScreenShare {
            source_name: "My Display".to_owned(),
            width: 1920,
            height: 1080,
            mode: ScreenShareMode::Game,
            resolution: ScreenShareResolution::P1080,
            target_fps: 60,
            requested_codecs: default_screen_share_requested_codecs(),
            transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"start_screen_share\""));
        assert!(json.contains("\"mode\":\"game\""));
        assert!(json.contains("\"resolution\":\"1080p\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn client_frame_start_screen_share_defaults_legacy_metadata() {
        let json = br#"{"type":"start_screen_share","source_name":"My Display","width":1920,"height":1080}"#;
        let decoded = serde_json::from_slice::<ClientFrame>(json).unwrap();

        assert_eq!(
            decoded,
            ClientFrame::StartScreenShare {
                source_name: "My Display".to_owned(),
                width: 1920,
                height: 1080,
                mode: ScreenShareMode::Text,
                resolution: ScreenShareResolution::P720,
                target_fps: 5,
                requested_codecs: default_screen_share_requested_codecs(),
                transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
            }
        );
    }

    #[test]
    fn client_frame_stop_screen_share_round_trips() {
        let frame = ClientFrame::StopScreenShare;

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"stop_screen_share\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn client_frame_screen_share_frame_round_trips() {
        let frame = ClientFrame::ScreenShareFrame {
            width: 1920,
            height: 1080,
            image_format: "jpeg".to_owned(),
            codec: SCREEN_SHARE_CODEC_JPEG.to_owned(),
            transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
            data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_owned(),
            sequence: 42,
            unix_ms: 1672531200000,
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"screen_share_frame\""));

        let decoded = serde_json::from_str::<ClientFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn server_frame_screen_share_started_round_trips() {
        let frame = ServerFrame::ScreenShareStarted {
            user_id: "user-1".to_owned(),
            display_name: "alice".to_owned(),
            source_name: "My Display".to_owned(),
            width: 1920,
            height: 1080,
            mode: ScreenShareMode::Text,
            resolution: ScreenShareResolution::P1080,
            target_fps: 5,
            requested_codecs: default_screen_share_requested_codecs(),
            active_codec: SCREEN_SHARE_CODEC_JPEG.to_owned(),
            transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"screen_share_started\""));

        let decoded = serde_json::from_str::<ServerFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn server_frame_screen_share_stopped_round_trips() {
        let frame = ServerFrame::ScreenShareStopped {
            user_id: "user-1".to_owned(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"screen_share_stopped\""));

        let decoded = serde_json::from_str::<ServerFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn server_frame_screen_share_frame_round_trips() {
        let frame = ServerFrame::ScreenShareFrame {
            user_id: "user-1".to_owned(),
            display_name: "alice".to_owned(),
            width: 1920,
            height: 1080,
            image_format: "jpeg".to_owned(),
            mode: ScreenShareMode::Game,
            resolution: ScreenShareResolution::P1080,
            target_fps: 30,
            codec: SCREEN_SHARE_CODEC_JPEG.to_owned(),
            transport: SCREEN_SHARE_TRANSPORT_SFU_RELAY.to_owned(),
            data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_owned(),
            sequence: 42,
            unix_ms: 1672531200000,
        };

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"screen_share_frame\""));

        let decoded = serde_json::from_str::<ServerFrame>(&json).unwrap();
        assert_eq!(decoded, frame);
    }
}
