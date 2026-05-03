use super::PlatformInfo;

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: "unsupported",
        audio_stack_hint: "not implemented",
        packaging_hint: "not implemented",
        notification_hint: "not implemented",
    }
}
