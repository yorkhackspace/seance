//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

pub mod default_passes;
mod hpgl;
mod laser_passes;
mod paths;
mod pcl;
pub mod svg;

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};

use hpgl::generate_hpgl;
pub use laser_passes::ToolPass;
pub use paths::resolve_paths;
use paths::{convert_points_to_plotter_units, filter_paths_to_tool_passes};
use pcl::wrap_hpgl_in_pcl;
use svg::get_paths_grouped_by_colour;
use usvg;

type Vec2 = (f32, f32);

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

/// The default print device to use on non-Windows systems.
#[cfg(not(target_os = "windows"))]
pub const DEFAULT_PRINT_DEVICE: &'static str = "/dev/usb/lp0";

/// A loaded design.
pub struct DesignFile {
    /// The name of the design.
    pub name: String,
    /// The SVG tree.
    pub tree: usvg::Tree,
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

/// Errors that can occur when sending the design to the HPGL device.
#[derive(Debug)]
pub enum SendToDeviceError {
    /// There was an error while parsing the SVG file.
    ErrorParsingSvg(usvg::Error),
    /// Failed to open the printer port.
    FailedToOpenPrinter(io::Error),
    /// Failed to write to the printer port.
    FailedToWriteToPrinter(io::Error),
}

/// The printer-like device that we're using.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub enum PrintDevice {
    /// We're printing to a device path.
    #[cfg(not(target_os = "windows"))]
    Path {
        /// The path to send the bytes to.
        path: String,
    },
    /// We're using a USB port.
    #[cfg(target_os = "windows")]
    USBPort {
        /// The USB port to use.
        port: Option<USBPort>,
    },
}

/// Represents a USB port.
#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct USBPort {
    /// The vendor Id of the USB device.
    vendor_id: u16,
    /// The product Id of the USB device.
    product_id: u16,
}

impl PrintDevice {
    /// Sends a PCL string to the printer-like device.
    ///
    /// # Arguments
    /// * `design`: The PCL to print.
    ///
    /// # Returns
    /// `Ok(())` if the PCL was successfully sent to the printer, otherwise a [`SendToDeviceError`].
    fn print(&self, design: &str) -> Result<(), SendToDeviceError> {
        match self {
            #[cfg(not(target_os = "windows"))]
            PrintDevice::Path { path } => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(false)
                    .append(true)
                    .open(path)
                    .map_err(SendToDeviceError::FailedToOpenPrinter)?;
                file.write(design.as_bytes())
                    .map_err(SendToDeviceError::FailedToWriteToPrinter)?;

                Ok(())
            }
            #[cfg(target_os = "windows")]
            PrintDevice::USBPort { port } => {
                let api = hidapi_rusb::HidApi::new().unwrap();
                if let Some(port) = port {
                    if let Ok(device) = api.open(port.vendor_id, port.product_id) {
                        device.write(design.as_bytes()).expect("Failed to print");
                    }
                }
            }
        }
    }

    /// Checks whether the print device is valid to be used for printing.
    ///
    /// # Returns
    /// `true` if the print device is valid to be used for printing.
    pub fn is_valid(&self) -> bool {
        match self {
            #[cfg(not(target_os = "windows"))]
            PrintDevice::Path { path } => Path::new(path).exists(),
            #[cfg(target_os = "windows")]
            PrintDevice::USBPort { port } => {
                let Some(port) = port else {
                    return false;
                };

                let api = hidapi_rusb::HidApi::new().unwrap();
                api.open(port.vendor_id, port.product_id).is_ok()
            }
        }
    }
}

impl Default for PrintDevice {
    #[cfg(not(target_os = "windows"))]
    fn default() -> Self {
        PrintDevice::Path {
            path: DEFAULT_PRINT_DEVICE.to_string(),
        }
    }

    #[cfg(target_os = "windows")]
    fn default() -> Self {
        let port = usb_enumeration::enumerate(None, None)
            .first()
            .map(|port| USBPort {
                vendor_id: port.vendor_id,
                product_id: port.product_id,
            });
        PrintDevice::USBPort { port }
    }
}

/// Sends a design file to the printer-like device.
///
/// # Arguments
/// * `design_file`: The design to send to the printer-like device.
/// * `tool_passes`: Passes of the cutting tool.
/// * `print_device`: The device to send the design to.
/// * `offset`: How much to move the design by relative to its starting position, in mm, where +x is more right and +y is more down.
///
/// # Returns
/// `Ok(())` if the file has been sent correctly, otherwise a [`SendToDeviceError`].
pub fn cut_file(
    design_file: &DesignFile,
    tool_passes: &Vec<ToolPass>,
    print_device: &PrintDevice,
    offset: Vec2,
) -> Result<(), SendToDeviceError> {
    let design_name = design_file.name();

    let paths = get_paths_grouped_by_colour(&design_file.tree)?;
    let mut paths_in_mm = resolve_paths(&paths, offset, 1.0);
    filter_paths_to_tool_passes(&mut paths_in_mm, tool_passes);
    let resolved_paths = convert_points_to_plotter_units(&paths_in_mm);
    let hpgl = generate_hpgl(&resolved_paths, &tool_passes);
    let pcl = wrap_hpgl_in_pcl(hpgl, &design_name, &tool_passes);
    print_device.print(&pcl)?;

    Ok(())
}
