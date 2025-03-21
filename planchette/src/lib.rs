//! # planchette
//!
//! Receives a design file as a sequence of bytes and writes it to `/dev/usb/lp0`

pub use seance;
use seance::{DesignOffset, ToolPass};
use serde::{Deserialize, Serialize};

/// A design to be sent to the printer-like HPGL device.
#[derive(Serialize, Deserialize)]
pub struct PrintJob {
    /// The raw bytes of the SVG file to be cut.
    pub design_file: Vec<u8>,
    /// The name to be displayed to the user as the name of their design.
    pub file_name: String,
    /// The tool passes to use for cutting the design.
    pub tool_passes: Vec<ToolPass>,
    /// The offset of the design from the top-left, in mm.
    pub offset: DesignOffset,
}
