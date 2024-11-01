//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
// hide console window on Windows in release
#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> eframe::Result {
    use seance::render_task;

    env_logger::init();

    let (render_thread_tx, render_thread_rx) = std::sync::mpsc::channel();
    let _render_thread = std::thread::spawn(|| render_task(render_thread_rx));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([640.0, 480.0])
            .with_drag_and_drop(true),
        follow_system_theme: true,
        default_theme: eframe::Theme::Dark,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "seance",
        native_options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(seance::Seance::new(cc, render_thread_tx)))
        }),
    )
}
