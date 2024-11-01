//! `preview`
//!
//! Generates previews of design files.

use std::sync::{Arc, RwLock};

use egui::{ColorImage, ImageData, TextureHandle, TextureOptions};
use oneshot::TryRecvError;
use resvg::{tiny_skia::Color, usvg};

use crate::{DesignFile, BED_HEIGHT_MM, BED_WIDTH_MM};

/// The maximum that we can zoom into the design preview.
pub const MAX_ZOOM_LEVEL: f32 = 5.0;
/// The minimum zoom level for the design preview.
pub const MIN_ZOOM_LEVEL: f32 = 1.05;

/// The background colour for the design preview.
const PREVIEW_BACKGROUND_COLOUR: [u8; 4] = [230, 230, 230, 255];

pub type RenderThreadTx = std::sync::mpsc::Sender<RenderThreadMessage>;
pub type RenderThreadRx = std::sync::mpsc::Receiver<RenderThreadMessage>;

/// The cache for the design preview.
pub struct DesignPreview {
    /// The size of the preview.
    size: egui::Vec2,
    /// The current zoom level.
    zoom: f32,
    /// The texture handle created from the texture buffer, this is what egui uses to draw the preview in the UI.
    image_texture: Option<TextureHandle>,
    /// Channel into which requests to re-render can be sent.
    render_request_tx: std::sync::mpsc::Sender<RenderThreadMessage>,
    /// The callback for the latest render request. Callbacks for old requests will be dropped.
    waiting_render_callback: Option<oneshot::Receiver<RenderedImage>>,
}

impl DesignPreview {
    /// Creates a new [`DesignPreview`].
    ///
    /// # Arguments
    /// * `size`: The size to draw the preview at.
    /// * `zoom`: The current zoom level.
    /// * `design_file`: The design file to draw the preview for.
    /// * `render_request_tx`: Channel into which requests to re-render can be sent.
    ///
    /// # Returns
    /// A new [`DesignPreview`].
    pub fn new(
        size: egui::Vec2,
        mut zoom: f32,
        design_file: &Arc<RwLock<Option<DesignFile>>>,
        render_request_tx: RenderThreadTx,
    ) -> Self {
        zoom = zoom.min(MAX_ZOOM_LEVEL).max(MIN_ZOOM_LEVEL);
        let image_texture = None;

        let (callback_tx, callback_rx) = oneshot::channel();
        let _ = render_request_tx.send(RenderThreadMessage::RequestRender {
            size: size.clone(),
            design_file: design_file.clone(),
            callback: callback_tx,
        });

        Self {
            size,
            zoom,
            image_texture,
            render_request_tx,
            waiting_render_callback: Some(callback_rx),
        }
    }

    /// Resizes the deisgn preview.
    ///
    /// # Arguments
    /// * `size`: The new size of the preview.
    /// * `design_file`: The design file being drawn.
    pub fn resize(&mut self, size: egui::Vec2, design_file: &Arc<RwLock<Option<DesignFile>>>) {
        if size != self.size {
            self.size = size;
            self.render(design_file);
        }
    }

    /// Sets the zoom level of the design preview.
    ///
    /// # Arguments
    /// * `zoom`: The new zoom level.
    pub fn zoom(&mut self, mut zoom: f32) {
        zoom = zoom.min(MAX_ZOOM_LEVEL).max(MIN_ZOOM_LEVEL);
        if zoom != self.zoom {
            self.zoom = zoom;
        }
    }

    /// Gets the image to be drawn to the UI.
    ///
    /// # Arguments
    /// * `ctx`: egui context that can be used to allocate resources if needed.
    /// * `design_file`: The file to render if we need to request a re-render.
    ///
    /// # Returns
    /// The image to draw to the UI as the design preview, if any is available.
    pub fn image(
        &mut self,
        ctx: &egui::Context,
        design_file: &Arc<RwLock<Option<DesignFile>>>,
    ) -> Option<egui::Image<'_>> {
        let mut waiting_render_callback = self.waiting_render_callback.take();
        if let Some(waiting) = waiting_render_callback {
            match waiting.try_recv() {
                Ok(img) => {
                    let texture = ctx.load_texture(
                        "design",
                        ImageData::Color(img.image.into()),
                        TextureOptions::default(),
                    );
                    self.image_texture = Some(texture);
                    waiting_render_callback = None;
                }
                Err(TryRecvError::Disconnected) => {
                    let (callback_tx, callback_rx) = oneshot::channel();
                    let _ = self
                        .render_request_tx
                        .send(RenderThreadMessage::RequestRender {
                            size: self.size,
                            design_file: design_file.clone(),
                            callback: callback_tx,
                        });
                    waiting_render_callback = Some(callback_rx);
                }
                Err(TryRecvError::Empty) => {
                    waiting_render_callback = Some(waiting);
                }
            }
        }
        self.waiting_render_callback = waiting_render_callback;

