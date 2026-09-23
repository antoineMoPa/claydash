use super::frame::FaceDomain;
use super::*;

pub(super) fn face_edge_directions(domain: &FaceDomain) -> Vec<Vec2> {
    match domain {
        FaceDomain::Rectangle { .. } => vec![Vec2::X, Vec2::Y],
        FaceDomain::Circle { .. } => vec![Vec2::X, Vec2::Y],
        FaceDomain::Polygon(vertices) => vertices
            .iter()
            .enumerate()
            .filter_map(|(index, a)| {
                let edge = vertices[(index + 1) % vertices.len()] - *a;
                (edge.length_squared() > 0.000_001).then_some(edge / edge.length())
            })
            .collect(),
    }
}

#[derive(Clone, Copy)]
pub(super) struct FaceCutGuide {
    pub(super) start: egui::Pos2,
    pub(super) end: egui::Pos2,
    pub(super) label: &'static str,
}

pub(super) struct FaceAnchorGuide {
    pub(super) screen: egui::Pos2,
    pub(super) label: String,
}

pub(super) fn fraction_divisions(projected_span: f32) -> u32 {
    let mut divisions = 1;
    while divisions < 64 && projected_span / (divisions * 2) as f32 >= FRACTION_MIN_SPACING {
        divisions *= 2;
    }
    divisions
}

pub(super) fn fraction_label(index: u32, divisions: u32) -> String {
    if index == 0 {
        return "0".into();
    }
    if index == divisions {
        return "1".into();
    }
    let shift = index.trailing_zeros().min(divisions.trailing_zeros());
    format!("{}/{}", index >> shift, divisions >> shift)
}

pub(super) fn guided_face_anchor_point(
    camera: &Camera,
    scale: f32,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    raw: Vec2,
) -> (Vec2, Option<FaceAnchorGuide>) {
    let Some((_, frame)) = source_and_frame(scene, face) else {
        return (raw, None);
    };
    let screen =
        |point| point_world(scene, face, point).and_then(|world| camera.project(world, scale));
    let Some(pointer) = screen(raw) else {
        return (raw, None);
    };
    let mut best: Option<(u8, f32, Vec2, FaceAnchorGuide)> = None;
    let mut offer = |candidate: Vec2, tier: u8, label: String| {
        if !face_point_inside(&frame.domain, candidate) {
            return;
        }
        let Some(projected) = screen(candidate) else {
            return;
        };
        let error = pointer.distance(projected);
        if error > ANCHOR_GUIDE_DISTANCE
            || best
                .as_ref()
                .is_some_and(|(old_tier, old_error, _, _)| (*old_tier, *old_error) <= (tier, error))
        {
            return;
        }
        best = Some((
            tier,
            error,
            candidate,
            FaceAnchorGuide {
                screen: projected,
                label,
            },
        ));
    };
    let (minimum, maximum, boundary) = match &frame.domain {
        FaceDomain::Rectangle { extent_u, extent_v } => (
            Vec2::new(-extent_u, -extent_v),
            Vec2::new(*extent_u, *extent_v),
            vec![
                Vec2::new(-extent_u, -extent_v),
                Vec2::new(*extent_u, -extent_v),
                Vec2::new(*extent_u, *extent_v),
                Vec2::new(-extent_u, *extent_v),
            ],
        ),
        FaceDomain::Circle { radius } => (Vec2::splat(-radius), Vec2::splat(*radius), Vec::new()),
        FaceDomain::Polygon(vertices) => (
            vertices
                .iter()
                .copied()
                .fold(Vec2::splat(f32::INFINITY), Vec2::min),
            vertices
                .iter()
                .copied()
                .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max),
            vertices.clone(),
        ),
    };
    if let FaceDomain::Circle { radius } = &frame.domain {
        let projected_diameter = screen(Vec2::new(-radius, 0.0))
            .zip(screen(Vec2::new(*radius, 0.0)))
            .map_or(0.0, |(a, b)| a.distance(b));
        let mut steps = 8;
        while steps < 128
            && projected_diameter * std::f32::consts::PI / (steps * 2) as f32
                >= FRACTION_MIN_SPACING
        {
            steps *= 2;
        }
        for index in 0..steps {
            let angle = index as f32 * std::f32::consts::TAU / steps as f32;
            offer(
                Vec2::new(angle.cos(), angle.sin()) * *radius,
                1,
                format!("Rim {}", fraction_label(index, steps)),
            );
        }
        if raw.length_squared() > 0.000_001 {
            offer(raw * (*radius / raw.length()), 2, "Rim".into());
        }
    } else {
        for index in 0..boundary.len() {
            let a = boundary[index];
            let b = boundary[(index + 1) % boundary.len()];
            offer(a, 0, "Corner".into());
            let edge = b - a;
            let length_squared = edge.length_squared();
            if length_squared < 0.000_001 {
                continue;
            }
            let span = screen(a).zip(screen(b)).map_or(0.0, |(a, b)| a.distance(b));
            let divisions = fraction_divisions(span);
            let fraction = ((raw - a).dot(edge) / length_squared).clamp(0.0, 1.0);
            let step = (fraction * divisions as f32).round() as u32;
            offer(
                a + edge * (step as f32 / divisions as f32),
                1,
                format!("Edge {}", fraction_label(step, divisions)),
            );
            offer(a + edge * fraction, 2, "Edge".into());
        }
    }
    let center = (minimum + maximum) * 0.5;
    let span_u = screen(Vec2::new(minimum.x, center.y))
        .zip(screen(Vec2::new(maximum.x, center.y)))
        .map_or(0.0, |(a, b)| a.distance(b));
    let span_v = screen(Vec2::new(center.x, minimum.y))
        .zip(screen(Vec2::new(center.x, maximum.y)))
        .map_or(0.0, |(a, b)| a.distance(b));
    let divisions_u = fraction_divisions(span_u);
    let divisions_v = fraction_divisions(span_v);
    let extent = maximum - minimum;
    if extent.x > 0.000_001 && extent.y > 0.000_001 {
        let u = (((raw.x - minimum.x) / extent.x) * divisions_u as f32)
            .round()
            .clamp(0.0, divisions_u as f32) as u32;
        let v = (((raw.y - minimum.y) / extent.y) * divisions_v as f32)
            .round()
            .clamp(0.0, divisions_v as f32) as u32;
        offer(
            Vec2::new(
                minimum.x + extent.x * u as f32 / divisions_u as f32,
                minimum.y + extent.y * v as f32 / divisions_v as f32,
            ),
            3,
            format!(
                "{} · {}",
                fraction_label(u, divisions_u),
                fraction_label(v, divisions_v)
            ),
        );
    }
    best.map_or((raw, None), |(_, _, point, guide)| (point, Some(guide)))
}

