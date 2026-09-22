use glam::{Vec2, Vec3};

use crate::{
    camera::Camera,
    model::{object_world_matrix, SdfObject, SdfParams, VectorAxis},
};

pub const SNAP_DISTANCE: f32 = 8.0;
pub const SNAP_RELEASE_DISTANCE: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceGuideDirection {
    Normal,
    TangentU,
    TangentV,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceGuideId {
    pub object: uuid::Uuid,
    pub face_axis: VectorAxis,
    pub positive: bool,
    pub direction: FaceGuideDirection,
}

#[derive(Clone, Copy, Debug)]
pub struct FaceGuide {
    pub id: FaceGuideId,
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct ActiveGuide {
    pub guide: FaceGuide,
    pub anchor: Vec3,
    pub anchor_index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ActiveGuideSet {
    pub primary: ActiveGuide,
    pub secondary: Option<ActiveGuide>,
}

impl ActiveGuideSet {
    pub fn iter(self) -> impl Iterator<Item = ActiveGuide> {
        [Some(self.primary), self.secondary].into_iter().flatten()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GuideSnap {
    pub correction: Vec3,
    pub active: ActiveGuideSet,
}

#[derive(Clone, Copy, Debug)]
struct SingleGuideSnap {
    correction: Vec3,
    active: ActiveGuide,
}

pub fn face_center_guides(scene: &[SdfObject], excluded: &[uuid::Uuid]) -> Vec<FaceGuide> {
    let mut guides = Vec::new();
    for object in scene {
        if excluded.contains(&object.uuid) {
            continue;
        }
        let SdfParams::BoxParams(params) = &object.params else {
            continue;
        };
        let matrix = object_world_matrix(scene, object.uuid);
        for face_axis in VectorAxis::ALL {
            let axis = face_axis.index();
            let tangent_u = (axis + 1) % 3;
            let tangent_v = (axis + 2) % 3;
            for positive in [false, true] {
                let sign = if positive { 1.0 } else { -1.0 };
                let mut local_center = Vec3::ZERO;
                local_center[axis] = params.box_q[axis] * sign;
                let origin = matrix.transform_point3(local_center);
                for (direction_kind, direction_axis) in [
                    (FaceGuideDirection::Normal, axis),
                    (FaceGuideDirection::TangentU, tangent_u),
                    (FaceGuideDirection::TangentV, tangent_v),
                ] {
                    let mut local_direction = Vec3::ZERO;
                    local_direction[direction_axis] = 1.0;
                    if direction_kind == FaceGuideDirection::Normal {
                        local_direction *= sign;
                    }
                    let direction = matrix
                        .transform_vector3(local_direction)
                        .normalize_or_zero();
                    if direction.length_squared() < 0.0001 {
                        continue;
                    }
                    guides.push(FaceGuide {
                        id: FaceGuideId {
                            object: object.uuid,
                            face_axis,
                            positive,
                            direction: direction_kind,
                        },
                        origin,
                        direction,
                    });
                }
            }
        }
    }
    guides
}

pub fn box_face_centers(object: &SdfObject, matrix: glam::Mat4) -> Vec<Vec3> {
    let mut centers = vec![matrix.transform_point3(Vec3::ZERO)];
    let SdfParams::BoxParams(params) = &object.params else {
        return centers;
    };
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut local = Vec3::ZERO;
            local[axis] = params.box_q[axis] * sign;
            centers.push(matrix.transform_point3(local));
        }
    }
    centers
}

pub fn object_anchors(scene: &[SdfObject], object_ids: &[uuid::Uuid]) -> Vec<Vec3> {
    let mut anchors = Vec::new();
    for id in object_ids {
        let Some(object) = scene.iter().find(|object| object.uuid == *id) else {
            continue;
        };
        anchors.extend(box_face_centers(
            object,
            object_world_matrix(scene, object.uuid),
        ));
    }
    anchors
}

pub fn snap_in_view_plane(
    camera: &Camera,
    raw_anchors: &[Vec3],
    guides: &[FaceGuide],
    pixels_per_point: f32,
    previous: Option<ActiveGuideSet>,
) -> Option<GuideSnap> {
    let primary = choose_snap(
        raw_anchors,
        guides,
        previous.map(|active| active.primary),
        |anchor, guide| {
            let raw_screen = camera.project(anchor, pixels_per_point)?;
            let (guide_origin, guide_direction) = projected_line(camera, guide, pixels_per_point)?;
            let snapped_screen =
                closest_point_on_screen_line(raw_screen, guide_origin, guide_direction);
            let distance = raw_screen.distance(snapped_screen);
            let physical = Vec2::new(snapped_screen.x, snapped_screen.y) * pixels_per_point;
            let snapped_anchor = camera.cursor_on_plane(physical, anchor);
            Some((distance, snapped_anchor - anchor, snapped_anchor))
        },
    )?;

    let shifted_anchors: Vec<_> = raw_anchors
        .iter()
        .map(|anchor| *anchor + primary.correction)
        .collect();
    let remaining_guides: Vec<_> = guides
        .iter()
        .copied()
        .filter(|guide| guide.id.object != primary.active.guide.id.object)
        .collect();
    let secondary =
        guide_follow_direction(camera, primary.active, pixels_per_point).and_then(|direction| {
            choose_line_snap(
                camera,
                &shifted_anchors,
                direction,
                &remaining_guides,
                pixels_per_point,
                previous.and_then(|active| active.secondary),
            )
        });

    if let Some(secondary) = secondary {
        let mut primary_active = primary.active;
        primary_active.anchor += secondary.correction;
        Some(GuideSnap {
            correction: primary.correction + secondary.correction,
            active: ActiveGuideSet {
                primary: primary_active,
                secondary: Some(secondary.active),
            },
        })
    } else {
        Some(GuideSnap {
            correction: primary.correction,
            active: ActiveGuideSet {
                primary: primary.active,
                secondary: None,
            },
        })
    }
}

pub fn snap_along_line(
    camera: &Camera,
    raw_anchors: &[Vec3],
    motion_direction: Vec3,
    guides: &[FaceGuide],
    pixels_per_point: f32,
    previous: Option<ActiveGuideSet>,
) -> Option<GuideSnap> {
    let snap = choose_line_snap(
        camera,
        raw_anchors,
        motion_direction,
        guides,
        pixels_per_point,
        previous.map(|active| active.primary),
    )?;
    Some(GuideSnap {
        correction: snap.correction,
        active: ActiveGuideSet {
            primary: snap.active,
            secondary: None,
        },
    })
}

fn choose_line_snap(
    camera: &Camera,
    raw_anchors: &[Vec3],
    motion_direction: Vec3,
    guides: &[FaceGuide],
    pixels_per_point: f32,
    previous: Option<ActiveGuide>,
) -> Option<SingleGuideSnap> {
    let motion_direction = motion_direction.normalize_or_zero();
    if motion_direction.length_squared() < 0.0001 {
        return None;
    }
    choose_snap(raw_anchors, guides, previous, |anchor, guide| {
        let raw_screen = camera.project(anchor, pixels_per_point)?;
        let ahead = camera.project(anchor + motion_direction, pixels_per_point)?;
        let motion_screen_direction = ahead - raw_screen;
        if motion_screen_direction.length_sq() < 0.0001 {
            return None;
        }
        let (guide_origin, guide_direction) = projected_line(camera, guide, pixels_per_point)?;
        let intersection = screen_line_intersection(
            raw_screen,
            motion_screen_direction,
            guide_origin,
            guide_direction,
        )?;
        let distance = raw_screen.distance(intersection);
        let physical = Vec2::new(intersection.x, intersection.y) * pixels_per_point;
        let (ray_origin, ray_direction) = camera.ray(physical);
        let closest_anchor =
            closest_point_between_lines(anchor, motion_direction, ray_origin, ray_direction)?;
        let correction = motion_direction * (closest_anchor - anchor).dot(motion_direction);
        let snapped_anchor = anchor + correction;
        Some((distance, correction, snapped_anchor))
    })
}

fn choose_snap(
    raw_anchors: &[Vec3],
    guides: &[FaceGuide],
    previous: Option<ActiveGuide>,
    mut candidate: impl FnMut(Vec3, FaceGuide) -> Option<(f32, Vec3, Vec3)>,
) -> Option<SingleGuideSnap> {
    if let Some(previous) = previous {
        if let (Some(anchor), Some(guide)) = (
            raw_anchors.get(previous.anchor_index).copied(),
            guides
                .iter()
                .copied()
                .find(|guide| guide.id == previous.guide.id),
        ) {
            if let Some((distance, correction, snapped_anchor)) = candidate(anchor, guide) {
                if distance <= SNAP_RELEASE_DISTANCE {
                    let (correction, snapped_anchor) =
                        stable_correction(anchor, correction, snapped_anchor);
                    return Some(SingleGuideSnap {
                        correction,
                        active: ActiveGuide {
                            guide,
                            anchor: snapped_anchor,
                            anchor_index: previous.anchor_index,
                        },
                    });
                }
            }
        }
    }

    let mut best: Option<(f32, SingleGuideSnap)> = None;
    for (anchor_index, anchor) in raw_anchors.iter().copied().enumerate() {
        for guide in guides.iter().copied() {
            let Some((distance, correction, snapped_anchor)) = candidate(anchor, guide) else {
                continue;
            };
            if distance > SNAP_DISTANCE || best.is_some_and(|(score, _)| score <= distance) {
                continue;
            }
            let (correction, snapped_anchor) =
                stable_correction(anchor, correction, snapped_anchor);
            best = Some((
                distance,
                SingleGuideSnap {
                    correction,
                    active: ActiveGuide {
                        guide,
                        anchor: snapped_anchor,
                        anchor_index,
                    },
                },
            ));
        }
    }
    best.map(|(_, snap)| snap)
}

fn guide_follow_direction(
    camera: &Camera,
    active: ActiveGuide,
    pixels_per_point: f32,
) -> Option<Vec3> {
    let anchor_screen = camera.project(active.anchor, pixels_per_point)?;
    let (_, screen_direction) = projected_line(camera, active.guide, pixels_per_point)?;
    let screen_direction = screen_direction.normalized();
    if screen_direction.length_sq() < 0.0001 {
        return None;
    }
    let next_screen = anchor_screen + screen_direction * 10.0;
    let physical = Vec2::new(next_screen.x, next_screen.y) * pixels_per_point;
    let next_world = camera.cursor_on_plane(physical, active.anchor);
    let direction = (next_world - active.anchor).normalize_or_zero();
    (direction.length_squared() >= 0.0001).then_some(direction)
}

fn stable_correction(anchor: Vec3, correction: Vec3, snapped_anchor: Vec3) -> (Vec3, Vec3) {
    if correction.length_squared() < 0.0001 * 0.0001 {
        (Vec3::ZERO, anchor)
    } else {
        (correction, snapped_anchor)
    }
}

fn projected_line(
    camera: &Camera,
    guide: FaceGuide,
    pixels_per_point: f32,
) -> Option<(egui::Pos2, egui::Vec2)> {
    let origin = camera.project(guide.origin, pixels_per_point)?;
    let ahead = camera.project(guide.origin + guide.direction, pixels_per_point)?;
    let direction = ahead - origin;
    (direction.length_sq() >= 0.0001).then_some((origin, direction))
}

fn closest_point_on_screen_line(
    point: egui::Pos2,
    origin: egui::Pos2,
    direction: egui::Vec2,
) -> egui::Pos2 {
    origin + direction * (point - origin).dot(direction) / direction.length_sq()
}

fn screen_line_intersection(
    a_origin: egui::Pos2,
    a_direction: egui::Vec2,
    b_origin: egui::Pos2,
    b_direction: egui::Vec2,
) -> Option<egui::Pos2> {
    let cross = a_direction.x * b_direction.y - a_direction.y * b_direction.x;
    if cross.abs() < 0.0001 {
        return None;
    }
    let offset = b_origin - a_origin;
    let amount = (offset.x * b_direction.y - offset.y * b_direction.x) / cross;
    Some(a_origin + a_direction * amount)
}

fn closest_point_between_lines(
    line_origin: Vec3,
    line_direction: Vec3,
    ray_origin: Vec3,
    ray_direction: Vec3,
) -> Option<Vec3> {
    let offset = line_origin - ray_origin;
    let a = line_direction.length_squared();
    let b = line_direction.dot(ray_direction);
    let c = ray_direction.length_squared();
    let d = line_direction.dot(offset);
    let e = ray_direction.dot(offset);
    let denominator = a * c - b * b;
    if denominator.abs() < 0.000001 {
        return None;
    }
    let amount = (b * e - c * d) / denominator;
    Some(line_origin + line_direction * amount)
}

pub fn screen_segment(
    camera: &Camera,
    guide: FaceGuide,
    pixels_per_point: f32,
    rect: egui::Rect,
) -> Option<[egui::Pos2; 2]> {
    let (origin, direction) = projected_line(camera, guide, pixels_per_point)?;
    let mut points = Vec::new();
    if direction.x.abs() > 0.0001 {
        for x in [rect.left(), rect.right()] {
            let amount = (x - origin.x) / direction.x;
            let point = origin + direction * amount;
            if point.y >= rect.top() - 0.1 && point.y <= rect.bottom() + 0.1 {
                points.push(point);
            }
        }
    }
    if direction.y.abs() > 0.0001 {
        for y in [rect.top(), rect.bottom()] {
            let amount = (y - origin.y) / direction.y;
            let point = origin + direction * amount;
            if point.x >= rect.left() - 0.1 && point.x <= rect.right() + 0.1 {
                points.push(point);
            }
        }
    }
    points.dedup_by(|a, b| a.distance_sq(*b) < 0.01);
    (points.len() >= 2).then_some([points[0], points[1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{ProjectionMode, ViewAngle};
    use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

    fn front_camera() -> Camera {
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        camera.projection_mode = ProjectionMode::Orthographic;
        camera.snap(ViewAngle::Front);
        camera
    }

    fn vertical_guide() -> FaceGuide {
        FaceGuide {
            id: FaceGuideId {
                object: uuid::Uuid::nil(),
                face_axis: VectorAxis::X,
                positive: true,
                direction: FaceGuideDirection::TangentU,
            },
            origin: Vec3::ZERO,
            direction: Vec3::Y,
        }
    }

    fn horizontal_guide() -> FaceGuide {
        FaceGuide {
            id: FaceGuideId {
                object: uuid::Uuid::from_u128(1),
                face_axis: VectorAxis::Y,
                positive: true,
                direction: FaceGuideDirection::TangentU,
            },
            origin: Vec3::ZERO,
            direction: Vec3::X,
        }
    }

    #[test]
    fn boxes_emit_face_center_frames_and_other_primitives_do_not() {
        let mut box_object = SdfObject::create(TYPE_BOX);
        box_object.transform.translation = Vec3::new(2.0, 3.0, 4.0);
        box_object.transform.rotation = glam::Quat::from_rotation_y(0.4);
        let sphere = SdfObject::create(TYPE_SPHERE);
        let guides = face_center_guides(&[box_object.clone(), sphere], &[]);
        assert_eq!(guides.len(), 18);
        let positive_x = guides.iter().find(|guide| {
            guide.id.face_axis == VectorAxis::X
                && guide.id.positive
                && guide.id.direction == FaceGuideDirection::Normal
        });
        let positive_x = positive_x.expect("positive X normal guide");
        let expected = box_object
            .transform
            .matrix()
            .transform_point3(Vec3::X * 0.3);
        assert!(positive_x.origin.distance(expected) < 0.0001);
    }

    #[test]
    fn excluded_boxes_do_not_emit_guides() {
        let object = SdfObject::create(TYPE_BOX);
        assert!(face_center_guides(std::slice::from_ref(&object), &[object.uuid]).is_empty());
    }

    #[test]
    fn group_object_anchors_use_each_descendants_complete_world_transform() {
        let mut root = SdfObject::create(TYPE_BOX);
        root.group_transform.translation = Vec3::new(2.0, 0.0, 0.0);
        let mut child = SdfObject::create(TYPE_BOX);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::new(0.0, 3.0, 0.0);
        let scene = vec![root.clone(), child.clone()];
        let anchors = object_anchors(&scene, &[root.uuid, child.uuid]);
        assert_eq!(anchors.len(), 14);
        assert!(anchors.iter().any(|anchor| {
            anchor.distance(object_world_matrix(&scene, child.uuid).transform_point3(Vec3::ZERO))
                < 0.0001
        }));
    }

    #[test]
    fn view_plane_snap_places_the_anchor_on_the_projected_guide() {
        let camera = front_camera();
        let raw_anchor = Vec3::new(0.02, 0.4, 0.0);
        let snap = snap_in_view_plane(&camera, &[raw_anchor], &[vertical_guide()], 1.0, None)
            .expect("anchor is within the acquisition distance");
        let snapped = raw_anchor + snap.correction;
        assert!(snapped.x.abs() < 0.0001);
        assert!((snapped.y - raw_anchor.y).abs() < 0.0001);
        assert!(snapped.z.abs() < 0.0001);
    }

    #[test]
    fn axis_snap_changes_only_the_allowed_axis() {
        let camera = front_camera();
        let raw_anchor = Vec3::new(0.02, 0.4, 0.0);
        let snap = snap_along_line(
            &camera,
            &[raw_anchor],
            Vec3::X,
            &[vertical_guide()],
            1.0,
            None,
        )
        .expect("motion line crosses a nearby guide");
        assert!(snap.correction.x.abs() > 0.001);
        assert_eq!(snap.correction.y, 0.0);
        assert_eq!(snap.correction.z, 0.0);
        assert!((raw_anchor + snap.correction).x.abs() < 0.0001);
    }

    #[test]
    fn latched_guide_uses_the_larger_release_distance() {
        let camera = front_camera();
        let raw_anchor = Vec3::new(0.05, 0.4, 0.0);
        let guide = vertical_guide();
        assert!(snap_in_view_plane(&camera, &[raw_anchor], &[guide], 1.0, None).is_none());
        let previous = ActiveGuideSet {
            primary: ActiveGuide {
                guide,
                anchor: Vec3::new(0.0, 0.4, 0.0),
                anchor_index: 0,
            },
            secondary: None,
        };
        assert!(
            snap_in_view_plane(&camera, &[raw_anchor], &[guide], 1.0, Some(previous)).is_some()
        );
    }

    #[test]
    fn free_move_can_satisfy_guides_from_two_other_objects() {
        let camera = front_camera();
        let raw_anchors = [Vec3::new(0.02, 0.4, 0.0), Vec3::new(0.4, 0.02, 0.0)];
        let snap = snap_in_view_plane(
            &camera,
            &raw_anchors,
            &[vertical_guide(), horizontal_guide()],
            1.0,
            None,
        )
        .expect("both nearby guides should acquire");
        assert!((raw_anchors[0] + snap.correction).x.abs() < 0.0001);
        assert!((raw_anchors[1] + snap.correction).y.abs() < 0.0001);
        assert!(snap.active.secondary.is_some());
    }
}
