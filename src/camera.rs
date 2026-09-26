use glam::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

const DEFAULT_CAMERA_DISTANCE: f32 = 3.8;
// The showcase view: 34 degrees around Y and 15 degrees above the ground plane.
const ISOMETRIC_DIRECTION: Vec3 = Vec3::new(0.540_138_84, 0.258_819_04, 0.800_788_8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionMode {
    Perspective,
    Orthographic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCamera {
    pub uuid: uuid::Uuid,
    pub name: String,
    #[serde(default)]
    pub transform: crate::model::Transform,
    #[serde(default = "default_focal_distance")]
    pub focal_distance: f32,
    pub projection_mode: ProjectionMode,
}

fn default_focal_distance() -> f32 {
    DEFAULT_CAMERA_DISTANCE
}

impl SceneCamera {
    pub fn from_view(name: impl Into<String>, camera: &Camera) -> Self {
        let offset = camera.target - camera.position;
        let distance = offset.length().max(0.0001);
        let (_, rotation, _) = camera.view().inverse().to_scale_rotation_translation();
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: name.into(),
            transform: crate::model::Transform {
                translation: camera.position,
                rotation,
                scale: Vec3::ONE,
            },
            focal_distance: distance,
            projection_mode: camera.projection_mode,
        }
    }

    pub fn apply_to_view(&self, camera: &mut Camera) {
        camera.position = self.transform.translation;
        camera.target = self.target();
        camera.up = self.transform.rotation * Vec3::Y;
        camera.projection_mode = self.projection_mode;
    }

    pub fn target(&self) -> Vec3 {
        self.transform.translation
            + self.transform.rotation * Vec3::NEG_Z * self.focal_distance.max(0.01)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_axis_views_produce_finite_pick_rays() {
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(640.0, 480.0);
        camera.viewport_origin = Vec2::new(230.0, 50.0);
        for angle in [
            ViewAngle::Front,
            ViewAngle::Back,
            ViewAngle::Left,
            ViewAngle::Right,
            ViewAngle::Top,
            ViewAngle::Bottom,
        ] {
            camera.snap(angle);
            for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
                camera.projection_mode = mode;
                let (_, direction) = camera.ray(camera.viewport_origin + camera.viewport * 0.5);
                assert!(direction.is_finite());
                assert!((camera.target - camera.position).cross(direction).length() < 0.001);
            }
        }
    }

    #[test]
    fn referenced_pan_keeps_world_point_under_cursor() {
        for mode in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
            let mut camera = Camera::new();
            camera.position = Vec3::new(0.0, 0.0, 8.0);
            camera.viewport = Vec2::new(800.0, 600.0);
            camera.projection_mode = mode;
            let reference = Vec3::new(0.0, 0.0, 0.5);
            let cursor = Vec2::new(510.0, 365.0);

            camera.pan_to_cursor(cursor, reference);

            let projected = camera.project(reference, 1.0).unwrap();
            assert!((projected.x - cursor.x).abs() < 0.001);
            assert!((projected.y - cursor.y).abs() < 0.001);
        }
    }

    #[test]
    fn default_and_isometric_snap_use_the_showcase_view() {
        let mut camera = Camera::new();
        let default_offset = camera.position - camera.target;
        assert!((default_offset.length() - DEFAULT_CAMERA_DISTANCE).abs() < 0.0001);
        assert!((default_offset / default_offset.length()).distance(ISOMETRIC_DIRECTION) < 0.0001);

        camera.position = Vec3::new(-7.0, 2.0, 4.0);
        let snap_distance = (camera.position - camera.target).length();
        camera.snap(ViewAngle::Isometric);
        let snapped_offset = camera.position - camera.target;
        assert!((snapped_offset.length() - snap_distance).abs() < 0.0001);
        assert!((snapped_offset / snapped_offset.length()).distance(ISOMETRIC_DIRECTION) < 0.0001);
    }

    #[test]
    fn zoom_amounts_match_scroll_and_trackpad_pinch_direction() {
        let mut camera = Camera::new();
        let initial = camera.position.distance(camera.target);
        camera.zoom(0.5);
        assert!(camera.position.distance(camera.target) < initial);
        camera.zoom(-0.5);
        assert!(camera.position.distance(camera.target) > initial * 0.9);
    }

    #[test]
    fn quaternion_orbit_crosses_camera_poles_without_a_singularity() {
        let mut camera = Camera::new();
        let radius = camera.position.distance(camera.target);
        for _ in 0..1_000 {
            camera.orbit(Vec2::new(1.7, 3.1));
            let view = camera.view();
            assert!(camera.position.is_finite());
            assert!(camera.up.is_finite());
            assert!(view.is_finite());
            assert!((camera.position.distance(camera.target) - radius).abs() < 0.001);
            assert!((camera.up.length() - 1.0).abs() < 0.001);
            assert!(
                camera
                    .up
                    .dot((camera.target - camera.position).normalize())
                    .abs()
                    < 0.001
            );
        }
    }

    #[test]
    fn scene_camera_view_preserves_roll() {
        let rotation = Quat::from_rotation_y(0.4) * Quat::from_rotation_z(0.7);
        let scene_camera = SceneCamera {
            uuid: uuid::Uuid::new_v4(),
            name: "Rolled camera".into(),
            transform: crate::model::Transform {
                translation: Vec3::new(2.0, 1.0, 5.0),
                rotation,
                scale: Vec3::ONE,
            },
            focal_distance: 4.0,
            projection_mode: ProjectionMode::Perspective,
        };
        let mut viewport_camera = Camera::new();

        scene_camera.apply_to_view(&mut viewport_camera);

        let view_rotation = viewport_camera
            .view()
            .inverse()
            .to_scale_rotation_translation()
            .1;
        assert!((view_rotation * Vec3::Y).distance(rotation * Vec3::Y) < 0.0001);
        assert!((view_rotation * Vec3::NEG_Z).distance(rotation * Vec3::NEG_Z) < 0.0001);

        let captured = SceneCamera::from_view("Captured", &viewport_camera);
        assert!((captured.transform.rotation * Vec3::Y).distance(rotation * Vec3::Y) < 0.0001);
    }
}

