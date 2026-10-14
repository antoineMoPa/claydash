//! Export error is measured in scene units, independently of camera distance.
use crate::model::SdfObject;

#[derive(Clone, Copy)]
pub struct TessellationQuality {
    pub position_error: f32,
    pub normal_angle: f32,
}
impl TessellationQuality {
    pub fn from_resolution(resolution: u32) -> Self {
        Self {
            position_error: 0.032 / resolution.clamp(8, 256) as f32,
            normal_angle: 15_f32.to_radians(),
        }
    }
    pub fn local(self, scene: &[SdfObject], object: &SdfObject) -> Self {
        let matrix = crate::model::object_world_matrix(scene, object.uuid);
        // Gershgorin bounds the largest eigenvalue of A^T A, including shear.
        // Unlike a Frobenius bound, an identity transform keeps local/world error equal.
        let axes = [
            matrix.x_axis.truncate(),
            matrix.y_axis.truncate(),
            matrix.z_axis.truncate(),
        ];
        let stretch = axes
            .iter()
            .map(|a| axes.iter().map(|b| a.dot(*b).abs()).sum::<f32>())
            .fold(0.0, f32::max)
            .sqrt()
            .max(1e-6);
        Self {
            position_error: self.position_error / stretch,
            ..self
        }
    }
    pub fn circle_segments(self, radius: f32) -> usize {
        let radius = radius.abs();
        if radius <= self.position_error {
            return 8;
        }
        let half_angle = (1.0 - self.position_error / radius).clamp(-1.0, 1.0).acos();
        let step = (2.0 * half_angle).min(self.normal_angle).max(1e-4);
        (std::f32::consts::TAU / step).ceil().clamp(8.0, 1024.0) as usize
    }
}
