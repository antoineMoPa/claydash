use glam::Vec2;

/// Returns the center, outward normal, and length of a polygon prism's side edge.
pub fn polygon_side_frame(vertices: &[Vec2], edge: usize) -> Option<(Vec2, Vec2, f32)> {
    if vertices.len() < 3 {
        return None;
    }
    let a = *vertices.get(edge)?;
    let b = vertices[(edge + 1) % vertices.len()];
    let delta = b - a;
    let length = delta.length();
    if length <= 0.000_01 {
        return None;
    }
    let signed_area = vertices
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = vertices[(index + 1) % vertices.len()];
            point.x * next.y - next.x * point.y
        })
        .sum::<f32>();
    let outward =
        Vec2::new(delta.y, -delta.x) / length * if signed_area >= 0.0 { 1.0 } else { -1.0 };
    Some(((a + b) * 0.5, outward, length))
}
