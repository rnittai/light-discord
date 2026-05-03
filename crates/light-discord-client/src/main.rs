mod app;
mod fonts;
mod net;
mod voice;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1040.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Light Discord",
        options,
        Box::new(|cc| Box::new(app::LightDiscordApp::new(cc))),
    )
}
