//! `paths`
//!
//! Provides utilities for tracing paths, turning them into a set of points that
//! the toolhead moves through.

use std::collections::HashMap;

use lyon_algorithms::geom::euclid::UnknownUnit;
use lyon_algorithms::path::math::Point;
use lyon_algorithms::path::PathSlice;
use lyon_algorithms::walk::{walk_along_path, RegularPattern, WalkerEvent};
use usvg::Path;

use crate::{DesignOffset, ToolPass, BED_HEIGHT_MM};

/// The number of mm that are moved per unit that the plotter is instructed to move.
/// This is the HPGL/2 default specified in the HPGL/2 specification.
const MM_PER_PLOTTER_UNIT: f32 = 0.025;

/// This is a point that is along a path that we wish to trace with the tool.
/// The units are HPGL/2 units, which are rather nebulous and may vary from
/// machine to machine in terms of their translation to mm.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedPoint {
    /// Horizontal axis position.
    pub x: i16,
    /// Vertical axis position.
    pub y: i16,
}
/// A path that the toolhead will move through, comprised of a series of points in-order.
pub type ResolvedPath = Vec<ResolvedPoint>;
/// A toolpath expressed as a series of points in mm.
pub type PathInMM = Vec<PointInMillimeters>;

/// The colour associated with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathColour(pub [u8; 3]);

impl PartialEq<PathColour> for [u8; 3] {
    fn eq(&self, other: &PathColour) -> bool {
        other.eq(self)
    }
}

impl PartialEq<[u8; 3]> for PathColour {
    fn eq(&self, other: &[u8; 3]) -> bool {
        self.0.eq(other)
    }
}

/// Takes a set of SVG paths grouped by their colour and traces them, turning
/// the paths into a set of points expressed in mm.
///
/// # Arguments
/// * `paths_grouped_by_colour`: The paths to be traced, grouped by their colour.
/// * `offset`: How much to move the design by relative to its starting position, in mm, where +x is more right and +y is more down.
/// * `interval`: How often to sample along a path, in SVG units.
///
/// # Returns
/// A set of resolved paths, grouped by path colour.
#[allow(clippy::module_name_repetitions)]
#[allow(clippy::implicit_hasher)]
pub fn resolve_paths(
    paths_grouped_by_colour: &HashMap<PathColour, Vec<Box<Path>>>,
    offset: &DesignOffset,
    interval: f32,
) -> HashMap<PathColour, Vec<PathInMM>> {
    let mut resolved_paths: HashMap<PathColour, Vec<PathInMM>> = HashMap::new();

    for (path_colour, paths) in paths_grouped_by_colour {
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
                    // The target point is the end of the curve, the first control point is towards the beginning
                    // of the curve, the second control point is towards the end of the curve.
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
            points_along_path(built_path.as_slice(), &mut points, interval);
            if closed {
                if let Some(first_point) = points.first() {
                    points.push(*first_point);
                }
            }
            for mut point in points {
                offset_point(&mut point, offset);
                resolved_points.push(point.into());
            }

            let entry = resolved_paths.entry(*path_colour).or_default();
            entry.push(resolved_points);
        }
    }

    resolved_paths
}

/// Filter a set of paths to only the paths that are covered by (enabled) tool passes.
///
/// # Arguments
/// * `paths`: The set of paths to filter, will be modified in-place.
/// * `tool_passes`: The tool passes to filter down to.
pub fn filter_paths_to_tool_passes(
    paths: &mut HashMap<PathColour, Vec<PathInMM>>,
    tool_passes: &[ToolPass],
) {
    paths.retain(|colour, _| {
        tool_passes
            .iter()
            .any(|pass| pass.colour() == colour && *pass.enabled())
    });
}

/// Convert paths expressed as a series of points recorded as mm values to paths expressed as a series of points in plotter units.
///
/// # Arguments
/// * `paths_in_mm`: The paths to be converted from mm to plotter units.
///
/// # Returns
/// The paths expressed in plotter units.
pub fn convert_points_to_plotter_units(
    paths_in_mm: &HashMap<PathColour, Vec<PathInMM>>,
) -> HashMap<PathColour, Vec<ResolvedPath>> {
    let mut resolved_paths: HashMap<PathColour, Vec<ResolvedPath>> =
        HashMap::with_capacity(paths_in_mm.capacity());
    for (path_colour, paths) in paths_in_mm {
        for path in paths {
            let entry = resolved_paths.entry(*path_colour).or_default();
            entry.push(points_in_mm_to_printer_units(path));
        }
    }
    resolved_paths
}

