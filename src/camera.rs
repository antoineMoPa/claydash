use glam::{Mat4, Quat, Vec2, Vec3};

pub struct Camera {
    pub target: Vec3,
    pub position: Vec3,
    pub viewport: Vec2,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            target: Vec3::ZERO,
            position: Vec3::new(-3.3, 0.8, 1.7),
            viewport: Vec2::ONE,
        }
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.position, self.target, Vec3::Y)
    }

    pub fn projection(&self) -> Mat4 {
        Mat4::perspective_rh(
            45_f32.to_radians(),
            (self.viewport.x / self.viewport.y).max(0.01),
            0.01,
            100.0,
        )
    }

    pub fn ray(&self, cursor: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec3::new(
            cursor.x / self.viewport.x * 2.0 - 1.0,
            1.0 - cursor.y / self.viewport.y * 2.0,
            1.0,
        );
        let point = (self.projection() * self.view())
            .inverse()
            .project_point3(ndc);
        (self.position, (point - self.position).normalize())
    }

    pub fn cursor_at_depth(&self, cursor: Vec2, world_position: Vec3) -> Vec3 {
        let (origin, direction) = self.ray(cursor);
        origin + direction * world_position.distance(origin)
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

    pub fn zoom(&mut self, amount: f32) {
        let offset = self.position - self.target;
        let radius = (offset.length() * (1.0 - amount * 0.2).max(0.01)).clamp(0.05, 1000.0);
        self.position = self.target + offset.normalize() * radius;
    }
}
