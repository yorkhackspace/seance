use std::{ops::RangeInclusive, sync::LazyLock};

use crate::paths::{PointInMillimeters, ResolvedPoint, MM_PER_PLOTTER_UNIT};

/// Dimensions and offset information for a given device's print bed.
///
/// All measurements are in millimetres.
pub struct PrintBed {
    /// Value ranges of the X axis.
    x_axis: RangeInclusive<f32>,
    /// Value ranges of the Y axis.
    y_axis: RangeInclusive<f32>,
    /// Whether to "mirror" the X axis.
    ///
    /// This might be desirable because, for example, the GCC Spirit has x=0 at the bottom.
    /// Generally we want 0,0 to be in the top-left, so we would mirror the x axis in this case.
    pub mirror_x: bool,
    /// Whether to "mirror" the Y axis.
    pub mirror_y: bool,
}

/// Bed configuration for the [GCC Spirit Laser Engraver](https://www.gccworld.com/product/laser-engraver-supremacy/spirit).
pub static BED_GCC_SPIRIT: LazyLock<PrintBed> = LazyLock::new(|| {
    PrintBed::new(
        (
            // actually -50.72 but the cutter refuses to move this far...
            0.0, 901.52,
        ),
        false,
        (
            // Again, actually -4.80 but 🤷.
            0.0, 463.20,
        ),
        true,
    )
});

const VALID_MM_RANGE: RangeInclusive<f32> =
    (i16::MIN as f32 * MM_PER_PLOTTER_UNIT)..=(i16::MAX as f32 * MM_PER_PLOTTER_UNIT);

impl PrintBed {
    /// Creates a new [`PrintBed`] specification.
    ///
    /// `x_axis` and `y_axis` are tuples of lower/upper limits of the bed in millimetres.
    /// They will be clamped to their HPGL-representable range.
    ///
    /// # Panics
    /// - When `x_axis` or `y_axis` aren't in order.
    /// - When `x_axis` or `y_axis` contain `Nan` or an infinity.
    pub fn new(
        mut x_axis: (f32, f32),
        mirror_x: bool,
        mut y_axis: (f32, f32),
        mirror_y: bool,
    ) -> Self {
        #[inline]
        fn validate(val: &mut f32) {
            assert!(val.is_finite(), "{val} is not a finite number");

            if !VALID_MM_RANGE.contains(&val) {
                let adjusted = val.clamp(*VALID_MM_RANGE.start(), *VALID_MM_RANGE.end());
                log::warn!(
                    "axis value {val} would produce invalid HPGL values, truncating to {adjusted}",
                );
                *val = adjusted
            }
        }

        assert!(
            x_axis.0 <= x_axis.1,
            "X axis values are the wrong way around"
        );
        assert!(
            y_axis.0 <= y_axis.1,
            "y axis values are the wrong way around"
        );

        validate(&mut x_axis.0);
        validate(&mut x_axis.1);
        validate(&mut y_axis.0);
        validate(&mut y_axis.1);

        Self {
            x_axis: x_axis.0..=x_axis.1,
            y_axis: y_axis.0..=y_axis.1,
            mirror_x,
            mirror_y,
        }
    }

    /// Converts a [`PointInMillimeters`] into the same point in HPGL/2 units **for this printer**.
    ///
    /// Returns `None` if the point is out of the bed of this printer.
    ///
    /// # Arguments
    /// * `point`: The point to convert from mm.
    ///
    /// # Panics
    /// When `point` contains a non-finite number.
    pub fn place_point(&self, point: PointInMillimeters) -> Option<ResolvedPoint> {
        #[inline]
        fn mm_to_hpgl(mut value: f32, mirror: Option<f32>) -> Option<i16> {
            // TODO: this isn't correct behaviour if self.x_axis.start() < 0
            if let Some(max) = mirror {
                value = max - value;
            }

            let adjusted = value / MM_PER_PLOTTER_UNIT;
            if !((i16::MIN as f32)..=(i16::MAX as f32)).contains(&adjusted) {
                // value would be truncated
                log::warn!(
                    "HPGL value {adjusted} from {value}mm is out of i16 range: {:?}",
                    (i16::MIN..=i16::MAX)
                );
                None
            } else {
                Some(adjusted.round() as i16)
            }
        }

        assert!(
            point.x.is_finite(),
            "point x value {} is not finite",
            point.x
        );
        assert!(
            point.y.is_finite(),
            "point y value {} is not finite",
            point.y
        );

        if !(self.x_axis.contains(&point.x)) {
            log::warn!(
                "x-axis value {}mm is outside of bed size {:?}",
                point.x,
                self.x_axis,
            );
            return None;
        }
        if !(self.y_axis.contains(&point.y)) {
            log::warn!(
                "y-axis value {}mm is outside of bed size {:?}",
                point.y,
                self.y_axis,
            );
            return None;
        }

        Some(ResolvedPoint {
            x: mm_to_hpgl(point.x, self.mirror_x.then_some(*self.x_axis.end()))?,
            y: mm_to_hpgl(point.y, self.mirror_y.then_some(*self.y_axis.end()))?,
        })
    }

    /// Gets the x axis value range of this print bed in millimetres.
    pub fn x_axis(&self) -> &RangeInclusive<f32> {
        &self.x_axis
    }

    /// Gets the y axis value range of this print bed in millimetres.
    pub fn y_axis(&self) -> &RangeInclusive<f32> {
        &self.y_axis
    }

    /// Gets the width of this print bed in millimetres.
    pub fn width(&self) -> f32 {
        self.x_axis.end() - self.x_axis.start()
    }

    /// Gets the height of this print bed in millimetres.
    pub fn height(&self) -> f32 {
        self.y_axis.end() - self.y_axis.start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_to_hpgl_units() {
        let bed = &BED_GCC_SPIRIT;

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
            bed.place_point((819.175, 462.0).into()).unwrap(),
            (32767, 48).into(),
            "bed maximum"
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
