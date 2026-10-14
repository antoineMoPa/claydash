//! Persistent, directed C0/C1 constraints for parallel material-height patches.
use super::{
    object_world_matrix, SdfObject, SdfParams, SheetCut, SheetEdge, SheetEmboss, SheetPanelParams,
    SheetSurface,
};
use glam::{Mat4, Vec2};

impl SheetEdge {
    fn axis(self) -> usize {
        match self {
            Self::MinX | Self::MaxX => 0,
            Self::MinY | Self::MaxY => 1,
        }
    }
    fn sign(self) -> f32 {
        match self {
            Self::MinX | Self::MinY => -1.0,
            Self::MaxX | Self::MaxY => 1.0,
        }
    }
    fn opposite(self) -> Self {
        match self {
            Self::MinX => Self::MaxX,
            Self::MaxX => Self::MinX,
            Self::MinY => Self::MaxY,
            Self::MaxY => Self::MinY,
        }
    }
    fn index(self, station: usize, inner: bool) -> usize {
        let index = match (self.sign() > 0.0, inner) {
            (false, false) => 0,
            (false, true) => 1,
            (true, false) => 3,
            (true, true) => 2,
        };
        if self.axis() == 0 {
            station * 4 + index
        } else {
            index * 4 + station
        }
    }
    fn endpoints(self, size: Vec2) -> [Vec2; 2] {
        let mut a = -size * 0.5;
        let mut b = size * 0.5;
        a[self.axis()] = size[self.axis()] * self.sign() * 0.5;
        b[self.axis()] = a[self.axis()];
        [a, b]
    }
}

fn edge_available(panel: &SheetPanelParams, edge: SheetEdge) -> bool {
    if !panel.bends.is_empty() {
        return false;
    }
    let [a, b] = edge.endpoints(panel.size);
    let distance = |point: Vec2| {
        let e = b - a;
        point.distance(a + e * ((point - a).dot(e) / e.length_squared()).clamp(0.0, 1.0))
    };
    for cut in &panel.cuts {
        match cut {
            SheetCut::Straight { from, to } => {
                let e = *to - *from;
                for point in [a, b] {
                    let mut inward = Vec2::ZERO;
                    inward[edge.axis()] = edge.sign() * panel.size[edge.axis()] * 1e-4;
                    // A line coincident with the boundary must retain the
                    // sheet's interior, not just a zero-area edge or stray tab.
                    for probe in [point, point - inward] {
                        let p = probe - *from;
                        if (e.y * p.x - e.x * p.y) / e.length() > 1e-6 {
                            return false;
                        }
                    }
                }
            }
            SheetCut::Circle { center, radius } => {
                if distance(*center) <= *radius + 1e-5 {
                    return false;
                }
            }
        }
    }
    for relief in &panel.embosses {
        let SheetEmboss::Circle {
            center,
            radius,
            transition,
            ..
        } = relief;
        if distance(*center) <= radius + transition + 1e-5 {
            return false;
        }
    }
    for path in &panel.path_embosses {
        // Endpoint hull plus maximum half-width encloses every swept segment.
        for segment in path.stations.windows(2) {
            let width = segment[0].width.max(segment[1].width);
            let axis = edge.axis();
            let boundary = a[axis];
            let low = segment[0].point[axis].min(segment[1].point[axis]) - width;
            let high = segment[0].point[axis].max(segment[1].point[axis]) + width;
            if boundary >= low - 1e-5 && boundary <= high + 1e-5 {
                return false;
            }
        }
    }
    true
}

/// Resolve on a temporary copy and commit only when every dependency is valid.
/// A panel has at most one incoming seam. Sources may have many dependants.
/// Existing authored control values outside the two constrained rows are retained.
pub fn resolve_sheet_seams(objects: &mut [SdfObject]) -> Result<(), String> {
    if !objects
        .iter()
        .any(|o| matches!(&o.params,SdfParams::SheetPanelParams(p) if p.seam.is_some()))
    {
        return Ok(());
    }
    let mut draft = objects.to_vec();
    let mut state = vec![0u8; draft.len()];
    for i in 0..draft.len() {
        visit(i, &mut draft, &mut state, 0)?;
    }
    objects.clone_from_slice(&draft);
    Ok(())
}