pub(super) fn face_point_inside(domain: &FaceDomain, point: Vec2) -> bool {
    match domain {
        FaceDomain::Rectangle { extent_u, extent_v } => {
            point.x.abs() <= *extent_u && point.y.abs() <= *extent_v
        }
        FaceDomain::Circle { radius } => point.length() <= *radius + 0.001,
        FaceDomain::Polygon(vertices) => crate::model::polygon_distance(point, vertices) <= 0.001,
    }
}

fn face_cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

pub(super) fn guided_outline_point(
    camera: &Camera,
    scale: f32,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    vertices: &[Vec2],
    raw: Vec2,
) -> (Vec2, Vec<FaceCutGuide>, Option<FaceAnchorGuide>) {
    let (anchor_point, anchor_guide) = guided_face_anchor_point(camera, scale, scene, face, raw);
    let Some(&start) = vertices.last() else {
        return (anchor_point, Vec::new(), anchor_guide);
    };
    let (angle_point, angle_guides) = guided_face_point(
        camera,
        scale,
        scene,
        face,
        start,
        &vertices[..vertices.len() - 1],
        raw,
    );
    let Some(anchor) = anchor_guide else {
        return (angle_point, angle_guides, None);
    };
    let screen =
        |point| point_world(scene, face, point).and_then(|world| camera.project(world, scale));
    let Some(pointer) = screen(raw) else {
        return (angle_point, angle_guides, None);
    };
    let angle_error = if angle_guides.is_empty() {
        f32::INFINITY
    } else {
        screen(angle_point).map_or(f32::INFINITY, |point| pointer.distance(point))
    };
    if pointer.distance(anchor.screen) <= angle_error {
        let matching_angle =
            screen(angle_point).is_some_and(|point| point.distance(anchor.screen) <= 3.0);
        return (
            anchor_point,
            if matching_angle {
                angle_guides
            } else {
                Vec::new()
            },
            Some(anchor),
        );
    }
    (angle_point, angle_guides, None)
}

