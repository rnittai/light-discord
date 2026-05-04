use super::PlatformInfo;

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: "linux",
        audio_stack_hint:
            "CPAL device enumeration with future capture/playback over PipeWire/PulseAudio/ALSA",
        packaging_hint: "deb, rpm, AppImage, or tarball",
        notification_hint: "freedesktop notifications via a Linux-specific adapter",
    }
}
