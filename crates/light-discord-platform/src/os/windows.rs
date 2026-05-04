use super::PlatformInfo;

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: "windows",
        audio_stack_hint: "CPAL device enumeration with future capture/playback over WASAPI",
        packaging_hint: "msi or portable zip",
        notification_hint: "Windows toast notifications via a Windows-specific adapter",
    }
}
