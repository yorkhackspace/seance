//! `app`
//!
//! Build script for seance app.

fn main() {
    println!("cargo::rerun-if-changed=../logo.svg");

    let logo_svg_bytes = include_bytes!("../logo.svg");
    let tree = resvg::usvg::Tree::from_data(logo_svg_bytes, &resvg::usvg::Options::default())
        .expect("Failed to load logo SVG");

    let mut pixmap = resvg::tiny_skia::Pixmap::new(256, 256).expect("Failed to create pixmap");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default()
            .pre_scale(256.0 / tree.size().width(), 256.0 / tree.size().height()),
        &mut pixmap.as_mut(),
    );

    let buffer = image::RgbaImage::from_vec(256, 256, pixmap.data().to_vec())
        .expect("Failed to create image buffer");
    buffer
        .save_with_format("../logo.png", image::ImageFormat::Png)
        .expect("Failed to save logo");
}
