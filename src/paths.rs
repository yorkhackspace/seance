//! `paths`
//!
//! Provides utilities for tracing paths, turning them into a set of points that
//! the toolhead moves through.

use std::collections::HashMap;

use lyon_algorithms::geom::euclid::UnknownUnit;
use lyon_algorithms::path::math::Point;
use lyon_algorithms::path::PathSlice;
use lyon_algorithms::walk::{walk_along_path, RegularPattern, WalkerEvent};
use resvg::usvg;
use usvg::Path;

use crate::{ToolPass, BED_HEIGHT_MM};

/// The number of mm that are moved per unit that the plotter is instructed to move.
/// This is the HPGL/2 default specified in the HPGL/2 specification.
const MM_PER_PLOTTER_UNIT: f32 = 0.025;

/// This is a point that is along a path that we wish to trace with the tool.
/// The units are HPGL/2 units, which are rather nebulous and may vary from
/// machine to machine in terms of their translation to mm.
pub struct ResolvedPoint {
    /// Horizontal axis position.
    pub x: i16,
    /// Vertical axis position.
    pub y: i16,
}
/// A path that the toolhead will move through, comprised of a series of points in-order.
pub type ResolvedPath = Vec<ResolvedPoint>;

/// The colour associated with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathColour(pub [u8; 3]);

/// Takes a set of SVG paths grouped by their colour and traces them, turning
/// the paths into a set of points for the toolhead to move through.
///
/// # Arguments
/// * `paths_grouped_by_colour`: The paths to be traced, grouped by their colour.
/// * `tool_passes`: The toolhead passes to be done.
///
/// # Returns
/// A set of resolved paths, grouped by path colour.
pub fn resolve_paths(
    paths_grouped_by_colour: &HashMap<PathColour, Vec<Box<Path>>>,
    tool_passes: &[ToolPass; 16],
) -> HashMap<PathColour, Vec<ResolvedPath>> {
    let mut resolved_paths: HashMap<PathColour, Vec<ResolvedPath>> = HashMap::new();

    for pass in tool_passes {
        let path_colour = PathColour(pass.colour().to_owned());
        if let Some(paths) = paths_grouped_by_colour.get(&path_colour) {
            for path in paths {
                let mut path_builder = lyon_algorithms::path::Path::builder();
                let mut closed = false;
                for segment in path.data().segments() {
                    match segment {
                        usvg::tiny_skia_path::PathSegment::MoveTo(point) => {
                            path_builder.begin(
                                PointInMillimeters {
                                    x: point.x,
                                    y: point.y,
                                }
                                .into(),
                            );
                        }
                        usvg::tiny_skia_path::PathSegment::LineTo(point) => {
                            path_builder.line_to(
                                PointInMillimeters {
                                    x: point.x,
                                    y: point.y,
                                }
                                .into(),
                            );
                        }
                        // The target point is the end of the curve, the control point is somewhere in the middle.
                        usvg::tiny_skia_path::PathSegment::QuadTo(control_point, target_point) => {
                            path_builder.quadratic_bezier_to(
                                PointInMillimeters {
                                    x: control_point.x,
                                    y: control_point.y,
                                }
                                .into(),
                                PointInMillimeters {
                                    x: target_point.x,
                                    y: target_point.y,
                                }
                                .into(),
                            );
                        }
                        // The target point is the end of the curve, the first control point is towards the beginning of the curve, the second control point is towards the end of the curve.
                        usvg::tiny_skia_path::PathSegment::CubicTo(
                            first_control_point,
                            second_control_point,
                            target_point,
                        ) => {
                            path_builder.cubic_bezier_to(
                                PointInMillimeters {
                                    x: first_control_point.x,
                                    y: first_control_point.y,
                                }
                                .into(),
                                PointInMillimeters {
                                    x: second_control_point.x,
                                    y: second_control_point.y,
                                }
                                .into(),
                                PointInMillimeters {
                                    x: target_point.x,
                                    y: target_point.y,
                                }
                                .into(),
                            );
                        }
                        usvg::tiny_skia_path::PathSegment::Close => {
                            path_builder.end(true);
                            closed = true;
                        }
                    }
                }

                if !closed {
                    path_builder.end(false);
                }

                let mut resolved_points = vec![];

                let built_path = path_builder.build();
                let mut points = vec![];
                points_along_path(built_path.as_slice(), &mut points);
                for point in points {
                    resolved_points.push(point.into());
                }

                let entry = resolved_paths.entry(path_colour).or_default();
                entry.push(points_in_mm_to_printer_units(resolved_points));
            }
        }
    }

    resolved_paths
}

/// A point in terms of mm.
#[derive(Debug, Clone, Copy)]
struct PointInMillimeters {
    /// Horizontal axis.
    x: f32,
    /// Vertical axis.
    y: f32,
}

impl From<PointInMillimeters> for lyon_algorithms::geom::euclid::Point2D<f32, UnknownUnit> {
    fn from(value: PointInMillimeters) -> Self {
        lyon_algorithms::geom::euclid::Point2D::new(value.x, value.y)
    }
}

impl From<lyon_algorithms::geom::euclid::Point2D<f32, UnknownUnit>> for PointInMillimeters {
    fn from(value: lyon_algorithms::geom::euclid::Point2D<f32, UnknownUnit>) -> Self {
        PointInMillimeters {
            x: value.x,
            y: value.y,
        }
    }
}

/// Works out the points along a path and adds them to a vector of points.
///
/// # Arguments
/// * `path`: The path to trace.
/// * `points`: The vector of points to push new points into.
fn points_along_path<'path_slice>(path: PathSlice<'path_slice>, points: &mut Vec<Point>) {
    let mut pattern = RegularPattern {
        callback: &mut |event: WalkerEvent<'_>| {
            points.push(event.position);

            // Return true to continue walking the path.
            true
        },
        // Invoke the callback above at a regular interval of 1.0 units.
        interval: 1.0,
    };

    // The path flattening tolerance.
    let tolerance = 0.1;
    // Start walking at the beginning of the path.
    let start_offset = 0.0;
    walk_along_path(path.iter(), start_offset, tolerance, &mut pattern);
}

/// Takes a vector of points expressed in mm and turns them into a vector of resolved points.
///
/// # Arguments
/// * `points`: Points in mm to resolve.
///
/// # Returns
/// The provided points converted to HPGL/2 machine units.
fn points_in_mm_to_printer_units(points: Vec<PointInMillimeters>) -> Vec<ResolvedPoint> {
    let mut resolved_points = Vec::with_capacity(points.len());

    for point in points {
        resolved_points.push(ResolvedPoint {
            x: mm_to_hpgl_units(point.x, true),
            y: mm_to_hpgl_units(point.y, false),
        })
    }

    resolved_points
}

/// Converts a mm value into the value in HPGL/2 units.
///
/// # Arguments
/// * `mm`: The value in mm.
/// * `mirror_x_axis`: The GCC Spirit has x=0 at the bottom. Generally we want 0,0 to be
/// in the top-left, so we mirror the x axis in this case.
pub fn mm_to_hpgl_units(mm: f32, is_x_axis: bool) -> i16 {
    let position_mm = if is_x_axis { mm } else { BED_HEIGHT_MM - mm };
    (position_mm / MM_PER_PLOTTER_UNIT).round() as i16
}
