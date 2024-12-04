//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
// hide console window on Windows in release
#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> eframe::Result {
    use std::sync::{Arc, Mutex};

    use egui::FontId;
    use seance::{render_task, RenderRequest};

    env_logger::init();

    let render_request: Arc<Mutex<Option<RenderRequest>>> = Default::default();
    let render_thread_render_request = render_request.clone();
    let _render_thread = std::thread::spawn(|| render_task(render_thread_render_request));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([640.0, 480.0])
            .with_drag_and_drop(true),
        follow_system_theme: true,
        default_theme: eframe::Theme::Dark,
        renderer: eframe::Renderer::Wgpu,
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
                egui::FontData::from_static(include_bytes!(
                    "../fonts/departure-mono/DepartureMono-Regular.otf"
                )),
            );
            cc.egui_ctx.set_fonts(fonts);

            let font_id = FontId {
                size: 24.0,
                family: egui::FontFamily::Monospace,
            };
            let mut style = egui::Style::default();
            style
                .text_styles
                .insert(egui::TextStyle::Name("Movement Buttons".into()), font_id);
            cc.egui_ctx.set_style(style);

            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(seance::Seance::new(cc, render_request)))
        }),
    )
}
