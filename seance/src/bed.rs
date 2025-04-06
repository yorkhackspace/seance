use crate::paths::{PointInMillimeters, ResolvedPoint, MM_PER_PLOTTER_UNIT};

/// Dimensions and offset information for a given device's print bed.
///
/// All measurements are in millimetres.
pub struct PrintBed {
    /// Minimum X position of the X axis.
    pub x_min: f32,
    /// Minimum Y position of the Y axis.
    pub y_min: f32,
    /// Maximum X position of the X axis.
    pub x_max: f32,
    /// Maximum Y position of the Y axis.
    pub y_max: f32,
    /// Width of the cutting area.
    pub width: f32,
    /// Height of the cutting area.
    pub height: f32,
    // TODO: are these values meaningfully different to x_max and y_max?
    /// Whether to "mirror" the X axis.
    ///
    /// This might be desirable because, for example, the GCC Spirit has x=0 at the bottom.
    /// Generally we want 0,0 to be in the top-left, so we would mirror the x axis in this case.
    pub mirror_x: bool,
    /// Whether to "mirror" the Y axis.
    ///
    /// This might be desirable because, for example, the GCC Spirit has x=0 at the bottom.
    /// Generally we want 0,0 to be in the top-left, so we would mirror the x axis in this case.
    pub mirror_y: bool,
}

/// Bed configuration for the [GCC Spirit Laser Engraver](https://www.gccworld.com/product/laser-engraver-supremacy/spirit).
pub const BED_GCC_SPIRIT: PrintBed = PrintBed {
    // Actually -50.72 but the cutter refuses to move this far...
    x_min: 0.0,
    x_max: 901.52,
    // Again, actually -4.80 but 🤷.
    y_min: 0.0,
    y_max: 463.20,

    width: 901.52,
    height: 463.20,

    mirror_x: false,
    mirror_y: true,
};

impl PrintBed {
    /// Converts a mm value into HPGL/2 units.
    ///
    /// # Arguments
    /// * `value`: The value in mm.
    /// * `mirror`: `None` if the value is not to be mirrored,
    ///   `Some(max)` if the value is to be mirrored where `max` is the maximum value on that axis.
    #[inline]
    fn mm_to_hpgl_units(&self, mut value: f32, mirror: Option<f32>) -> i16 {
        if let Some(max) = mirror {
            value = max - value;
        }

        (value / MM_PER_PLOTTER_UNIT).round() as i16
    }

    /// Converts a mm value on the X axis into the value in HPGL/2 units.
    ///
    /// # Arguments
    /// * `value`: The value in mm.
    #[allow(clippy::cast_possible_truncation)]
    pub fn mm_to_hpgl_units_x(&self, value: f32) -> i16 {
        self.mm_to_hpgl_units(value, self.mirror_x.then_some(self.width))
    }

    /// Converts a mm value on the Y axis into the value in HPGL/2 units.
    ///
    /// # Arguments
    /// * `value`: The value in mm.
    #[allow(clippy::cast_possible_truncation)]
    pub fn mm_to_hpgl_units_y(&self, value: f32) -> i16 {
        self.mm_to_hpgl_units(value, self.mirror_y.then_some(self.height))
    }

    /// Takes a vector of points expressed in mm and turns them into a vector of resolved points.
    ///
    /// # Arguments
    /// * `points`: Points in mm to resolve.
    ///
    /// # Returns
    /// The provided points converted to HPGL/2 machine units.
    pub fn points_in_mm_to_printer_units(
        &self,
        points: &[PointInMillimeters],
    ) -> Vec<ResolvedPoint> {
        let mut resolved_points = Vec::with_capacity(points.len());

        for point in points {
            resolved_points.push(ResolvedPoint {
                x: self.mm_to_hpgl_units_x(point.x),
                y: self.mm_to_hpgl_units_y(point.y),
            });
        }

        resolved_points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_to_hpgl_units() {
        let bed = BED_GCC_SPIRIT;

        assert_eq!(bed.mm_to_hpgl_units_y(10.0), 18128, "10mm");
        assert_eq!(bed.mm_to_hpgl_units_x(0.0), 0, "0mm");
        assert_eq!(bed.mm_to_hpgl_units_x(-0.0), 0, "-0mm");

        // extreme values
        assert_eq!(bed.mm_to_hpgl_units_x(f32::MAX), 32767, "f32::MAX mm");
        assert_eq!(
            bed.mm_to_hpgl_units_x(819.175),
            32767,
            "approx maximum computable value"
        );
        assert_eq!(bed.mm_to_hpgl_units_x(f32::MIN), -32768, "f32::MIN mm");
        assert_eq!(
            bed.mm_to_hpgl_units_x(-820.0),
            -32768,
            "approx minimum computable value"
        );
    }

    #[test]
    fn test_points_in_mm_to_printer_units() {
        let bed = BED_GCC_SPIRIT;

        let points = &[
            PointInMillimeters { x: 10.0, y: 10.0 },
            PointInMillimeters { x: 11.0, y: 10.0 },
        ];
        let expected = &[
            ResolvedPoint { x: 400, y: 18128 },
            ResolvedPoint { x: 440, y: 18128 },
        ];

        assert_eq!(&bed.points_in_mm_to_printer_units(points), expected);
    }
}
