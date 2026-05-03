#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub audio_stack_hint: &'static str,
    pub packaging_hint: &'static str,
    pub notification_hint: &'static str,
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::platform_info;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub use unsupported::platform_info;
#[cfg(target_os = "windows")]
pub use windows::platform_info;
