//! The value and the derivative of one coordinate of a quadratic or cubic
//! Bézier curve, in `f32` and in `f64` (the callers keep their precision, so
//! that their results do not change): the one place for these polynomials.
//!
//! A curve is given by its control values: 3 for a quadratic, 4 for a
//! cubic, in Bernstein form. Other lengths give 0.

/// Makes the functions for one float type.
macro_rules! bezier {
    ($float:ty, $at:ident, $derivative:ident) => {
        /// The value at `t` of the curve with these control values.
        #[allow(clippy::many_single_char_names)] // The names of the polynomial.
        pub fn $at(v: &[$float], t: $float) -> $float {
            let u = 1.0 - t;
            match *v {
                [a, b, c] => u * u * a + 2.0 * u * t * b + t * t * c,
                [a, b, c, d] => {
                    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                }
                _ => 0.0,
            }
        }

        /// The derivative at `t` of the curve with these control values.
        #[allow(clippy::many_single_char_names)] // The names of the polynomial.
        pub fn $derivative(v: &[$float], t: $float) -> $float {
            let u = 1.0 - t;
            match *v {
                [a, b, c] => 2.0 * (u * (b - a) + t * (c - b)),
                [a, b, c, d] => 3.0 * (u * u * (b - a) + 2.0 * u * t * (c - b) + t * t * (d - c)),
                _ => 0.0,
            }
        }
    };
}

bezier!(f32, at_f32, derivative_f32);
bezier!(f64, at_f64, derivative_f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_reach_their_end_points() {
        assert_eq!(at_f32(&[1.0, 5.0, 9.0], 0.0), 1.0);
        assert_eq!(at_f64(&[1.0, 5.0, 9.0, 3.0], 1.0), 3.0);
        assert_eq!(at_f64(&[1.0, 2.0], 0.5), 0.0);
    }

    #[test]
    fn a_quadratic_at_the_middle() {
        // (a + 2 b + c) / 4.
        assert_eq!(at_f64(&[0.0, 8.0, 0.0], 0.5), 4.0);
        assert_eq!(derivative_f64(&[0.0, 8.0, 0.0], 0.0), 16.0);
    }

    #[test]
    fn a_cubic_at_the_middle() {
        // (a + 3 b + 3 c + d) / 8.
        assert_eq!(at_f32(&[0.0, 8.0, 8.0, 0.0], 0.5), 6.0);
        assert_eq!(derivative_f32(&[0.0, 8.0, 8.0, 0.0], 0.5), 0.0);
    }
}
