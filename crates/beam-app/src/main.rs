//! beaM. application entry point.

mod app;

use app::BeamApp;
use beam_core::APP_NAME;
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting {APP_NAME}");

    // Check if an initial file was passed as argument (e.g. `beam photo.ARW`)
    let initial_file = std::env::args().nth(1).map(PathBuf::from);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        native_options,
        Box::new(move |cc| {
            let mut app = BeamApp::new(cc);
            if let Some(path) = initial_file {
                app.load_file_path(path);
            }
            Ok(Box::new(app))
        }),
    )
}