pub(super) fn guided_face_point(
    camera: &Camera,
    scale: f32,
    scene: &[SdfObject],
    face: crate::model::ModelingFaceSelection,
    start: Vec2,
    previous: &[Vec2],
    raw: Vec2,
) -> (Vec2, Vec<FaceCutGuide>) {
    let Some((_, frame)) = source_and_frame(scene, face) else {
        return (raw, Vec::new());
    };
    let delta = raw - start;
    if delta.length_squared() < 0.000_001 {
        return (raw, Vec::new());
    }
    let screen =
        |point| point_world(scene, face, point).and_then(|world| camera.project(world, scale));
    let Some(raw_screen) = screen(raw) else {
        return (raw, Vec::new());
    };
    let directions = face_edge_directions(&frame.domain);
    let mut angle_lines = Vec::new();
    for edge in &directions {
        for (angle, label) in [
            (0.0_f32, "Parallel"),
            (std::f32::consts::FRAC_PI_4, "45°"),
            (std::f32::consts::FRAC_PI_2, "90°"),
            (3.0 * std::f32::consts::FRAC_PI_4, "135°"),
        ] {
            let direction = Vec2::new(
                edge.x * angle.cos() - edge.y * angle.sin(),
                edge.x * angle.sin() + edge.y * angle.cos(),
            );
            angle_lines.push((direction, label));
        }
    }
    let mut combined: Option<(f32, Vec2, Vec<FaceCutGuide>)> = None;
    let mut single: Option<(f32, Vec2, Vec<FaceCutGuide>)> = None;
    let mut offer = |candidate: Vec2, guides: Vec<FaceCutGuide>, both: bool| {
        if !face_point_inside(&frame.domain, candidate) {
            return;
        }
        let Some(candidate_screen) = screen(candidate) else {
            return;
        };
        let error = raw_screen.distance(candidate_screen);
        if error > ANGLE_GUIDE_DISTANCE {
            return;
        }
        let best = if both { &mut combined } else { &mut single };
        if best.as_ref().is_none_or(|value| error < value.0) {
            *best = Some((error, candidate, guides));
        }
    };
    for &(direction, label) in &angle_lines {
        let candidate = start + direction * delta.dot(direction);
        if let (Some(from), Some(to)) = (screen(start), screen(candidate)) {
            offer(
                candidate,
                vec![FaceCutGuide {
                    start: from,
                    end: to,
                    label,
                }],
                false,
            );
        }
        if let Some(prior) = previous.last().copied() {
            let side_length = start.distance(prior);
            let side_direction = if delta.dot(direction) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let equal_side = start + direction * side_length * side_direction;
            if let (Some(from), Some(to)) = (screen(start), screen(equal_side)) {
                offer(
                    equal_side,
                    vec![FaceCutGuide {
                        start: from,
                        end: to,
                        label: "Equal length",
                    }],
                    true,
                );
            }
        }
    }
    for &anchor in previous {
        for &reference in &directions {
            let candidate = anchor + reference * (raw - anchor).dot(reference);
            if let (Some(from), Some(to)) = (screen(anchor), screen(candidate)) {
                offer(
                    candidate,
                    vec![FaceCutGuide {
                        start: from,
                        end: to,
                        label: "Aligned",
                    }],
                    false,
                );
            }
            for &(direction, angle_label) in &angle_lines {
                let denominator = face_cross(direction, reference);
                if denominator.abs() < 0.000_01 {
                    continue;
                }
                let intersection =
                    start + direction * (face_cross(anchor - start, reference) / denominator);
                if intersection.distance_squared(start) < 0.000_001 {
                    continue;
                }
                if let (Some(angle_start), Some(alignment_start), Some(to)) =
                    (screen(start), screen(anchor), screen(intersection))
                {
                    offer(
                        intersection,
                        vec![
                            FaceCutGuide {
                                start: angle_start,
                                end: to,
                                label: angle_label,
                            },
                            FaceCutGuide {
                                start: alignment_start,
                                end: to,
                                label: "Aligned",
                            },
                        ],
                        true,
                    );
                }
            }
        }
    }
    combined
        .or(single)
        .map_or((raw, Vec::new()), |(_, point, guides)| (point, guides))
}
