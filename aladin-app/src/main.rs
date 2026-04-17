// aladin-app/src/main.rs

mod app;
mod worker;
mod ui;

use app::AladinApp;
use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Aladin Decryptor")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Aladin Decryptor",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(AladinApp::default()))
        }),
    )
}