fn visit(
    i: usize,
    objects: &mut [SdfObject],
    state: &mut [u8],
    depth: usize,
) -> Result<(), String> {
    if state[i] == 2 {
        return Ok(());
    }
    if state[i] == 1 {
        return Err("sheet seam dependency cycle".into());
    }
    if depth > 64 {
        return Err("sheet seam dependency chains support at most 64 links".into());
    }
    state[i] = 1;
    let seam = match &objects[i].params {
        SdfParams::SheetPanelParams(p) => p.seam.clone(),
        _ => None,
    };
    if let Some(seam) = seam {
        let source = objects
            .iter()
            .position(|o| o.uuid == seam.source)
            .ok_or("sheet seam source missing")?;
        visit(source, objects, state, depth + 1)?;
        let SdfParams::SheetPanelParams(source_panel) = &objects[source].params else {
            return Err("sheet seam source must be a sheet panel".into());
        };
        let SdfParams::SheetPanelParams(target_panel) = &objects[i].params else {
            unreachable!()
        };
        source_panel.validate()?;
        target_panel.validate()?;
        if seam.edge != seam.source_edge.opposite()
            || !edge_available(source_panel, seam.source_edge)
            || !edge_available(target_panel, seam.edge)
        {
            return Err("sheet seams require opposite full rectangular edges with no boundary cuts, relief support or bends".into());
        }
        for index in [source, i] {
            let mut ancestor = Some(objects[index].uuid);
            let mut visited = std::collections::HashSet::new();
            while let Some(id) = ancestor {
                if !visited.insert(id) {
                    return Err("sheet seam Boolean ancestry cycle".into());
                }
                let object = objects
                    .iter()
                    .find(|o| o.uuid == id)
                    .ok_or("sheet seam ancestor missing")?;
                if object.lattice.is_some()
                    || object.mirror.is_some()
                    || object.surface_inlay.is_some()
                    || (object.softness != 0.0 && super::has_boolean_children(objects, id))
                    || object.operation != super::BooleanOperation::Union
                    || (object.repetition.enabled
                        && object.repetition.count.iter().any(|count| *count > 1))
                {
                    return Err("sheet seam panels and ancestors require hard unions without lattice, mirror, repetition or surface inlay".into());
                }
                ancestor = object.boolean_parent;
            }
        }
        let source_world = object_world_matrix(objects, objects[source].uuid);
        let target_world = object_world_matrix(objects, objects[i].uuid);
        if !source_world.is_finite()
            || !target_world.is_finite()
            || source_world.determinant().abs() < 1e-8
            || target_world.determinant().abs() < 1e-8
        {
            return Err("sheet seams require finite nonsingular transforms".into());
        }
        let relative = target_world.inverse() * source_world;
        if !relative.is_finite() {
            return Err("sheet seam relative transform must be finite".into());
        }
        let translation = relative.w_axis.truncate();
        let linear = Mat4::from_cols(
            relative.x_axis,
            relative.y_axis,
            relative.z_axis,
            glam::Vec4::W,
        );
        if linear
            .to_cols_array()
            .iter()
            .zip(Mat4::IDENTITY.to_cols_array())
            .any(|(a, b)| (*a - b).abs() > 1e-5)
        {
            return Err("sheet seam panels require matching world rotation and scale".into());
        }
        let axis = seam.edge.axis();
        let tangent = 1 - axis;
        let source_edge = source_panel.size[axis] * seam.source_edge.sign() * 0.5;
        let target_edge = target_panel.size[axis] * seam.edge.sign() * 0.5;
        if (source_panel.size[tangent] - target_panel.size[tangent]).abs() > 1e-5
            || translation[tangent].abs() > 1e-5
            || (target_edge - source_edge - translation[axis] - seam.source_edge.sign() * seam.gap)
                .abs()
                > 1e-5
        {
            return Err("sheet seam edges must have equal spans, aligned tangent coordinates and translations matching the declared gap".into());
        }
        let source_surface = source_panel.surface.clone().unwrap_or_default();
        let mut target_surface = target_panel.surface.clone().unwrap_or_default();
        let ratio = target_panel.size[axis] / source_panel.size[axis];
        for station in 0..4 {
            let height = source_surface.heights[seam.source_edge.index(station, false)];
            let outward = height - source_surface.heights[seam.source_edge.index(station, true)];
            target_surface.heights[seam.edge.index(station, false)] = height + translation.z;
            target_surface.heights[seam.edge.index(station, true)] =
                height + translation.z + outward * ratio;
        }
        let SdfParams::SheetPanelParams(target_panel) = &mut objects[i].params else {
            unreachable!()
        };
        target_panel.surface = Some(SheetSurface {
            heights: target_surface.heights,
        });
        target_panel.validate()?;
    }
    state[i] = 2;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{PrimitiveKind, SheetSeam, Transform};
    use super::*;
    use glam::{Quat, Vec3};
    fn pair(edge: SheetEdge, gap: f32) -> Vec<SdfObject> {
        let mut source = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        source.params = SdfParams::SheetPanelParams(SheetPanelParams {
            size: Vec2::new(0.8, 0.6),
            thickness: 0.005,
            surface: Some(SheetSurface {
                heights: [
                    0.0, 0.04, 0.07, 0.08, 0.02, 0.08, 0.11, 0.12, 0.02, 0.08, 0.11, 0.12, 0.0,
                    0.04, 0.07, 0.08,
                ],
            }),
            ..Default::default()
        });
        let mut target = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        let mut size = Vec2::new(0.8, 0.6);
        size[edge.axis()] *= 0.6;
        target.transform.translation[edge.axis()] =
            edge.sign() * (Vec2::new(0.8, 0.6)[edge.axis()] * 0.5 + size[edge.axis()] * 0.5 + gap);
        target.transform.translation.z = 0.03;
        target.params = SdfParams::SheetPanelParams(SheetPanelParams {
            size,
            thickness: 0.005,
            seam: Some(SheetSeam {
                source: source.uuid,
                source_edge: edge,
                edge: edge.opposite(),
                gap,
            }),
            ..Default::default()
        });
        vec![source, target]
    }
    fn assert_continuity(objects: &[SdfObject], edge: SheetEdge, gap: f32) {
        let SdfParams::SheetPanelParams(source) = &objects[0].params else {
            panic!()
        };
        let SdfParams::SheetPanelParams(target) = &objects[1].params else {
            panic!()
        };
        let a = edge.endpoints(source.size);
        let b = edge.opposite().endpoints(target.size);
        for step in 0..=40 {
            let t = step as f32 / 40.0;
            let u = a[0].lerp(a[1], t);
            let v = b[0].lerp(b[1], t);
            let source_point = objects[0]
                .transform
                .matrix()
                .transform_point3(source.material_point(u));
            let target_point = objects[1]
                .transform
                .matrix()
                .transform_point3(target.material_point(v));
            let mut offset = Vec3::ZERO;
            offset[edge.axis()] = edge.sign() * gap;
            assert!(source_point.distance(target_point - offset) < 1e-6);
            for axis in 0..2 {
                let mut du = Vec2::ZERO;
                du[axis] = 1e-4;
                // One-sided derivatives normal to boundaries, central along them.
                let (su, tv) = if axis == edge.axis() {
                    (u - du * edge.sign(), v + du * edge.sign())
                } else {
                    (u + du, v + du)
                };
                let ds =
                    (source.material_point(u) - source.material_point(su)) / (u[axis] - su[axis]);
                let dt =
                    (target.material_point(tv) - target.material_point(v)) / (tv[axis] - v[axis]);
                assert!(ds.distance(dt) < 0.003, "tangent mismatch {ds:?} {dt:?}");
            }
        }
    }
    #[test]
    fn seam_solver_matches_positions_and_both_tangents_after_later_source_edits() {
        for edge in [
            SheetEdge::MinX,
            SheetEdge::MaxX,
            SheetEdge::MinY,
            SheetEdge::MaxY,
        ] {
            for gap in [0.0, 0.005] {
                let mut objects = pair(edge, gap);
                resolve_sheet_seams(&mut objects).unwrap();
                assert_continuity(&objects, edge, gap);
                let SdfParams::SheetPanelParams(source) = &mut objects[0].params else {
                    panic!()
                };
                for (i, h) in source
                    .surface
                    .as_mut()
                    .unwrap()
                    .heights
                    .iter_mut()
                    .enumerate()
                {
                    *h += 0.007 * (i % 3) as f32;
                }
                resolve_sheet_seams(&mut objects).unwrap();
                assert_continuity(&objects, edge, gap);
                let encoded = serde_json::to_value(&objects).unwrap();
                let mut restored: Vec<SdfObject> = serde_json::from_value(encoded.clone()).unwrap();
                resolve_sheet_seams(&mut restored).unwrap();
                assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
            }
        }
    }
    #[test]
    fn seam_errors_preserve_all_objects_and_reject_cycles_cuts_and_transforms() {
        let baseline = pair(SheetEdge::MaxX, 0.005);
        for case in 0..7 {
            let mut objects = baseline.clone();
            let target_id = objects[1].uuid;
            match case {
                0 => {
                    let SdfParams::SheetPanelParams(p) = &mut objects[0].params else {
                        panic!()
                    };
                    p.seam = Some(SheetSeam {
                        source: target_id,
                        source_edge: SheetEdge::MinX,
                        edge: SheetEdge::MaxX,
                        gap: 0.005,
                    });
                }
                1 => {
                    objects[1].transform.translation.x += 0.01;
                }
                2 => {
                    objects[1].transform.rotation = Quat::from_rotation_z(0.1);
                }
                3 => {
                    let SdfParams::SheetPanelParams(p) = &mut objects[1].params else {
                        panic!()
                    };
                    p.seam.as_mut().unwrap().source = uuid::Uuid::new_v4();
                }
                4 => {
                    let SdfParams::SheetPanelParams(p) = &mut objects[0].params else {
                        panic!()
                    };
                    p.cuts.push(SheetCut::Circle {
                        center: Vec2::new(0.4, 0.0),
                        radius: 0.03,
                    });
                }
                5 => {
                    objects[0].mirror = Some(super::super::Mirror::default());
                }
                _ => {
                    objects[0].repetition.enabled = true;
                    objects[0].repetition.count[0] = 2;
                }
            }
            let before = serde_json::to_value(&objects).unwrap();
            assert!(resolve_sheet_seams(&mut objects).is_err());
            assert_eq!(serde_json::to_value(&objects).unwrap(), before);
        }
        for retains_interior in [false, true] {
            let mut objects = baseline.clone();
            let SdfParams::SheetPanelParams(panel) = &mut objects[0].params else {
                panic!()
            };
            let (from, to) = if retains_interior {
                (Vec2::new(0.4, -0.3), Vec2::new(0.4, 0.3))
            } else {
                (Vec2::new(0.4, 0.3), Vec2::new(0.4, -0.3))
            };
            panel.cuts.push(SheetCut::Straight { from, to });
            assert_eq!(
                resolve_sheet_seams(&mut objects).is_ok(),
                retains_interior,
                "coincident trim must retain material beside the seam"
            );
        }
    }
    #[test]
    fn seam_supports_shared_group_transforms_and_rejects_inherited_modifiers() {
        let mut objects = pair(SheetEdge::MaxX, 0.0);
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.softness = 0.0;
        parent.group_transform = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_y(0.7),
            scale: Vec3::splat(2.0),
        };
        for object in &mut objects {
            object.boolean_parent = Some(parent.uuid);
        }
        objects.push(parent);
        resolve_sheet_seams(&mut objects).unwrap();
        objects[2].mirror = Some(super::super::Mirror::default());
        assert!(resolve_sheet_seams(&mut objects)
            .unwrap_err()
            .contains("ancestors"));
    }
}