        let Some(texture) = &self.image_texture else {
            return None;
        };

        let zoomed_bounding_box_width = self.size.x * self.zoom;
        let zoomed_bounding_box_height = self.size.y * self.zoom;

        let texture_width = zoomed_bounding_box_width.floor();
        let texture_height = zoomed_bounding_box_height.floor();

        let image = egui::Image::from_texture(texture)
            .max_width(texture_width)
            .max_height(texture_height);
        Some(image)
    }

    /// Request that the design preview be rendered.
    ///
    /// # Arguments
    /// * `design_file`: The design to render.
    pub fn render(&mut self, design_file: &Arc<RwLock<Option<DesignFile>>>) {
        let (callback_tx, callback_rx) = oneshot::channel();
        let _ = self
            .render_request_tx
            .send(RenderThreadMessage::RequestRender {
                size: self.size,
                design_file: design_file.clone(),
                callback: callback_tx,
            });
        self.waiting_render_callback = Some(callback_rx);
    }
}

/// The result of rendering the design preview.
pub struct RenderedImage {
    /// The resulting image.
    image: ColorImage,
}

/// Messages that can be sent to the render thread.
pub enum RenderThreadMessage {
    /// Request that a design preview be rendered for the given design file.
    RequestRender {
        /// The size of the preview to render.
        size: egui::Vec2,
        /// The design file to render.
        design_file: Arc<RwLock<Option<DesignFile>>>,
        /// Callback to send the rendered preview into.
        callback: RenderRequestCallback,
    },
}

/// Callbacks for rendered design previews.
pub type RenderRequestCallback = oneshot::Sender<RenderedImage>;

/// Long-running task to render design previews in the background.
///
/// # Arguments
/// * `render_request_rx`: Channel that messages sent to this task will be received on.
pub fn render_task(render_request_rx: RenderThreadRx) {
    let mut texture_buffer: Vec<u8> = vec![];
    let mut previous_design_hash: Option<u64> = None;
    let mut design_texture: Option<resvg::tiny_skia::Pixmap> = None;

    while let Ok(msg) = render_request_rx.recv() {
        match msg {
            RenderThreadMessage::RequestRender {
                size,
                design_file,
                callback,
            } => {
                render_inner(
                    size,
                    &design_file,
                    &mut texture_buffer,
                    &mut previous_design_hash,
                    &mut design_texture,
                    callback,
                );
            }
        }
    }
}

