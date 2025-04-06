//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

pub mod bed;
pub mod default_passes;
pub mod hpgl;
mod laser_passes;
pub mod paths;
pub mod pcl;
pub mod svg;

use std::{fs, path::PathBuf};

use bed::PrintBed;
use hpgl::generate_hpgl;
pub use laser_passes::ToolPass;
pub use paths::resolve_paths;
use paths::{convert_points_to_plotter_units, filter_paths_to_tool_passes};
use pcl::wrap_hpgl_in_pcl;
use serde::{Deserialize, Serialize};
use svg::get_paths_grouped_by_colour;

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
    print_bed: &PrintBed,
    print_device: &PathBuf,
    offset: &DesignOffset,
) -> Result<(), SendToDeviceError> {
    let paths = get_paths_grouped_by_colour(design_file);
    let mut paths_in_mm = resolve_paths(&paths, offset, 1.0);
    filter_paths_to_tool_passes(&mut paths_in_mm, tool_passes);
    let resolved_paths = convert_points_to_plotter_units(&paths_in_mm, print_bed);
    let hpgl = generate_hpgl(&resolved_paths, tool_passes, print_bed);
    let pcl = wrap_hpgl_in_pcl(hpgl, design_name, tool_passes);
    fs::write(print_device, pcl.as_bytes()).unwrap();

    Ok(())
}
