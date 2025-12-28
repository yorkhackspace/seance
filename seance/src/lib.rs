//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

pub mod default_passes;
mod hpgl;
mod laser_passes;
mod paths;
mod pcl;
pub mod svg;

use std::{fs, path::PathBuf};

use hpgl::generate_hpgl;
pub use laser_passes::ToolPass;
pub use paths::resolve_paths;
use paths::{convert_points_to_plotter_units, filter_paths_to_tool_passes};
use pcl::wrap_hpgl_in_pcl;
use serde::{Deserialize, Serialize};
use svg::get_paths_grouped_by_colour;

/// Minimum X position of the X axis in mm.
/// Actually -50.72 but the cutter refuses to move this far...
pub const BED_X_AXIS_MINIMUM_MM: f32 = 0.0;
/// Maximum X position of the X axis in mm.
/// Actual value.
pub const BED_X_AXIS_MAXIMUM_MM: f32 = 901.52;
/// Minimum Y position of the Y axis in mm.
/// Again, actually -4.80 but 🤷.
pub const BED_Y_AXIS_MINIMUM_MM: f32 = 0.0;
/// Maximum Y position of the Y axis in mm.
/// Actual value.
pub const BED_Y_AXIS_MAXIMUM_MM: f32 = 463.20;

/// The width of the cutting area, in mm.
pub const BED_WIDTH_MM: f32 = BED_X_AXIS_MAXIMUM_MM;
/// The height of the cutting area, in mm.
pub const BED_HEIGHT_MM: f32 = BED_Y_AXIS_MAXIMUM_MM;

/// A loaded design.
pub struct DesignFile {
    /// The name of the design.
    pub name: String,
    /// The SVG tree.
    pub tree: usvg::Tree,
    /// The raw bytes of the file.
    pub bytes: Vec<u8>,
    /// Width of the design in mm.
    pub width_mm: f32,
    /// Height of the design in mm.
    pub height_mm: f32,
}

impl DesignFile {
    /// Gets the name of the design.
    ///
    /// # Returns
    /// The name of the design.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the SVG tree.
    ///
    /// # Returns
    /// The parsed SVG tree.
    pub fn tree(&self) -> &usvg::Tree {
        &self.tree
    }
}

/// Offset of a design from the origin (top-left), in mm.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignOffset {
    /// Horizontal axis offset, in mm, where more
    /// positive is more to the right.
    pub x: f32,
    /// Vertical axis offset, in mm, where more
    /// positive is more to the bottom.
    pub y: f32,
}

/// Errors that can occur when sending the design to the HPGL device.
#[derive(Debug)]
pub enum SendToDeviceError {
    /// There was an error while parsing the SVG file.
    ErrorParsingSvg(usvg::Error),
    /// Failed to write to the printer port.
    FailedToWriteToPrinter(String),
    /// Failed to generate valid HPGL.
    GenerateHpglError(String),
}

/// Sends a design file to the printer-like device.
///
/// # Arguments
/// * `design_file`: The design to send to the printer-like device.
/// * `design_name`: The name of the design to be shown to the user.
/// * `tool_passes`: Passes of the cutting tool.
/// * `print_device`: The path to the device to write to.
/// * `offset`: How much to move the design by relative to its starting position, in mm, where +x is more right and +y is more down.
///
/// # Returns
/// `Ok(())` if the file has been sent correctly, otherwise a [`SendToDeviceError`].
///
/// # Errors
/// If there's an error preparing the print file or communicating with the printer.
pub fn cut_file(
    design_file: &usvg::Tree,
    design_name: &str,
    tool_passes: &Vec<ToolPass>,
    print_device: &PathBuf,
    offset: &DesignOffset,
) -> Result<(), SendToDeviceError> {
    let paths = get_paths_grouped_by_colour(design_file);
    let mut paths_in_mm = resolve_paths(&paths, offset, 1.0);
    filter_paths_to_tool_passes(&mut paths_in_mm, tool_passes);
    let resolved_paths = convert_points_to_plotter_units(&paths_in_mm);
    let hpgl = generate_hpgl(&resolved_paths, tool_passes)
        .map_err(SendToDeviceError::GenerateHpglError)?;
    let pcl = wrap_hpgl_in_pcl(hpgl, design_name, tool_passes);
    fs::write(print_device, pcl.as_bytes()).unwrap();

    Ok(())
}
