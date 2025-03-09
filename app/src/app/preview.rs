//! `preview`
//!
//! Generates previews of design files.

use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use egui::{ColorImage, ImageData, TextureHandle, TextureOptions};
use oneshot::TryRecvError;

use seance::{
    resolve_paths, svg::get_paths_grouped_by_colour, DesignFile, BED_HEIGHT_MM, BED_WIDTH_MM,
};

use super::DesignWithMeta;

/// The maximum that we can zoom into the design preview.
pub const MAX_ZOOM_LEVEL: f32 = 5.0;
/// The minimum zoom level for the design preview.
pub const MIN_ZOOM_LEVEL: f32 = 1.0;

/// The background colour for the design preview.
const PREVIEW_BACKGROUND_COLOUR: [u8; 4] = [230, 230, 230, 255];
/// How thick to draw lines for the design preview, in pixels.
const PREVIEW_LINE_THICKNESS_PIXELS: usize = 4;

/// The cache for the design preview.
pub struct DesignPreview {
    /// The size of the preview.
    size: egui::Vec2,
    /// The current zoom level.
    zoom: f32,
    /// How much the design is offset (in mm) from top-left corner.
    design_offset_mm: egui::Vec2,
    /// The texture handle created from the texture buffer, this is what egui uses to draw the preview in the UI.
    image_texture: Option<TextureHandle>,
    /// Where to put requests to re-render.
    render_request: Arc<Mutex<Option<RenderRequest>>>,
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
    /// * `render_request_tx`: Where to put requests to re-render.
    ///
    /// # Returns
    /// A new [`DesignPreview`].
    pub fn new(
        size: egui::Vec2,
        mut zoom: f32,
        design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
        render_request: Arc<Mutex<Option<RenderRequest>>>,
    ) -> Self {
        zoom = zoom.min(MAX_ZOOM_LEVEL).max(MIN_ZOOM_LEVEL);
        let image_texture = None;

        let (callback_tx, callback_rx) = oneshot::channel();
        {
            let mut render_request_lock = render_request
                .lock()
                .expect("Render requests mutex must be lockable");
            *render_request_lock = Some(RenderRequest {
                size: size.clone(),
                design_offset_mm: Default::default(),
                design_file: design_file.clone(),
                callback: callback_tx,
            });
        }

        Self {
            size,
            zoom,
            design_offset_mm: Default::default(),
            image_texture,
            render_request,
            waiting_render_callback: Some(callback_rx),
        }
    }

