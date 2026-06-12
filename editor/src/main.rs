// RtV Save Editor
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod app;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([1080.0, 640.0])
            .with_title("RtV Save Editor"),
        ..Default::default()
    };
    eframe::run_native(
        "RtV Save Editor",
        native_options,
        Box::new(|cc| Ok(Box::new(app::EditorApp::new(cc)))),
    )
}
