use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenShareSourceKind {
    Display,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareSource {
    pub id: String,
    pub kind: ScreenShareSourceKind,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCaptureFrame {
    pub width: u32,
    pub height: u32,
    pub image_format: String,
    pub data: Vec<u8>,
}

fn format_display_id(display_id: u32) -> String {
    format!("display:{display_id}")
}

fn format_window_id(window_id: u32) -> String {
    format!("window:{window_id}")
}

fn parse_source_id(id: &str) -> Result<(ScreenShareSourceKind, u32)> {
    let Some((kind, value)) = id.split_once(':') else {
        anyhow::bail!("invalid screen share source id: {id}");
    };
    if value.is_empty() {
        anyhow::bail!("invalid screen share source id: {id}");
    }

    let native_id = value
        .parse::<u32>()
        .with_context(|| format!("invalid screen share source id: {id}"))?;

    match kind {
        "display" => Ok((ScreenShareSourceKind::Display, native_id)),
        "window" => Ok((ScreenShareSourceKind::Window, native_id)),
        _ => anyhow::bail!("unknown screen share source kind: {kind}"),
    }
}

fn clamp_jpeg_quality(quality: u8) -> u8 {
    quality.clamp(1, 100)
}

fn scaled_dimensions(width: u32, height: u32, max_width: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || width <= max_width {
        return (width, height);
    }

    let scaled_height = ((height as u64 * max_width as u64) / width as u64)
        .max(1)
        .min(u32::MAX as u64) as u32;
    (max_width, scaled_height)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn available_screen_sources() -> Result<Vec<ScreenShareSource>> {
    let mut sources = Vec::new();

    for monitor in xcap::Monitor::all().context("failed to enumerate displays")? {
        let native_id = monitor.id().context("failed to read display id")?;
        let width = monitor.width().context("failed to read display width")?;
        let height = monitor.height().context("failed to read display height")?;
        if width == 0 || height == 0 {
            continue;
        }

        let mut title = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| format!("Display {native_id}"));
        if title.trim().is_empty() {
            title = format!("Display {native_id}");
        }
        if monitor.is_primary().unwrap_or(false) {
            title.push_str(" (primary)");
        }

        sources.push(ScreenShareSource {
            id: format_display_id(native_id),
            kind: ScreenShareSourceKind::Display,
            title,
            width,
            height,
        });
    }

    for window in xcap::Window::all().context("failed to enumerate windows")? {
        if window.is_minimized().unwrap_or(false) {
            continue;
        }

        let native_id = window.id().context("failed to read window id")?;
        let width = window.width().context("failed to read window width")?;
        let height = window.height().context("failed to read window height")?;
        if width == 0 || height == 0 {
            continue;
        }

        let mut title = window
            .title()
            .or_else(|_| window.app_name())
            .unwrap_or_else(|_| format!("Window {native_id}"));
        if title.trim().is_empty() {
            title = format!("Window {native_id}");
        }

        sources.push(ScreenShareSource {
            id: format_window_id(native_id),
            kind: ScreenShareSourceKind::Window,
            title,
            width,
            height,
        });
    }

    Ok(sources)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn available_screen_sources() -> Result<Vec<ScreenShareSource>> {
    Err(anyhow!("screen capture is not supported on this platform"))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn capture_screen_source_jpeg(
    source_id: &str,
    max_width: u32,
    quality: u8,
) -> Result<ScreenCaptureFrame> {
    let (kind, native_id) = parse_source_id(source_id)?;
    match kind {
        ScreenShareSourceKind::Display => {
            let monitor = xcap::Monitor::all()
                .context("failed to enumerate displays")?
                .into_iter()
                .find(|monitor| monitor.id().ok() == Some(native_id))
                .ok_or_else(|| anyhow!("display source not found: {source_id}"))?;
            encode_jpeg_frame(
                monitor
                    .capture_image()
                    .context("failed to capture display")?,
                max_width,
                quality,
            )
        }
        ScreenShareSourceKind::Window => {
            let window = xcap::Window::all()
                .context("failed to enumerate windows")?
                .into_iter()
                .find(|window| window.id().ok() == Some(native_id))
                .ok_or_else(|| anyhow!("window source not found: {source_id}"))?;
            if window.is_minimized().unwrap_or(false) {
                anyhow::bail!("window source is minimized: {source_id}");
            }
            encode_jpeg_frame(
                window.capture_image().context("failed to capture window")?,
                max_width,
                quality,
            )
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn capture_screen_source_jpeg(
    _source_id: &str,
    _max_width: u32,
    _quality: u8,
) -> Result<ScreenCaptureFrame> {
    Err(anyhow!("screen capture is not supported on this platform"))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn encode_jpeg_frame(
    image: image::RgbaImage,
    max_width: u32,
    quality: u8,
) -> Result<ScreenCaptureFrame> {
    let original_width = image.width();
    let original_height = image.height();
    let (width, height) = scaled_dimensions(original_width, original_height, max_width);
    let output = if (width, height) == (original_width, original_height) {
        image
    } else {
        image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
    };

    let mut data = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, clamp_jpeg_quality(quality));
    encoder
        .encode_image(&output)
        .context("failed to encode screen frame as JPEG")?;

    Ok(ScreenCaptureFrame {
        width,
        height,
        image_format: "jpeg".to_owned(),
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_round_trip() {
        assert_eq!(format_display_id(0), "display:0");
        assert_eq!(format_window_id(42), "window:42");
        assert_eq!(
            parse_source_id("display:7").unwrap(),
            (ScreenShareSourceKind::Display, 7)
        );
        assert_eq!(
            parse_source_id("window:9").unwrap(),
            (ScreenShareSourceKind::Window, 9)
        );
    }

    #[test]
    fn source_id_parser_rejects_invalid_values() {
        for id in [
            "",
            "display",
            "display:",
            "display:not-a-number",
            "screen:1",
            "window:-1",
            "window:1:extra",
        ] {
            assert!(parse_source_id(id).is_err(), "{id} should be invalid");
        }
    }

    #[test]
    fn jpeg_quality_is_clamped() {
        assert_eq!(clamp_jpeg_quality(0), 1);
        assert_eq!(clamp_jpeg_quality(1), 1);
        assert_eq!(clamp_jpeg_quality(70), 70);
        assert_eq!(clamp_jpeg_quality(100), 100);
        assert_eq!(clamp_jpeg_quality(255), 100);
    }

    #[test]
    fn dimensions_scale_down_by_width() {
        assert_eq!(scaled_dimensions(1920, 1080, 960), (960, 540));
        assert_eq!(scaled_dimensions(1600, 1200, 800), (800, 600));
        assert_eq!(scaled_dimensions(800, 600, 1200), (800, 600));
        assert_eq!(scaled_dimensions(800, 600, 0), (800, 600));
    }

    #[test]
    fn dimensions_keep_minimum_height_when_scaling() {
        assert_eq!(scaled_dimensions(10_000, 1, 100), (100, 1));
    }
}
