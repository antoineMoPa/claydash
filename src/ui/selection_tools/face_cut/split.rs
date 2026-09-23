use super::frame::FaceDomain;
use super::guides::face_point_inside;
use super::shape::{polygon_is_valid, signed_area};
use super::*;

pub(super) enum SplitRegions {
    Open([Vec<Vec2>; 2]),
    Closed { inner: Vec<Vec2>, outer: Vec<Vec2> },
}

impl SplitRegions {
    pub(super) fn polygon(&self, region: usize) -> &[Vec2] {
        match self {
            Self::Open(polygons) => &polygons[region],
            Self::Closed { inner, outer } => {
                if region == 0 {
                    inner
                } else {
                    outer
                }
            }
        }
    }

    pub(super) fn hole(&self, region: usize) -> Option<&[Vec2]> {
        match (self, region) {
            (Self::Closed { inner, .. }, 1) => Some(inner),
            _ => None,
        }
    }

    pub(super) fn region_at(&self, point: Vec2) -> usize {
        let first = self.polygon(0);
        if crate::model::polygon_distance(point, first) <= 0.0 {
            0
        } else {
            1
        }
    }
}

fn face_boundary(domain: &FaceDomain, path_length: usize) -> Vec<Vec2> {
    match domain {
        FaceDomain::Rectangle { extent_u, extent_v } => vec![
            Vec2::new(-extent_u, -extent_v),
            Vec2::new(*extent_u, -extent_v),
            Vec2::new(*extent_u, *extent_v),
            Vec2::new(-extent_u, *extent_v),
        ],
        FaceDomain::Polygon(vertices) => vertices.clone(),
        FaceDomain::Circle { radius } => {
            let sides = (crate::model::MAX_POLYGON_PRISM_VERTICES
                .saturating_sub(path_length.saturating_sub(2)))
            .clamp(8, 24);
            let outer_radius = radius / (std::f32::consts::PI / sides as f32).cos();
            (0..sides)
                .map(|index| {
                    let angle = (index as f32 + 0.5) * std::f32::consts::TAU / sides as f32;
                    Vec2::new(angle.cos(), angle.sin()) * outer_radius
                })
                .collect()
        }
    }
}

fn boundary_hit(boundary: &[Vec2], point: Vec2, tolerance: f32) -> Option<(usize, f32, Vec2)> {
    let mut best: Option<(f32, usize, f32, Vec2)> = None;
    for index in 0..boundary.len() {
        let a = boundary[index];
        let b = boundary[(index + 1) % boundary.len()];
        let edge = b - a;
        let length_squared = edge.length_squared();
        if length_squared < 0.000_001 {
            continue;
        }
        let t = ((point - a).dot(edge) / length_squared).clamp(0.0, 1.0);
        let projected = a + edge * t;
        let distance = point.distance(projected);
        if best.as_ref().is_none_or(|value| distance < value.0) {
            best = Some((distance, index, t, projected));
        }
    }
    best.filter(|value| value.0 <= tolerance)
        .map(|(_, index, t, point)| (index, t, point))
}

fn arc(boundary: &[Vec2], start: usize, end: usize) -> Vec<Vec2> {
    let mut result = vec![boundary[start]];
    let mut index = start;
    while index != end {
        index = (index + 1) % boundary.len();
        result.push(boundary[index]);
    }
    result
}

