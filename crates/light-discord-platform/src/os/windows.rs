use super::PlatformInfo;

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: "windows",
        audio_stack_hint: "WASAPI via a future cpal backend",
        packaging_hint: "msi or portable zip",
        notification_hint: "Windows toast notifications via a Windows-specific adapter",
    }
}
