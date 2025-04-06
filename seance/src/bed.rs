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
    /// Converts a [`PointInMillimeters`] into the same point in HPGL/2 units **for this printer**.
    ///
    /// Returns `None` if the point is out of the bed of this printer.
    ///
    /// # Arguments
    /// * `point`: The point to convert from mm.
    ///
    /// # Panics
    /// Panics when the values of `self` would cause truncation at the origin.
    pub fn place_point(&self, point: PointInMillimeters) -> Option<ResolvedPoint> {
        #[inline]
        fn mm_to_hpgl(mut value: f32, mirror: Option<f32>) -> Option<i16> {
            if let Some(max) = mirror {
                value = max - value;
            }

            let adjusted = value / MM_PER_PLOTTER_UNIT;
            if adjusted > i16::MAX as f32 || adjusted < i16::MIN as f32 {
                // value would be truncated
                None
            } else {
                Some(adjusted.round() as i16)
            }
        }

        // check printer bed sizes won't automatically cause truncation
        // TODO: do this in constructor?
        debug_assert!(
            self.mirror_x && mm_to_hpgl(self.x_max, None).is_none(),
            "x-axis mirroring is enabled but the axis is so large it would truncate"
        );
        debug_assert!(
            self.mirror_y && mm_to_hpgl(self.y_max, None).is_none(),
            "y-axis mirroring is enabled but the axis is so large it would truncate"
        );

        Some(ResolvedPoint {
            x: mm_to_hpgl(point.x, self.mirror_x.then_some(self.width))?,
            y: mm_to_hpgl(point.y, self.mirror_y.then_some(self.height))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_to_hpgl_units() {
        let bed = BED_GCC_SPIRIT;

        assert_eq!(
            bed.place_point((10.0, 10.0).into()).unwrap(),
            (400, 18128).into(),
            "10mm"
        );
        assert_eq!(
            bed.place_point((0.0, 0.0).into()).unwrap(),
            (0, 18528).into(),
            "0mm"
        );
        assert_eq!(
            bed.place_point((-0.0, -0.0).into()).unwrap(),
            (0, 18528).into(),
            "-0mm"
        );

        // extreme values
        assert!(
            bed.place_point((f32::MAX, f32::MAX).into()).is_none(),
            "f32::MAX mm"
        );
        assert_eq!(
            bed.place_point((819.175, 819.175).into()).unwrap(),
            (32767, -14239).into(),
            "approx maximum computable value"
        );
        assert!(
            bed.place_point((f32::MIN, f32::MIN).into()).is_none(),
            "f32::MIN mm"
        );
        assert!(
            bed.place_point((-818.0, -818.0).into()).is_none(),
            "negative values"
        );
    }
}
