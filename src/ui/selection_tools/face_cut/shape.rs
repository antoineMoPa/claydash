use super::*;

pub(super) fn signed_area(vertices: &[Vec2]) -> f32 {
    vertices
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = vertices[(index + 1) % vertices.len()];
            point.x * next.y - next.x * point.y
        })
        .sum::<f32>()
        * 0.5
}

fn edge_side(a: Vec2, b: Vec2, point: Vec2) -> f32 {
    let edge = b - a;
    let relative = point - a;
    edge.x * relative.y - edge.y * relative.x
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab_c = edge_side(a, b, c);
    let ab_d = edge_side(a, b, d);
    let cd_a = edge_side(c, d, a);
    let cd_b = edge_side(c, d, b);
    if ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0 {
        return true;
    }
    let on_segment = |start: Vec2, end: Vec2, point: Vec2, side: f32| {
        side.abs() <= 0.000_001
            && point.x >= start.x.min(end.x) - 0.000_001
            && point.x <= start.x.max(end.x) + 0.000_001
            && point.y >= start.y.min(end.y) - 0.000_001
            && point.y <= start.y.max(end.y) + 0.000_001
    };
    on_segment(a, b, c, ab_c)
        || on_segment(a, b, d, ab_d)
        || on_segment(c, d, a, cd_a)
        || on_segment(c, d, b, cd_b)
}

pub(super) fn polygon_is_valid(vertices: &[Vec2]) -> bool {
    if vertices.len() < 3
        || signed_area(vertices).abs() < 0.000_01
        || vertices.iter().enumerate().any(|(index, point)| {
            point.distance_squared(vertices[(index + 1) % vertices.len()]) < 0.000_001
        })
    {
        return false;
    }
    for first in 0..vertices.len() {
        let first_next = (first + 1) % vertices.len();
        for second in (first + 1)..vertices.len() {
            let second_next = (second + 1) % vertices.len();
            if first == second
                || first_next == second
                || second_next == first
                || (first == 0 && second_next == 0)
            {
                continue;
            }
            if segments_intersect(
                vertices[first],
                vertices[first_next],
                vertices[second],
                vertices[second_next],
            ) {
                return false;
            }
        }
    }
    true
}

pub(super) fn create_face_shape(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    vertices: &[Vec2],
    depth: f32,
    id: uuid::Uuid,
) -> Option<SdfObject> {
    let (source, frame) = source_and_frame(scene, face)?;
    if !polygon_is_valid(vertices) {
        return None;
    }
    let minimum = vertices
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let maximum = vertices
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    let center = (minimum + maximum) * 0.5;
    let centered: Vec<_> = vertices.iter().map(|point| *point - center).collect();
    let signed_depth = if depth.abs() < 0.01 {
        0.01 * if depth < 0.0 { -1.0 } else { 1.0 }
    } else {
        depth
    };
    let local_origin = frame.origin
        + frame.u * center.x
        + frame.v * center.y
        + frame.normal * (signed_depth * 0.5);
    let basis = glam::Mat4::from_cols(
        frame.u.extend(0.0),
        frame.v.extend(0.0),
        frame.normal.extend(0.0),
        local_origin.extend(1.0),
    );
    let matrix = source.transform.matrix() * basis;
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let mut shape = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    shape.uuid = id;
    shape.name = if signed_depth < 0.0 {
        "Face cut".into()
    } else {
        "Face extrusion".into()
    };
    shape.transform = crate::model::Transform {
        translation,
        rotation,
        scale,
    };
    shape.params = SdfParams::PolygonPrismParams(crate::model::PolygonPrismParams {
        vertices: centered,
        half_depth: signed_depth.abs() * 0.5 + FACE_CUT_OVERLAP,
    });
    shape.color = source.color;
    shape.material = source.material;
    shape.material_id = source.material_id;
    shape.boolean_parent = Some(source.uuid);
    shape.operation = if signed_depth < 0.0 {
        BooleanOperation::Subtract
    } else {
        BooleanOperation::Union
    };
    Some(shape)
}