/// Does the actual rendering of the design preview.
///
/// # Arguments
/// * `size`: The size to draw the preview at.
/// * `design_file`: The design file to render.
/// * `texture_buffer`: This is the texture that is actually shown to the user.
/// * `previous_design_hash`: The previous hash of the design file.
/// * `design_texture`: The texture to render an SVG design into.
/// * `callback`: Callback into which the rendered image will be sent.
fn render_inner(
    size: egui::Vec2,
    design_file: &Arc<RwLock<Option<DesignFile>>>,
    texture_buffer: &mut Vec<u8>,
    previous_design_hash: &mut Option<u64>,
    design_texture: &mut Option<resvg::tiny_skia::Pixmap>,
    callback: RenderRequestCallback,
) {
    let zoomed_bounding_box_width = size.x * MAX_ZOOM_LEVEL;
    let zoomed_bounding_box_height = size.y * MAX_ZOOM_LEVEL;

    let texture_width = zoomed_bounding_box_width.floor() as u32;
    let texture_height = zoomed_bounding_box_height.floor() as u32;

    resize_texture_buffer(
        texture_buffer,
        texture_width as usize,
        texture_height as usize,
    );

    let Ok(design_lock) = design_file.read() else {
        log::error!("Failed to lock design file for render");
        return;
    };

    let design = &*design_lock;

    if let Some(DesignFile {
        name: _,
        path: _,
        hash,
        tree,
        width_mm,
        height_mm,
    }) = &design
    {
        if Some(*hash) != *previous_design_hash {
            *previous_design_hash = Some(*hash);
            let width = (width_mm / BED_WIDTH_MM) * size.x * MAX_ZOOM_LEVEL;
            let height = (height_mm / BED_HEIGHT_MM) * size.y * MAX_ZOOM_LEVEL;

            let Some(mut pixmap) =
                resvg::tiny_skia::Pixmap::new((width).ceil() as u32, (height).ceil() as u32)
            else {
                log::error!("Could not create pixmap for rendering design preview");
                invalidate_design_texture(previous_design_hash, design_texture);
                return;
            };

            pixmap.fill(Color::from_rgba8(
                PREVIEW_BACKGROUND_COLOUR[0],
                PREVIEW_BACKGROUND_COLOUR[1],
                PREVIEW_BACKGROUND_COLOUR[2],
                PREVIEW_BACKGROUND_COLOUR[3],
            ));
            let transform = usvg::Transform::default();
            resvg::render(&tree, transform, &mut pixmap.as_mut());
            *design_texture = Some(pixmap);
        }
    } else {
        invalidate_design_texture(previous_design_hash, design_texture);
    }

    let pixels_per_mm_x = zoomed_bounding_box_width / BED_WIDTH_MM;
    let pixels_per_mm_y = zoomed_bounding_box_height / BED_HEIGHT_MM;

    let pixels_per_10_mm_x = pixels_per_mm_x * 10.0;
    let pixels_per_10_mm_y = pixels_per_mm_y * 10.0;

    for (index, pixel) in texture_buffer.chunks_exact_mut(4).enumerate() {
        let x = index % texture_width as usize;
        let y = index / texture_width as usize;
        let mut written = false;
        if let Some(design) = design_texture {
            let width = design.width().min(texture_width) as usize;
            let height = design.height().min(texture_height) as usize;
            if x < width && y < height {
                let design_texture_start = ((y * width) + x) * 4;
                pixel.copy_from_slice(
                    &design.data()[design_texture_start..design_texture_start + 4],
                );
                written = true;
            }
        }

        if !written {
            let bed_width_fraction = (x as f32) / pixels_per_10_mm_x;
            let bed_height_fraction = (y as f32) / pixels_per_10_mm_y;

            let proportion_x = bed_height_fraction.fract();
            let proportion_y = bed_width_fraction.fract();

            if (proportion_x <= 0.1 || proportion_x >= 0.9)
                && (proportion_y <= 0.1 || proportion_y >= 0.9)
            {
                pixel.copy_from_slice(&[100, 100, 100, 255]);
            } else {
                pixel.copy_from_slice(&PREVIEW_BACKGROUND_COLOUR);
            }
        }
    }

    let ci = ColorImage::from_rgba_unmultiplied(
        [texture_width as usize, texture_height as usize],
        &texture_buffer[0..(texture_width as usize * texture_height as usize * 4)],
    );
    let _ = callback.send(RenderedImage { image: ci });
}

/// Resizes the texture buffer to a new width and height.
/// Will only allocate new memory if the total memory required is larger that the
/// current amount of memory that has been allocated.
///
/// # Arguments
/// * `buffer`: The buffer to be resized.
/// * `width`: The new width, in pixels.
/// * `height`: The new height, in pixels.
fn resize_texture_buffer(buffer: &mut Vec<u8>, width: usize, height: usize) {
    let total_size = width * height * 4; // rgba

    // We only ever want to increase the size of the buffer so that we have the maximum size the user ever uses available to us.
    if buffer.len() < total_size {
        buffer.resize(total_size, 0);
    }
}

/// Yeets the cached values for the design preview.
///
/// # Arguments
/// * `design_hash`: The hash of the design.
/// * `design_texture`: The pixmap used to render SVGs.
fn invalidate_design_texture(
    design_hash: &mut Option<u64>,
    design_texture: &mut Option<resvg::tiny_skia::Pixmap>,
) {
    *design_hash = None;
    *design_texture = None;
}
