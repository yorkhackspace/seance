//! `pcl`
//!
//! Generates PCL to send to a machine.

use crate::ToolPass;

/// The escape character, we insert this _a lot_.
const ESC: char = '\x1b';

/// Take some HPGL and wrap it in PCL.
///
/// # Arguments
/// * `hpgl`: The HPGL to be wrapped in PCL.
/// * `filename`: This will be displayed on the screen of the machine, so should be recognisable to the user.
/// * `laser_passes`: The passes of the toolhead to perform.
///
/// # Returns
/// PCL string that can be sent to the machine.
#[allow(clippy::module_name_repetitions)]
pub fn wrap_hpgl_in_pcl(hpgl: String, filename: &str, laser_passes: &Vec<ToolPass>) -> String {
    vec![
        pjl_universal_exit_language(),
        pcl_reset(),
        pcl_filename(filename),
        pcl_pen_table(laser_passes),
        pcl_raster_resolution(508),
        pcl_unit_of_measure(508),
        format!("{ESC}!r0N"),
        pcl_enter_pcl_mode(),
        format!("{ESC}!r1000I{ESC}!r1000K{ESC}!r500P"),
        pcl_raster_resolution(508),
        pcl_unit_of_measure(508),
        format!("{ESC}!m0S{ESC}!s1S"),
        pcl_enter_hpgl_mode(),
        hpgl,
        pcl_enter_pcl_mode(),
        pcl_reset(),
        pjl_universal_exit_language(),
    ]
    .join("")
}

/// Insert the Printer Job Language (PJL) Universal Exit Language (UEL) command.
/// Right so this instructs a printer to switch from Printer Job
/// Language to Printer Control Language. Clear? No? Well you see
/// CNC machines are actually printers, so receive print jobs. A print job
/// starts in the PJL context, and to switch from PJL to other command languages
/// you must send the UEL command. It also needs to be sent at the end of the job to
/// allow the printer to return to the PJL context.
///
/// # Returns
/// The Universal Exit Language command.
fn pjl_universal_exit_language() -> String {
    format!("{ESC}%-12345X")
}

/// Sending this command enters PCL and resets the printer in this mode.
/// This must be sent before any other PCL commands. It is also good manners to
/// send this at the end to return the tool and bed to their home positions.
///
/// # Returns
/// The PCL reset command.
fn pcl_reset() -> String {
    format!("{ESC}E")
}

/// Tells PCL to report the filename of the print job.
///
/// # Arguments
/// * `filename`: The filename to report.
///
/// # Returns
/// Command to report the filename.
fn pcl_filename(filename: &str) -> String {
    let len = filename.len();
    format!("{ESC}!m{len}N{filename}")
}

/// Constructs the table of 'pens'.
/// A pen is a pass of the tool. Think about CNC machines as being pen plotters.
/// I mean, they basically are right?
/// Like, they draw on a material. They just might be drawing on a sheet of brass using
/// a tool that is spinning at several thousand RPM.
/// As much as a pen plotter may move the pen at a different speed with a different pressure to
/// achieve different line styles, a CNC can move its tool at different speeds and different 'powers'
/// (e.g. laser power) in order to perform different kinds of cut.
/// Therefore a single pass of the tool of a CNC machine is a 'pen'!
///
/// # Arguments
/// * `tool_passes`: The tool passes to perform.
///
/// # Returns
/// A PCL string containing the pens table.
fn pcl_pen_table(tool_passes: &Vec<ToolPass>) -> String {
    let num_pens = tool_passes.len();
    let message_bytes = num_pens * 4;

    let mut result = String::new();
    result += &format!("{ESC}!v{num_pens}R");

    for _ in tool_passes {
        result.extend(['1']);
    }

    // Pen PPI
    result += &format!("{ESC}!v{message_bytes}I");
    for _ in tool_passes {
        result += "0400";
    }

    // Pen Speed
    result += &format!("{ESC}!v{message_bytes}V");
    for pen in tool_passes {
        result += &format!("{:0>4}", pen.speed());
    }

    // Pen Power
    result += &format!("{ESC}!v{message_bytes}P");
    for pen in tool_passes {
        result += &format!("{:0>4}", pen.power());
    }

    // Pen enable.
    // TODO: Should be based on enabled pens.
    result += &format!("{ESC}!v{num_pens}D");
    for pass in tool_passes {
        if *pass.enabled() {
            result.push(ascii::AsciiChar::SOX.into());
        } else {
            result.push(ascii::AsciiChar::Null.into());
        }
    }

    result
}

/// Sets the resolution of rasterization performed by PCL.
///
/// # Arguments
/// * `dpi`: The DPI to use for rasterization.
///
/// # Returns
/// DPI set command.
fn pcl_raster_resolution(dpi: u64) -> String {
    format!("{ESC}*t{dpi}R")
}

/// Sets the DPI equivalent of a single machine unit.
///
/// # Arguments
/// * `dpi`: The DPI to use.
///
/// # Returns
/// Unit of measure set command.
fn pcl_unit_of_measure(dpi: u64) -> String {
    format!("{ESC}&u{dpi}R")
}

/// Enters PCL mode inside of PCL.
/// ...
/// Right ok so PCL is a language but also a way of ~life~ thinking.
/// Printers can operate in different modes when speaking PCL, but
/// PCL mode is the "original" way of doing things and what we want.
///
/// # Returns
/// The enter PCL mode command.
fn pcl_enter_pcl_mode() -> String {
    format!("{ESC}%1A")
}

/// Enters HPGL mode inside of PCL.
/// ...
/// Yeah so HPGL is a sub-mode of PCL. Ish. For our purposes.
///
/// # Returns
/// The enter HPGL mode command.
fn pcl_enter_hpgl_mode() -> String {
    format!("{ESC}%1B")
}
