//! `hpgl`
//!
//! Contains methods for working with HPGL.

use std::collections::HashMap;

use crate::{
    paths::{mm_to_hpgl_units, PathColour, ResolvedPath},
    ToolPass,
};

/// Generates the HPGL for a design.
///
/// # Aguments
/// * `resolved_paths`: Paths resolved by [`super::paths::resolve_paths`].
/// * `tool_passes`: Tool passes to perform.
///
/// # Returns
/// HPGL as a string.
#[allow(clippy::module_name_repetitions)]
pub fn generate_hpgl(
    resolved_paths: &HashMap<PathColour, Vec<ResolvedPath>>,
    tool_passes: &[ToolPass],
) -> String {
    if tool_passes.len() != 16 {
        return "Exactly 16 tool passes are required".to_string();
    }

    let Some((first_pen, _)) = tool_passes
        .iter()
        .enumerate()
        .find(|(_, pass)| *pass.enabled())
    else {
        return "No tool passes enabled".to_string();
    };

    // In, Default Coordinate System, Pen Up, Select first pen, reset line type, move to 0,0.
    let var_name = format!(
        "IN;SC;PU;{}LT;PU{},{};",
        pen_change(first_pen),
        mm_to_hpgl_units(0.0, true),
        mm_to_hpgl_units(0.0, false)
    );
    let mut hpgl = var_name;

    'laser_passes_iter: for (index, pass) in tool_passes.iter().enumerate() {
        if let Some(paths) = resolved_paths.get(&PathColour(*pass.colour())) {
            if paths.is_empty() {
                continue 'laser_passes_iter;
            }

            append_hpgl(&mut hpgl, &pen_change(index));
            for path in paths {
                append_hpgl(&mut hpgl, &trace_path(path));
            }
        }
    }

    hpgl.push_str(&format!(
        "PU{},{};SP0;EC0;EC1;OE;",
        mm_to_hpgl_units(0.0, true),
        mm_to_hpgl_units(0.0, false)
    ));

    hpgl
}

/// Appends some HPGL to the end of an existing HPGL string.
///
/// # Arguments
/// * `hpgl`: The HPGL to modify in-place.
/// * `to_append`: The HPGL to add to the end of the HPGL string.
fn append_hpgl(hpgl: &mut String, to_append: &str) {
    hpgl.push_str(to_append);
}

/// Generate the HPGL for a pen change.
///
/// # Arguments
/// * `pen_index`: The pen index (from 0) to change to.
///
/// # Returns
/// The HPGL for the pen change.
fn pen_change(pen_index: usize) -> String {
    // Select Pen X.
    format!("SP{};", pen_index + 1)
}

/// Creates a HPGL string that traces through all of the points in a path.
///
/// # Arguments
/// * `path`: The path to trace.
///
/// # Returns
/// The HPGL for the traced path.
fn trace_path(path: &ResolvedPath) -> String {
    let mut hpgl = String::new();

    // Move to starting position with pen up, then pen down.
    if let Some(point) = path.first() {
        let x = point.x;
        let y = point.y;
        hpgl.push_str(&format!("PU{x},{y};PD{x},{y};"));
    }

    let mut point_strs = vec![];
    for point in path {
        let x = point.x;
        let y = point.y;
        point_strs.push(format!("{x},{y}"));
    }
    hpgl.push_str(&format!("PA{};PU;", point_strs.join(",")));

    hpgl
}
