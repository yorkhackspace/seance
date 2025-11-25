//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

// Hide console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::Seance;
use app::{render_task, RenderRequest};

fn main() -> eframe::Result {
    use std::sync::{Arc, Mutex};

    env_logger::init();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../logo.png"))
        .expect("The icon data must be valid");

    let render_request: Arc<Mutex<Option<RenderRequest>>> = Arc::default();
    let render_thread_render_request = render_request.clone();
    let _render_thread = std::thread::spawn(|| render_task(render_thread_render_request));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([640.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        renderer: eframe::Renderer::Glow,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "seance",
        native_options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "Departure Mono".to_owned(),
                Arc::new(egui::FontData::from_static(include_bytes!(
                    "../fonts/departure-mono/DepartureMono-Regular.otf"
                ))),
            );
            cc.egui_ctx.set_fonts(fonts);

            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Seance::new(cc, render_request)))
        }),
    )
}