fn open_regions(domain: &FaceDomain, path: &[Vec2]) -> Option<SplitRegions> {
    if path.len() < 2 || path.len() > crate::model::MAX_POLYGON_PRISM_VERTICES {
        return None;
    }
    let boundary = face_boundary(domain, path.len());
    if !polygon_is_valid(&boundary) {
        return None;
    }
    let size = boundary
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max)
        - boundary
            .iter()
            .copied()
            .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let tolerance = size.max_element() * 0.015 + 0.001;
    let (start_edge, start_t, start) = boundary_hit(&boundary, path[0], tolerance)?;
    let (end_edge, end_t, end) = boundary_hit(&boundary, *path.last()?, tolerance)?;
    if start.distance_squared(end) < 0.000_001 {
        return None;
    }
    if path.iter().any(|point| !face_point_inside(domain, *point)) {
        return None;
    }
    for segment in path.windows(2) {
        for step in 1..5 {
            let midpoint = segment[0].lerp(segment[1], step as f32 / 5.0);
            if !face_point_inside(domain, midpoint) {
                return None;
            }
        }
    }
    let mut along = Vec::new();
    for edge in 0..boundary.len() {
        along.push(boundary[edge]);
        let mut inserts = Vec::new();
        if edge == start_edge && start_t > 0.000_01 && start_t < 0.999_99 {
            inserts.push((start_t, start));
        }
        if edge == end_edge && end_t > 0.000_01 && end_t < 0.999_99 {
            inserts.push((end_t, end));
        }
        inserts.sort_by(|a, b| a.0.total_cmp(&b.0));
        along.extend(inserts.into_iter().map(|(_, point)| point));
    }
    let start_index = along
        .iter()
        .position(|point| point.distance_squared(start) < 0.000_001)?;
    let end_index = along
        .iter()
        .position(|point| point.distance_squared(end) < 0.000_001)?;
    if start_index == end_index {
        return None;
    }
    let mut path = path.to_vec();
    path[0] = start;
    *path.last_mut()? = end;
    let mut first = arc(&along, start_index, end_index);
    first.extend(path[1..path.len() - 1].iter().rev().copied());
    let mut second = arc(&along, end_index, start_index);
    second.extend(path[1..path.len() - 1].iter().copied());
    let max = crate::model::MAX_POLYGON_PRISM_VERTICES;
    if first.len() > max
        || second.len() > max
        || !polygon_is_valid(&first)
        || !polygon_is_valid(&second)
    {
        return None;
    }
    let area = signed_area(&along).abs();
    if (signed_area(&first).abs() + signed_area(&second).abs() - area).abs() > area * 0.02 {
        return None;
    }
    Some(SplitRegions::Open([first, second]))
}

fn closed_regions(domain: &FaceDomain, path: &[Vec2]) -> Option<SplitRegions> {
    if !polygon_is_valid(path) {
        return None;
    }
    if path.iter().any(|point| !face_point_inside(domain, *point)) {
        return None;
    }
    for index in 0..path.len() {
        let a = path[index];
        let b = path[(index + 1) % path.len()];
        for step in 1..5 {
            if !face_point_inside(domain, a.lerp(b, step as f32 / 5.0)) {
                return None;
            }
        }
    }
    let outer = face_boundary(domain, path.len());
    if !polygon_is_valid(&outer) || outer.len() > crate::model::MAX_POLYGON_PRISM_VERTICES {
        return None;
    }
    Some(SplitRegions::Closed {
        inner: path.to_vec(),
        outer,
    })
}

pub(super) fn split_regions(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    path: &[Vec2],
    closed: bool,
) -> Option<SplitRegions> {
    let (_, frame) = source_and_frame(scene, face)?;
    if closed {
        closed_regions(&frame.domain, path)
    } else {
        open_regions(&frame.domain, path)
    }
}

