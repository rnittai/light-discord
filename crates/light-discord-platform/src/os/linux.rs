use super::PlatformInfo;

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: "linux",
        audio_stack_hint: "PipeWire/PulseAudio/ALSA via a future cpal backend",
        packaging_hint: "deb, rpm, AppImage, or tarball",
        notification_hint: "freedesktop notifications via a Linux-specific adapter",
    }
}
