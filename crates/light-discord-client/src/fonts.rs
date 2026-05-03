use eframe::egui::{self, FontFamily};
use std::{env, fs, path::PathBuf};

const FONT_NAME: &str = "light_discord_japanese";

pub fn configure_japanese_fonts(ctx: &egui::Context) -> Option<PathBuf> {
    let path = find_japanese_font()?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to read Japanese font {}: {err}", path.display());
            return None;
        }
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert(FONT_NAME.to_owned(), egui::FontData::from_owned(bytes));

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, FONT_NAME.to_owned());
    }

    ctx.set_fonts(fonts);
    Some(path)
}

fn find_japanese_font() -> Option<PathBuf> {
    if let Ok(path) = env::var("LIGHT_DISCORD_FONT_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    candidate_font_paths()
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn candidate_font_paths() -> Vec<&'static str> {
    vec![
        // Linux: Noto CJK, IPA, Takao, and common distro aliases.
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansJP-Regular.ttf",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
        "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
        // Windows: Meiryo, Yu Gothic, and MS Gothic.
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
    ]
}
