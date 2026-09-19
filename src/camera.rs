use glam::{Mat4, Quat, Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionMode {
    Perspective,
    Orthographic,
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

pub struct Camera {
    pub target: Vec3,
    pub position: Vec3,
    pub viewport: Vec2,
    pub viewport_origin: Vec2,
    pub projection_mode: ProjectionMode,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            target: Vec3::ZERO,
            position: Vec3::new(-3.3, 0.8, 1.7),
            viewport: Vec2::ONE,
            viewport_origin: Vec2::ZERO,
            projection_mode: ProjectionMode::Perspective,
        }
    }

    pub fn view(&self) -> Mat4 {
        let offset = self.position - self.target;
        let up = if offset.x.abs() + offset.z.abs() < 0.0001 {
            Vec3::NEG_Z
        } else {
            Vec3::Y
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
        let offset = self.cursor_at_depth(cursor, center) - center;
        let inverse_view = self.view().inverse();
        offset
            .dot(inverse_view.y_axis.truncate())
            .atan2(offset.dot(inverse_view.x_axis.truncate()))
    }

    pub fn orbit(&mut self, delta: Vec2) {
        let mut offset = self.position - self.target;
        offset = Quat::from_rotation_y(-delta.x * 0.003) * offset;
        let pitch =
            Quat::from_axis_angle(self.view().inverse().x_axis.truncate(), -delta.y * 0.003)
                * offset;
        if pitch.normalize().dot(Vec3::Y).abs() < 0.995 {
            offset = pitch;
        }
        self.position = self.target + offset;
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
            ViewAngle::Isometric => Vec3::new(-1.0, 0.72, 1.0).normalize(),
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