/// A point in terms of mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointInMillimeters {
    /// Horizontal axis.
    pub x: f32,
    /// Vertical axis.
    pub y: f32,
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
/// * `interval`: How often to sample along a path, in SVG units.
fn points_along_path(path: PathSlice<'_>, points: &mut Vec<Point>, interval: f32) {
    let mut pattern = RegularPattern {
        callback: &mut |event: WalkerEvent<'_>| {
            points.push(event.position);

            // Return true to continue walking the path.
            true
        },
        interval,
    };

    // The path flattening tolerance.
    let tolerance = 0.1;
    // Start walking at the beginning of the path.
    let start_offset = 0.0;
    walk_along_path(path.iter(), start_offset, tolerance, &mut pattern);
}

/// Offset a point, in place.
///
/// # Arguments
/// * `point`: The point to offset.
/// * `offset_x`: Offset in mm, where +x is more right
/// * `offset_y`: Offset in mm, where +y is more down.
fn offset_point(
    point: &mut Point,
    DesignOffset {
        x: offset_x,
        y: offset_y,
    }: &DesignOffset,
) {
    point.x += offset_x;
    point.y += offset_y;
}

/// Takes a vector of points expressed in mm and turns them into a vector of resolved points.
///
/// # Arguments
/// * `points`: Points in mm to resolve.
///
/// # Returns
/// The provided points converted to HPGL/2 machine units.
fn points_in_mm_to_printer_units(points: &[PointInMillimeters]) -> Vec<ResolvedPoint> {
    let mut resolved_points = Vec::with_capacity(points.len());

    for point in points {
        resolved_points.push(ResolvedPoint {
            x: mm_to_hpgl_units(point.x, true),
            y: mm_to_hpgl_units(point.y, false),
        });
    }

    resolved_points
}

/// Converts a mm value into the value in HPGL/2 units.
///
/// # Arguments
/// * `mm`: The value in mm.
/// * `is_x_axis`: The GCC Spirit has x=0 at the bottom. Generally we want 0,0 to be
///   in the top-left, so we mirror the x axis in this case.
#[allow(clippy::cast_possible_truncation)]
pub fn mm_to_hpgl_units(mm: f32, is_x_axis: bool) -> i16 {
    let position_mm = if is_x_axis { mm } else { BED_HEIGHT_MM - mm };
    (position_mm / MM_PER_PLOTTER_UNIT).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_to_hpgl_units() {
        assert_eq!(mm_to_hpgl_units(10.0, false), 18128, "10mm");
        assert_eq!(mm_to_hpgl_units(0.0, true), 0, "0mm");
        assert_eq!(mm_to_hpgl_units(-0.0, true), 0, "-0mm");

        // extreme values
        assert_eq!(mm_to_hpgl_units(f32::MAX, true), 32767, "f32::MAX mm");
        assert_eq!(
            mm_to_hpgl_units(819.175, true),
            32767,
            "approx maximum computable value"
        );
        assert_eq!(mm_to_hpgl_units(f32::MIN, true), -32768, "f32::MIN mm");
        assert_eq!(
            mm_to_hpgl_units(-820.0, true),
            -32768,
            "approx minimum computable value"
        );
    }

    #[test]
    fn test_points_in_mm_to_printer_units() {
        let points = &[
            PointInMillimeters { x: 10.0, y: 10.0 },
            PointInMillimeters { x: 11.0, y: 10.0 },
        ];
        let expected = &[
            ResolvedPoint { x: 400, y: 18128 },
            ResolvedPoint { x: 440, y: 18128 },
        ];

        assert_eq!(&points_in_mm_to_printer_units(points), expected);
    }

    #[test]
    fn test_filter_paths_to_tool_passes() {
        let mut passes = crate::default_passes::default_passes();
        // enable black
        passes[0].set_enabled(true);

        let mut paths = [
            // black, is enabled
            (
                PathColour([0, 0, 0]),
                vec![vec![PointInMillimeters { x: 15.0, y: 100.5 }]],
            ),
            // dark grey, not used as a tool pass
            (
                PathColour([10, 10, 10]),
                vec![vec![PointInMillimeters { x: 500.0, y: -10.0 }]],
            ),
            // white - present but not enabled
            (
                PathColour([255, 255, 255]),
                vec![vec![PointInMillimeters {
                    x: -1010.5,
                    y: -f32::MAX,
                }]],
            ),
        ]
        .into_iter()
        .collect();

        let expected = [(
            PathColour([0, 0, 0]),
            vec![vec![PointInMillimeters { x: 15.0, y: 100.5 }]],
        )]
        .into_iter()
        .collect();

        filter_paths_to_tool_passes(&mut paths, &passes);
        assert_eq!(paths, expected)
    }
}