    /// Resizes the deisgn preview.
    ///
    /// # Arguments
    /// * `size`: The new size of the preview.
    /// * `design_file`: The design file being drawn.
    pub fn resize(&mut self, size: egui::Vec2, design_file: &Arc<RwLock<Option<DesignWithMeta>>>) {
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

    /// Gets the current offset of the design from the top-left corner, in mm.
    ///
    /// # Returns
    /// Offset in mm.
    pub fn get_design_offset(&self) -> &egui::Vec2 {
        &self.design_offset_mm
    }

    /// Sets the offset of the design from the top-left corner, in mm.
    ///
    /// # Arguments
    /// * `offset_mm`: The offset to set.
    /// * `design_file`: The design file to be offset.
    pub fn set_design_offset(
        &mut self,
        mut offset_mm: egui::Vec2,
        design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    ) {
        offset_mm.x = offset_mm.x.max(0.0);
        offset_mm.y = offset_mm.y.max(0.0);
        if offset_mm != self.design_offset_mm {
            self.design_offset_mm = offset_mm;
            self.render(design_file);
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
        design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
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
                    {
                        let mut render_request_lock = self
                            .render_request
                            .lock()
                            .expect("Render requests mutex must be lockable");
                        *render_request_lock = Some(RenderRequest {
                            size: self.size,
                            design_offset_mm: self.design_offset_mm,
                            design_file: design_file.clone(),
                            callback: callback_tx,
                        });
                    }
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

        // If we ever actually zoom to 1x then the scrollbars disappear from the UI.
        // When we then zoom in, the bars flash back into existence in a very nasty
        // way. Therefore, we never allow the zoom level to actually return to 1.0
        let zoomed_bounding_box_width = self.size.x * (self.zoom * 1.05);
        let zoomed_bounding_box_height = self.size.y * (self.zoom * 1.05);

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
    pub fn render(&mut self, design_file: &Arc<RwLock<Option<DesignWithMeta>>>) {
        let (callback_tx, callback_rx) = oneshot::channel();
        {
            let mut render_request_lock = self
                .render_request
                .lock()
                .expect("Render requests mutex must be lockable");
            *render_request_lock = Some(RenderRequest {
                size: self.size,
                design_offset_mm: self.design_offset_mm,
                design_file: design_file.clone(),
                callback: callback_tx,
            });
        }
        self.waiting_render_callback = Some(callback_rx);
    }
}

/// The result of rendering the design preview.
pub struct RenderedImage {
    /// The resulting image.
    image: ColorImage,
}

/// Request that a design preview be rendered for the given design file.
pub struct RenderRequest {
    /// The size of the preview to render.
    size: egui::Vec2,
    /// Offset of the design from the top-left corner, in mm.
    design_offset_mm: egui::Vec2,
    /// The design file to render.
    design_file: Arc<RwLock<Option<DesignWithMeta>>>,
    /// Callback to send the rendered preview into.
    callback: RenderRequestCallback,
}

/// Callbacks for rendered design previews.
pub type RenderRequestCallback = oneshot::Sender<RenderedImage>;

/// Long-running task to render design previews in the background.
///
/// # Arguments
/// * `render_request`: Location where a render request can be read from. The request will be taken and replaced with `None`.
pub fn render_task(render_request: Arc<Mutex<Option<RenderRequest>>>) {
    let mut texture_buffer: Vec<u8> = vec![];
    let mut previous_design_hash: Option<u64> = None;
    let mut previous_size: Option<egui::Vec2> = None;

    loop {
        let request = {
            let Ok(mut request_lock) = render_request.lock() else {
                log::debug!("Render mutex dropped, render thread returning");
                return;
            };

            request_lock.take()
        };

        if let Some(RenderRequest {
            size,
            design_offset_mm,
            design_file,
            callback,
        }) = request
        {
            render_inner(
                size,
                &mut previous_size,
                &design_offset_mm,
                &design_file,
                &mut texture_buffer,
                &mut previous_design_hash,
                callback,
            );
        }

        // TODO: Nasty.
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Does the actual rendering of the design preview.
///
/// # Arguments
/// * `size`: The size to draw the preview at.
/// * `previous_size`: The previous size we drew the preview at, we will re-draw if the size has changed.
/// * `offset_mm`: The offset of the design from the top-left corner, in mm.
/// * `design_file`: The design file to render.
/// * `texture_buffer`: This is the texture that is actually shown to the user.
/// * `previous_design_hash`: The previous hash of the design file.
/// * `callback`: Callback into which the rendered image will be sent.
fn render_inner(
    size: egui::Vec2,
    previous_size: &mut Option<egui::Vec2>,
    offset_mm: &egui::Vec2,
    design_file: &Arc<RwLock<Option<DesignWithMeta>>>,
    texture_buffer: &mut Vec<u8>,
    previous_design_hash: &mut Option<u64>,
    callback: RenderRequestCallback,
) {
    // Calculate how big the texture should be.
    let zoomed_bounding_box_width = size.x * MAX_ZOOM_LEVEL;
    let zoomed_bounding_box_height = size.y * MAX_ZOOM_LEVEL;
    let texture_width = zoomed_bounding_box_width.floor() as u32;
    let texture_height = zoomed_bounding_box_height.floor() as u32;

    // Work out how many pixels correspond to 1mm in each dimension.
    let pixels_per_mm_x = zoomed_bounding_box_width / BED_WIDTH_MM;
    let pixels_per_mm_y = zoomed_bounding_box_height / BED_HEIGHT_MM;

    // We want to place a marker every 10mm to give the user a point of reference, so we need to work out how many pixels correspond to 10mm.
    let pixels_per_10_mm_x = pixels_per_mm_x * 10.0;
    let pixels_per_10_mm_y = pixels_per_mm_y * 10.0;

    let Ok(design_lock) = design_file.read() else {
        log::error!("Failed to lock design file for render");
        return;
    };
    let design = &*design_lock;

    if Some(size) == *previous_size
        && design.as_ref().map(|(_, hash, _)| hash) == previous_design_hash.as_ref()
    {
        // Nothing has changed, nothing to do.
        return;
    }

    // Resize texture buffer to fill the bounds.
    *previous_size = Some(size);
    resize_texture_buffer(
        texture_buffer,
        texture_width as usize,
        texture_height as usize,
    );

    for (index, pixel) in texture_buffer.chunks_exact_mut(4).enumerate() {
        // Get the x/y position of the pixel.
        let x = index % texture_width as usize;
        let y = index / texture_width as usize;

        // Work out where along the bed we are, in 10mm increments.
        let bed_width_fraction = (x as f32) / pixels_per_10_mm_x;
        let bed_height_fraction = (y as f32) / pixels_per_10_mm_y;

        // We want just the fractional component so that...
        let proportion_x = bed_height_fraction.fract();
        let proportion_y = bed_width_fraction.fract();

        // Anything that is -0.9 to +0.1 away from the nearest 10mm gets coloured in a different colour, so that the user sees markers for each 10mm increment.
        if (proportion_x <= 0.1 || proportion_x >= 0.9)
            && (proportion_y <= 0.1 || proportion_y >= 0.9)
        {
            pixel.copy_from_slice(&[100, 100, 100, 255]);
        } else {
            pixel.copy_from_slice(&PREVIEW_BACKGROUND_COLOUR);
        }
    }

    // If we have a design file then we need to check if the hash has changed, if so then we need to re-render the design.
    if let Some((DesignFile { tree, .. }, hash, _)) = &design {
        *previous_design_hash = Some(*hash);

        let grouped_paths = get_paths_grouped_by_colour(tree).unwrap();
        let resolved_paths = resolve_paths(&grouped_paths, (offset_mm.x, offset_mm.y), 0.1);

        for (path_colour, paths) in resolved_paths {
            for path in paths {
                for point in path {
                    let pixel_x = (point.x * pixels_per_mm_x).ceil() as usize;
                    let pixel_y = (point.y * pixels_per_mm_x).ceil() as usize;

                    // Draw either side of the line
                    for x in (pixel_x - (PREVIEW_LINE_THICKNESS_PIXELS / 2))
                        ..(pixel_x + (PREVIEW_LINE_THICKNESS_PIXELS / 2))
                    {
                        for y in (pixel_y - (PREVIEW_LINE_THICKNESS_PIXELS / 2))
                            ..(pixel_y + (PREVIEW_LINE_THICKNESS_PIXELS / 2))
                        {
                            if pixel_x == x || pixel_y == y {
                                if let Some(pixel) = texture_buffer
                                    .chunks_mut(4)
                                    .nth((y * texture_width as usize) + x)
                                {
                                    pixel.copy_from_slice(&[
                                        path_colour.0[0],
                                        path_colour.0[1],
                                        path_colour.0[2],
                                        255,
                                    ]);
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        invalidate_design_texture(previous_design_hash);
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
fn invalidate_design_texture(design_hash: &mut Option<u64>) {
    *design_hash = None;
}