impl ProjectionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Perspective => "Perspective",
            Self::Orthographic => "Orthographic",
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ViewAngle {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Isometric,
}

#[derive(Clone)]
pub struct Camera {
    pub target: Vec3,
    pub position: Vec3,
    pub viewport: Vec2,
    pub viewport_origin: Vec2,
    pub up: Vec3,
    pub projection_mode: ProjectionMode,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            target: Vec3::ZERO,
            position: ISOMETRIC_DIRECTION * DEFAULT_CAMERA_DISTANCE,
            viewport: Vec2::ONE,
            viewport_origin: Vec2::ZERO,
            up: Vec3::Y,
            projection_mode: ProjectionMode::Perspective,
        }
    }

    pub fn view(&self) -> Mat4 {
        let offset = self.position - self.target;
        let forward = -offset.normalize_or_zero();
        let up = if self.up.cross(forward).length_squared() < 0.0001 {
            Vec3::NEG_Z
        } else {
            self.up.normalize()
        };
        Mat4::look_at_rh(self.position, self.target, up)
    }

    pub fn projection(&self) -> Mat4 {
        let aspect = (self.viewport.x / self.viewport.y).max(0.01);
        match self.projection_mode {
            ProjectionMode::Perspective => {
                Mat4::perspective_rh(45_f32.to_radians(), aspect, 0.01, 100.0)
            }
            ProjectionMode::Orthographic => {
                let half_height = (self.position - self.target).length() * 0.42;
                Mat4::orthographic_rh(
                    -half_height * aspect,
                    half_height * aspect,
                    -half_height,
                    half_height,
                    0.01,
                    100.0,
                )
            }
        }
    }

    pub fn ray(&self, cursor: Vec2) -> (Vec3, Vec3) {
        let cursor = cursor - self.viewport_origin;
        let ndc = Vec3::new(
            cursor.x / self.viewport.x * 2.0 - 1.0,
            1.0 - cursor.y / self.viewport.y * 2.0,
            1.0,
        );
        let inverse = (self.projection() * self.view()).inverse();
        let far = inverse.project_point3(ndc);
        match self.projection_mode {
            ProjectionMode::Perspective => (self.position, (far - self.position).normalize()),
            ProjectionMode::Orthographic => {
                let near = inverse.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
                (near, (far - near).normalize())
            }
        }
    }

    pub fn cursor_at_depth(&self, cursor: Vec2, world_position: Vec3) -> Vec3 {
        let (origin, direction) = self.ray(cursor);
        origin + direction * world_position.distance(origin)
    }

    pub fn cursor_on_plane(&self, cursor: Vec2, world_position: Vec3) -> Vec3 {
        let (origin, direction) = self.ray(cursor);
        let normal = self.target - self.position;
        let denominator = direction.dot(normal);
        if denominator.abs() < 1e-6 {
            return world_position;
        }
        origin + direction * ((world_position - origin).dot(normal) / denominator)
    }

    pub fn cursor_angle(&self, cursor: Vec2, center: Vec3) -> f32 {
        let Some(projected) = self.project(center, 1.0) else {
            return 0.0;
        };
        let offset = cursor - Vec2::new(projected.x, projected.y);
        (-offset.y).atan2(offset.x)
    }

    pub fn orbit(&mut self, delta: Vec2) {
        let offset = self.position - self.target;
        let forward = -offset.normalize_or_zero();
        let up = self.up.normalize_or_zero();
        let right = forward.cross(up).normalize_or_zero();
        if right.length_squared() < 0.0001 {
            return;
        }

        // Rotate the complete camera frame. Keeping `up` in the quaternion
        // rotation removes the pole singularity of yaw/pitch Euler cameras and
        // lets an orbit pass smoothly over the top or bottom of the subject.
        let yaw = Quat::from_axis_angle(up, -delta.x * 0.003);
        let pitch = Quat::from_axis_angle(right, -delta.y * 0.003);
        let rotation = yaw * pitch;
        let rotated_offset = rotation * offset;
        let rotated_up = rotation * up;
        let rotated_forward = -rotated_offset.normalize_or_zero();
        let orthogonal_right = rotated_forward.cross(rotated_up).normalize_or_zero();
        self.position = self.target + rotated_offset;
        self.up = orthogonal_right.cross(rotated_forward).normalize_or_zero();
    }

    pub fn pan(&mut self, delta: Vec2) {
        let radius = (self.position - self.target).length();
        let inverse = self.view().inverse();
        let movement = (inverse.x_axis.truncate() * -delta.x + inverse.y_axis.truncate() * delta.y)
            * radius
            * 0.001;
        self.target += movement;
        self.position += movement;
    }

    pub fn pan_to_cursor(&mut self, cursor: Vec2, reference: Vec3) {
        let movement = reference - self.cursor_on_plane(cursor, reference);
        self.target += movement;
        self.position += movement;
    }

    pub fn zoom(&mut self, amount: f32) {
        let offset = self.position - self.target;
        let radius = (offset.length() * (1.0 - amount * 0.2).max(0.01)).clamp(0.05, 1000.0);
        self.position = self.target + offset.normalize() * radius;
    }

    pub fn toggle_projection(&mut self) {
        self.projection_mode = match self.projection_mode {
            ProjectionMode::Perspective => ProjectionMode::Orthographic,
            ProjectionMode::Orthographic => ProjectionMode::Perspective,
        };
    }

    pub fn snap(&mut self, angle: ViewAngle) {
        let radius = (self.position - self.target).length().max(0.05);
        let direction = match angle {
            ViewAngle::Front => Vec3::Z,
            ViewAngle::Back => Vec3::NEG_Z,
            ViewAngle::Left => Vec3::NEG_X,
            ViewAngle::Right => Vec3::X,
            ViewAngle::Top => Vec3::Y,
            ViewAngle::Bottom => Vec3::NEG_Y,
            ViewAngle::Isometric => ISOMETRIC_DIRECTION,
        };
        self.up = match angle {
            ViewAngle::Top | ViewAngle::Bottom => Vec3::NEG_Z,
            ViewAngle::Front
            | ViewAngle::Back
            | ViewAngle::Left
            | ViewAngle::Right
            | ViewAngle::Isometric => Vec3::Y,
        };
        self.position = self.target + direction * radius;
    }

    pub fn project(&self, world: Vec3, pixels_per_point: f32) -> Option<egui::Pos2> {
        let clip = self.projection() * self.view() * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(egui::pos2(
            (self.viewport_origin.x + (ndc.x * 0.5 + 0.5) * self.viewport.x) / pixels_per_point,
            (self.viewport_origin.y + (0.5 - ndc.y * 0.5) * self.viewport.y) / pixels_per_point,
        ))
    }
}