pub(super) fn region_shapes(
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    regions: &SplitRegions,
    selected: usize,
    depth: f32,
    root_id: uuid::Uuid,
    hole_id: Option<uuid::Uuid>,
) -> Option<Vec<SdfObject>> {
    let root = create_face_shape(scene, face, regions.polygon(selected), depth, root_id)?;
    let mut shapes = vec![root];
    if let Some(hole) = regions.hole(selected) {
        let mut cutter = create_face_shape(scene, face, hole, depth, hole_id?)?;
        cutter.boolean_parent = Some(root_id);
        cutter.operation = BooleanOperation::Subtract;
        cutter.name = "Face split void".into();
        if let SdfParams::PolygonPrismParams(params) = &mut cutter.params {
            params.half_depth += FACE_CUT_OVERLAP * 2.0;
        }
        shapes.push(cutter);
    }
    Some(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_across_a_square_makes_two_equal_selectable_regions() {
        let source = SdfObject::create_kind(PrimitiveKind::Box);
        let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
            object: source.uuid,
            axis: VectorAxis::Z,
            positive: true,
        });
        let scene = [source];
        let regions = split_regions(
            &scene,
            face,
            &[Vec2::new(0.0, -0.3), Vec2::new(0.0, 0.3)],
            false,
        )
        .unwrap();
        assert_eq!(regions.region_at(Vec2::new(0.1, 0.0)), 0);
        assert_eq!(regions.region_at(Vec2::new(-0.1, 0.0)), 1);
        assert!((signed_area(regions.polygon(0)).abs() - 0.18).abs() < 0.0001);
        assert!((signed_area(regions.polygon(1)).abs() - 0.18).abs() < 0.0001);
        let shapes =
            region_shapes(&scene, face, &regions, 0, 0.1, uuid::Uuid::new_v4(), None).unwrap();
        let mut extruded_scene = scene.to_vec();
        extruded_scene.extend(shapes);
        assert!(
            crate::model::scene_sample(Vec3::new(0.1, 0.0, 0.35), &extruded_scene)
                .unwrap()
                .0
                < 0.0
        );
        assert!(
            crate::model::scene_sample(Vec3::new(-0.1, 0.0, 0.35), &extruded_scene)
                .unwrap()
                .0
                > 0.0
        );
        assert!(
            region_shapes(&scene, face, &regions, 1, -0.1, uuid::Uuid::new_v4(), None).is_some()
        );
    }

    #[test]
    fn a_closed_outline_offers_inner_and_surrounding_regions() {
        let source = SdfObject::create_kind(PrimitiveKind::Box);
        let face = crate::model::ModelingFaceSelection::Box(crate::model::BoxFaceSelection {
            object: source.uuid,
            axis: VectorAxis::Z,
            positive: true,
        });
        let scene = [source];
        let regions = split_regions(
            &scene,
            face,
            &[
                Vec2::new(-0.1, -0.1),
                Vec2::new(0.1, -0.1),
                Vec2::new(0.1, 0.1),
                Vec2::new(-0.1, 0.1),
            ],
            true,
        )
        .unwrap();
        assert_eq!(regions.region_at(Vec2::ZERO), 0);
        assert_eq!(regions.region_at(Vec2::new(0.2, 0.2)), 1);
        let root = uuid::Uuid::new_v4();
        let shapes = region_shapes(
            &scene,
            face,
            &regions,
            1,
            0.1,
            root,
            Some(uuid::Uuid::new_v4()),
        )
        .unwrap();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[1].boolean_parent, Some(root));
        assert_eq!(shapes[1].operation, BooleanOperation::Subtract);
        let mut extruded_scene = scene.to_vec();
        extruded_scene.extend(shapes);
        assert!(
            crate::model::scene_sample(Vec3::new(0.2, 0.0, 0.35), &extruded_scene)
                .unwrap()
                .0
                < 0.0
        );
        assert!(
            crate::model::scene_sample(Vec3::new(0.0, 0.0, 0.35), &extruded_scene)
                .unwrap()
                .0
                > 0.0
        );
    }

    #[test]
    fn a_cylinder_cap_can_split_across_its_rim() {
        let source = SdfObject::create_kind(PrimitiveKind::Cylinder);
        let face =
            crate::model::ModelingFaceSelection::CylinderCap(crate::model::CylinderCapSelection {
                object: source.uuid,
                positive: true,
            });
        let scene = [source];
        let regions = split_regions(
            &scene,
            face,
            &[Vec2::new(0.0, -0.25), Vec2::new(0.0, 0.25)],
            false,
        )
        .unwrap();
        assert_ne!(
            regions.region_at(Vec2::new(-0.1, 0.0)),
            regions.region_at(Vec2::new(0.1, 0.0))
        );
    }
}
