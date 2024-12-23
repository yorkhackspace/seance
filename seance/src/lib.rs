//! `seance`
//!
//! A utility for talking to devices that speak HPGL.

mod app;
mod default_passes;
mod hpgl;
mod laser_passes;
mod paths;
mod pcl;
mod svg;

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub use app::Seance;
pub use app::{render_task, RenderRequest};
use egui::Vec2;
use hpgl::generate_hpgl;
use laser_passes::ToolPass;
use paths::resolve_paths;
use pcl::wrap_hpgl_in_pcl;
use resvg::usvg;
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

/// The default print device to use on non-Windows systems.
#[cfg(not(target_os = "windows"))]
pub const DEFAULT_PRINT_DEVICE: &'static str = "/dev/usb/lp0";

/// A loaded design.
pub struct DesignFile {
    /// The name of the design.
    name: String,
    /// The path the design was loaded from.
    path: PathBuf,
    /// The hash of the file.
    hash: u64,
    /// The SVG tree.
    tree: usvg::Tree,
    /// Width of the design in mm.
    width_mm: f32,
    /// Height of the design in mm.
    height_mm: f32,
}

impl DesignFile {
    /// Gets the name of the design.
    ///
    /// # Returns
    /// The name of the design.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the path that the design was loaded from.
    ///
    /// # Returns
    /// The path of the design file.
    pub fn path(&self) -> &PathBuf {
        &self.path
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
    tool_passes: &[ToolPass; 16],
    print_device: &PrintDevice,
    offset: &Vec2,
) -> Result<(), SendToDeviceError> {
    let design_name = design_file.name();

    let paths = get_paths_grouped_by_colour(&design_file.tree)?;
    let resolved_paths = resolve_paths(&paths, &tool_passes, offset);
    let hpgl = generate_hpgl(&resolved_paths, &tool_passes);
    let pcl = wrap_hpgl_in_pcl(hpgl, &design_name, &tool_passes);
    print_device.print(&pcl)?;

    Ok(())
}

/// Gets all of the possible capitalisations of a string.
/// We need this because the library we use for showig file dialogs is not very clever,
/// it does not match against file extensions case-insensitively. Therefore, we provide
/// the file dialog library with all of the possible capitalisations of the file extensions
/// we care about, just in case folks have bizarrely capitalised file extensions.
///
/// # Arguments
/// * `input`: The string to generate all the capitalisations of.
///
/// # Returns
/// An array of strings containing all of the possible capitalisations of the input string.
pub fn all_capitalisations_of(input: &str) -> Vec<String> {
    let mut result = vec![];

    let bitmask = ((2_u32.pow(input.len() as u32)) - 1) as usize;

    for mask in 0..=bitmask {
        let mut new_str = String::new();
        for i in 0..input.len() {
            if mask & (1 << i) > 0 {
                new_str += &input
                    .chars()
                    .nth(i)
                    .expect(&format!("Could not get character {i}"))
                    .to_uppercase()
                    .to_string();
            } else {
                new_str += &input
                    .chars()
                    .nth(i)
                    .expect(&format!("Could not get character {i}"))
                    .to_lowercase()
                    .to_string();
            }
        }
        result.push(new_str);
    }

    result
}

#[cfg(test)]
mod test {
    use crate::all_capitalisations_of;

    #[test]
    fn capitalisations() {
        let mut result = all_capitalisations_of("svg");
        result.sort();
        assert_eq!(result.len(), 8);
        assert_eq!(
            result,
            vec!["SVG", "SVg", "SvG", "Svg", "sVG", "sVg", "svG", "svg"]
        )
    }
}
