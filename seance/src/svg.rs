//!`svg`
//!
//! Provides utilities for handling SVG data.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use usvg;

use crate::{paths::PathColour, SendToDeviceError};

/// The number of SVG units per mm. This is based on 96 SVG units per inch.
pub const SVG_UNITS_PER_MM: f32 = 3.779527559;

/// Parses an SVG file and turns it into a tree of paths.
///
/// # Arguments
/// * `path`: The path to the file, will be used to allow the SVG to link to files in the same
/// directory, for example it will be used if the SVG embeds an image via a link.
/// * `bytes`: The bytes of the file.
///
/// # Returns
/// The parsed SVG if it was successfully parsed, otherwise an error.
pub fn parse_svg(path: &PathBuf, bytes: &[u8]) -> Result<usvg::Tree, usvg::Error> {
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    fontdb.set_serif_family("Times New Roman");
    fontdb.set_sans_serif_family("Arial");
    fontdb.set_cursive_family("Comic Sans MS");
    fontdb.set_fantasy_family("Impact");
    fontdb.set_monospace_family("Courier New");

    let resources_dir = path.parent().map(|p| p.to_path_buf());

    let re_opt = usvg::Options {
        resources_dir,
        dpi: 96.0,
        font_family: "Times New Roman".to_string(),
        font_size: 12.0,
        languages: vec!["en-GB".to_string()],
        shape_rendering: Default::default(),
        text_rendering: Default::default(),
        image_rendering: Default::default(),
        default_size: usvg::Size::from_wh(1000.0, 1000.0).expect("Could not set default size"),
        image_href_resolver: usvg::ImageHrefResolver::default(),
        font_resolver: usvg::FontResolver::default(),
        fontdb: Arc::new(fontdb),
        style_sheet: None,
    };

    usvg::Tree::from_data(bytes, &re_opt)
}

/// Finds all of the paths in the SVG and groups them by their stroke colour values.
///
/// # Arguments
/// * `svg`: The SVG to iterate over.
///
/// # Returns
/// The paths grouped by colour if successful, otherwise an error.
pub fn get_paths_grouped_by_colour(
    svg: &usvg::Tree,
) -> Result<HashMap<PathColour, Vec<Box<usvg::Path>>>, SendToDeviceError> {
    let mut grouped_paths = HashMap::new();
    group_paths_by_colour(svg.root(), &mut grouped_paths);

    Ok(grouped_paths)
}

/// Does the actual grouping of paths by colour.
/// Be warned, here be recursion.
/// Images and text are ignored.
///
/// # Arguments
/// * `group`: The SVG group to search through for paths. May contain nested groups.
/// * `grouped_paths`: The path grouping to extend with any new paths found.
fn group_paths_by_colour(
    group: &usvg::Group,
    grouped_paths: &mut HashMap<PathColour, Vec<Box<usvg::Path>>>,
) {
    'iter_children: for child in group.children() {
        match child {
            usvg::Node::Group(child_group) => group_paths_by_colour(child_group, grouped_paths),
            usvg::Node::Path(path) => {
                if let Some(stroke) = path.stroke() {
                    if !path.is_visible() {
                        continue 'iter_children;
                    }

                    if let usvg::Paint::Color(colour) = stroke.paint() {
                        let entry = grouped_paths
                            .entry(PathColour([colour.red, colour.green, colour.blue]))
                            .or_default();
                        entry.push(path.clone());
                    }
                }
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }

        child.subroots(|subroot| group_paths_by_colour(subroot, grouped_paths));
    }
}
