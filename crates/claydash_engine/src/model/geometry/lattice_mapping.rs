//! Forward points consistent with the native four-step inverse cage evaluator.
use super::Lattice;
use glam::{Mat3, Vec3};
impl Lattice {
    pub fn deformed_point(&self, rest: Vec3) -> Result<Vec3, String> {
        self.deformed_point_with_offsets(rest, &self.effective_offsets())
    }
    pub(crate) fn native_rest_with_offsets(&self, point: Vec3, offsets: &[Vec3]) -> Vec3 {
        let mut rest = point;
        for _ in 0..4 {
            rest = point - self.displacement_with_offsets(rest, offsets);
        }
        rest
    }
    pub(crate) fn deformed_point_with_offsets(
        &self,
        rest: Vec3,
        offsets: &[Vec3],
    ) -> Result<Vec3, String> {
        let mut point = rest + self.displacement_with_offsets(rest, offsets);
        let epsilon = (self.max - self.min).min_element() * 1e-4;
        for _ in 0..12 {
            let error = rest - self.native_rest_with_offsets(point, offsets);
            if error.length() <= 2e-6 {
                if point.is_finite() {
                    return Ok(point);
                }
                break;
            }
            let column = |axis: Vec3| {
                (self.native_rest_with_offsets(point + axis * epsilon, offsets)
                    - self.native_rest_with_offsets(point - axis * epsilon, offsets))
                    / (2.0 * epsilon)
            };
            let jacobian = Mat3::from_cols(column(Vec3::X), column(Vec3::Y), column(Vec3::Z));
            if !jacobian.is_finite() || jacobian.determinant() <= 1e-8 {
                break;
            }
            let step = jacobian.inverse() * error;
            let before = error.length_squared();
            let mut accepted = false;
            for factor in [1.0, 0.5, 0.25, 0.125] {
                let candidate = point + step * factor;
                if (rest - self.native_rest_with_offsets(candidate, offsets)).length_squared()
                    < before
                {
                    point = candidate;
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                break;
            }
        }
        Err("Direct lattice cannot match native cage inversion to surface precision; reduce cage deformation".into())
    }
}
