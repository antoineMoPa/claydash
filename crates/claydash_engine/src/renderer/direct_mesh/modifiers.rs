//! Explicit dispatch and transforms for native modifier semantics.
use super::{check_cancel, combine, Mesh};
use crate::model::{BooleanOperation, RepetitionMode, SdfObject, SdfParams};
use glam::{Mat4, Vec2, Vec3};
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

/// Validate before field preparation as world matrices deliberately tolerate
/// hierarchy cycles for interactive scene editing.
pub(super) fn validate_hierarchy(scene: &[SdfObject], root: usize) -> Result<(), String> {
    fn visit(scene: &[SdfObject], index: usize, stack: &mut Vec<Uuid>) -> Result<(), String> {
        let object = &scene[index];
        if stack.contains(&object.uuid) {
            return Err("Direct mesh encountered a Boolean hierarchy cycle".into());
        }
        stack.push(object.uuid);
        let mut ancestor = Some(object.uuid);
        let mut chain = Vec::new();
        while let Some(id) = ancestor {
            if chain.contains(&id) {
                return Err("Direct mesh encountered a Boolean hierarchy cycle".into());
            }
            chain.push(id);
            let Some(candidate) = scene.iter().find(|candidate| candidate.uuid == id) else {
                return Err("Direct mesh hierarchy references a missing parent".into());
            };
            for matrix in [
                candidate.transform.matrix(),
                candidate.group_transform.matrix(),
            ] {
                if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
                    return Err(format!(
                        "Direct mesh requires a finite, invertible transform on {}",
                        candidate.name
                    ));
                }
            }
            ancestor = candidate.boolean_parent;
        }
        let matrix = crate::model::object_world_matrix(scene, object.uuid);
        if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
            return Err(format!(
                "Direct mesh requires a finite, invertible transform on {}",
                object.name
            ));
        }
        for (child, candidate) in scene.iter().enumerate() {
            if candidate.boolean_parent == Some(object.uuid) {
                visit(scene, child, stack)?;
            }
        }
        stack.pop();
        Ok(())
    }
    visit(scene, root, &mut Vec::new())
}

pub(super) fn needs_field(scene: &[SdfObject], index: usize) -> bool {
    let object = &scene[index];
    let children: Vec<_> = scene
        .iter()
        .enumerate()
        .filter_map(|(index, child)| (child.boolean_parent == Some(object.uuid)).then_some(index))
        .collect();
    if object.surface_inlay.is_some() || (!children.is_empty() && object.softness > 0.0) {
        return true;
    }
    match &object.params {
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            // Ring surfaces admit the analytic parametrization. Horn/spindle
            // surfaces need the boundary of the actual native distance field.
            if major_radius.is_finite()
                && *major_radius >= 0.0
                && minor_radius.is_finite()
                && *minor_radius > 0.0
                && major_radius <= minor_radius
            {
                return true;
            }
        }
        SdfParams::PolygonPrismParams(p) if p.edge_softness > 0.0 => {
            if concave_outline(&p.vertices) {
                return true;
            }
        }
        _ => {}
    }
    // Completed hard group operations act on the entire source surface set.
    // Its operands may themselves use field tessellation or lattice deformation.
    false
}

fn concave_outline(vertices: &[Vec2]) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let area: f32 = (0..vertices.len())
        .map(|i| vertices[i].perp_dot(vertices[(i + 1) % vertices.len()]))
        .sum();
    (0..vertices.len()).any(|i| {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        let c = vertices[(i + 2) % vertices.len()];
        (b - a).perp_dot(c - b) * area < -1e-7
    })
}

/// Build clipping solids only around existing mesh bounds, avoiding arbitrary
/// world-sized boxes that lose precision on thin native sheets.
fn clip_axis(
    source: &Mesh,
    frame: Mat4,
    axis: usize,
    lower: f32,
    upper: f32,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    if source.triangles.is_empty() {
        return Ok(source.clone());
    }
    let inverse = frame.inverse();
    let (mut minimum, mut maximum) = source.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(minimum, maximum), point| {
            let local = inverse.transform_point3(*point);
            (minimum.min(local), maximum.max(local))
        },
    );
    if maximum[axis] <= lower || minimum[axis] >= upper {
        return Ok(super::empty_mesh());
    }
    if minimum[axis] >= lower && maximum[axis] <= upper {
        return Ok(source.clone());
    }
    let padding = (maximum - minimum).max_element().max(0.001) * 0.01;
    minimum -= Vec3::splat(padding);
    maximum += Vec3::splat(padding);
    minimum[axis] = minimum[axis].max(lower);
    maximum[axis] = maximum[axis].min(upper);
    let mut box_object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
    box_object.uuid = Uuid::nil();
    box_object.params = SdfParams::BoxParams(crate::model::BoxParams {
        box_q: (maximum - minimum) * 0.5,
        corner_radius: 0.0,
    });
    let mut clipping = super::primitives::build(&[], &box_object, resolution, cancel)?;
    transform_mesh(
        &mut clipping,
        frame * Mat4::from_translation((minimum + maximum) * 0.5),
        cancel,
    )?;
    combine(source, &clipping, BooleanOperation::Intersect, Uuid::nil())
}

fn transform_mesh(mesh: &mut Mesh, matrix: Mat4, cancel: &AtomicBool) -> Result<(), String> {
    for (index, point) in mesh.positions.iter_mut().enumerate() {
        if index % 256 == 0 {
            check_cancel(cancel)?;
        }
        *point = matrix.transform_point3(*point);
    }
    if matrix.determinant() < 0.0 {
        for face in &mut mesh.triangles {
            face.swap(1, 2);
        }
    }
    mesh.normals.clear();
    Ok(())
}

/// The finite CPU mapping rounds first, then clamps. Even counts therefore
/// have half-integer outside cells as well as the integer inner cells.
fn linear_cells(count: u32, spacing: f32) -> Vec<(f32, f32, f32)> {
    if count <= 1 {
        return vec![(0.0, f32::NEG_INFINITY, f32::INFINITY)];
    }
    let half = (count - 1) as f32 * 0.5;
    let inner = half.floor() as i32;
    let mut cells = Vec::new();
    if count % 2 == 0 {
        cells.push((
            -half * spacing,
            f32::NEG_INFINITY,
            -(inner as f32 + 0.5) * spacing,
        ));
    }
    for cell in -inner..=inner {
        let lower = if count % 2 == 1 && cell == -inner {
            f32::NEG_INFINITY
        } else {
            (cell as f32 - 0.5) * spacing
        };
        let upper = if count % 2 == 1 && cell == inner {
            f32::INFINITY
        } else {
            (cell as f32 + 0.5) * spacing
        };
        cells.push((cell as f32 * spacing, lower, upper));
    }
    if count % 2 == 0 {
        cells.push((
            half * spacing,
            (inner as f32 + 0.5) * spacing,
            f32::INFINITY,
        ));
    }
    cells
}

fn linear_copies(
    scene: &[SdfObject],
    object: &SdfObject,
    source: &Mesh,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    let has_children = scene
        .iter()
        .any(|child| child.boolean_parent == Some(object.uuid));
    let frame = if has_children {
        crate::model::group_world_matrix(scene, object.uuid)
    } else {
        crate::model::object_world_matrix(scene, object.uuid)
    };
    let inverse = frame.inverse();
    let mut result = source.clone();
    for axis in 0..3 {
        if object.repetition.count[axis] <= 1 {
            continue;
        }
        let base = result;
        result = super::empty_mesh();
        for (offset, lower, upper) in linear_cells(
            object.repetition.count[axis],
            object.repetition.spacing[axis].max(0.001),
        ) {
            check_cancel(cancel)?;
            let mut copy = base.clone();
            let mut translation = Vec3::ZERO;
            translation[axis] = offset;
            transform_mesh(
                &mut copy,
                frame * Mat4::from_translation(translation) * inverse,
                cancel,
            )?;
            copy = clip_axis(&copy, frame, axis, lower, upper, resolution, cancel)?;
            result = combine(&result, &copy, BooleanOperation::Union, object.uuid)?;
        }
    }
    Ok(result)
}

fn mirror_copies(
    scene: &[SdfObject],
    object: &SdfObject,
    source: &Mesh,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    let Some(mirror) = object.mirror else {
        return Ok(source.clone());
    };
    let frame = crate::model::group_world_matrix(scene, object.uuid);
    let inverse = frame.inverse();
    let mut result = source.clone();
    for axis in 0..3 {
        if !mirror.axes[axis] {
            continue;
        }
        result = clip_axis(&result, frame, axis, 0.0, f32::INFINITY, resolution, cancel)?;
        let mut reflected = result.clone();
        let mut scale = Vec3::ONE;
        scale[axis] = -1.0;
        transform_mesh(
            &mut reflected,
            frame * Mat4::from_scale(scale) * inverse,
            cancel,
        )?;
        result = combine(&result, &reflected, BooleanOperation::Union, object.uuid)?;
    }
    Ok(result)
}

pub(super) fn leaf_repetition(
    scene: &[SdfObject],
    object: &SdfObject,
    source: &Mesh,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    if !object.repetition.enabled
        || scene
            .iter()
            .any(|child| child.boolean_parent == Some(object.uuid))
    {
        return Ok(source.clone());
    }
    match object.repetition.mode {
        RepetitionMode::Linear => linear_copies(scene, object, source, resolution, cancel),
        RepetitionMode::Radial { .. } => radial_copies(scene, object, source, cancel),
    }
}

pub(super) fn apply(
    scene: &[SdfObject],
    object: &SdfObject,
    source: &Mesh,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    let mut result = source.clone();
    let children = scene
        .iter()
        .any(|child| child.boolean_parent == Some(object.uuid));
    if children && object.repetition.enabled {
        match object.repetition.mode {
            RepetitionMode::Linear => {
                result = linear_copies(scene, object, &result, resolution, cancel)?
            }
            _ => {}
        }
    }
    result = mirror_copies(scene, object, &result, resolution, cancel)?;
    if children
        && object.repetition.enabled
        && matches!(object.repetition.mode, RepetitionMode::Radial { .. })
    {
        result = radial_copies(scene, object, &result, cancel)?;
    }
    Ok(result)
}

/// Native radial repetition takes the minimum of whole rotated subtrees.
/// Conjugating by the object's world matrix retains pivot, group rotation,
/// nonuniform scale, nested children and source material owners.
pub(super) fn radial_copies(
    scene: &[SdfObject],
    object: &SdfObject,
    source: &Mesh,
    cancel: &AtomicBool,
) -> Result<Mesh, String> {
    let RepetitionMode::Radial { axis, count, pivot } = object.repetition.mode else {
        return Err("Direct linear repetition requires field tessellation".into());
    };
    let count = count.clamp(1, 32);
    if !pivot.is_finite() {
        return Err("Direct radial repetition requires a finite pivot".into());
    }
    let frame = crate::model::object_world_matrix(scene, object.uuid);
    let inverse = frame.inverse();
    let mut result = source.clone();
    for copy in 1..count {
        check_cancel(cancel)?;
        let local = Mat4::from_translation(pivot)
            * Mat4::from_quat(axis.rotation(std::f32::consts::TAU * copy as f32 / count as f32))
            * Mat4::from_translation(-pivot);
        let matrix = frame * local * inverse;
        let mut operand = source.clone();
        for (index, point) in operand.positions.iter_mut().enumerate() {
            if index % 256 == 0 {
                check_cancel(cancel)?;
            }
            *point = matrix.transform_point3(*point);
        }
        operand.normals.clear();
        result = combine(&result, &operand, BooleanOperation::Union, object.uuid)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mirror, PrimitiveKind, RepetitionAxis};
    use glam::{Quat, Vec3};
    use std::sync::atomic::AtomicU32;

    fn volume(mesh: &Mesh) -> f32 {
        mesh.triangles
            .iter()
            .map(|face| {
                let [a, b, c] = face.map(|v| mesh.positions[v as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn ordinary_thin_sheet_stays_analytic_and_smooth_groups_use_field() {
        let mut panel = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        assert!(!needs_field(&[panel.clone()], 0));
        panel.mirror = Some(Mirror::default());
        assert!(!needs_field(&[panel.clone()], 0));
        panel.mirror = None;
        panel.softness = 0.2;
        let mut cutter = SdfObject::create_kind(PrimitiveKind::Sphere);
        cutter.boolean_parent = Some(panel.uuid);
        assert!(needs_field(&[panel.clone(), cutter], 0));
    }

    #[test]
    fn linear_even_counts_and_overlap_stay_analytic() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.repetition.enabled = true;
        object.repetition.count = [4, 2, 1];
        object.repetition.spacing = Vec3::splat(0.2);
        assert!(!needs_field(&[object], 0));
    }

    #[test]
    fn radial_copies_transform_entire_hard_group_about_local_pivot() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Box);
        root.softness = 0.0;
        root.params = SdfParams::BoxParams(crate::model::BoxParams {
            box_q: Vec3::splat(0.1),
            corner_radius: 0.0,
        });
        root.transform.translation = Vec3::new(2.0, 0.0, 0.0);
        root.group_transform.rotation = Quat::from_rotation_x(0.4);
        root.group_transform.translation = Vec3::new(8.0, 2.0, -1.0);
        root.group_transform.scale = Vec3::new(1.5, 0.8, 1.0);
        let mut child = root.clone();
        child.uuid = Uuid::new_v4();
        child.group_transform = Default::default();
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::new(2.0, 0.0, 0.4);
        root.repetition.enabled = true;
        // Owning primitive frame has its own translation; use an offset pivot.
        root.repetition.mode = RepetitionMode::Radial {
            axis: RepetitionAxis::Z,
            count: 3,
            pivot: Vec3::new(-2.0, 0.0, 0.0),
        };
        let scene = [root.clone(), child];
        assert!(!needs_field(&scene, 0));
        let mesh = super::super::build(
            &scene,
            root.uuid,
            16,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!((volume(&mesh) - 0.008 * 2.0 * 3.0 * 1.2).abs() < 1e-4);
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        assert!(mesh.owners.contains(&scene[1].uuid));
        assert!(mesh.owners.contains(&root.uuid));
    }

    #[test]
    fn horn_torus_and_rounded_concave_prism_use_field() {
        let mut torus = SdfObject::create_kind(PrimitiveKind::Torus);
        torus.params = SdfParams::TorusParams {
            major_radius: 0.5,
            minor_radius: 0.5,
        };
        assert!(needs_field(&[torus], 0));
        assert!(concave_outline(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 0.5),
            Vec2::new(0.0, 1.0),
        ]));
    }

    #[test]
    fn even_linear_cells_match_native_round_then_clamp() {
        let cells = linear_cells(4, 1.0);
        assert_eq!(
            cells.iter().map(|cell| cell.0).collect::<Vec<_>>(),
            vec![-1.5, -1.0, 0.0, 1.0, 1.5]
        );
        for x in [-3.0_f32, -1.6, -1.4, -0.6, -0.4, 0.4, 0.6, 1.4, 1.6, 3.0] {
            let offset = cells
                .iter()
                .find(|(_, lower, upper)| x > *lower && x < *upper)
                .unwrap()
                .0;
            assert_eq!(offset, x.round().clamp(-1.5, 1.5));
        }
    }

    #[test]
    fn mirror_discards_negative_source_instead_of_unioning_whole_reflection() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.transform.translation.x = -2.0;
        object.mirror = Some(Mirror::default());
        let source = super::super::primitives::build(
            &[object.clone()],
            &object,
            16,
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut source = source;
        transform_mesh(
            &mut source,
            object.transform.matrix(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let result = apply(
            &[object.clone()],
            &object,
            &source,
            16,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.triangles.is_empty());
    }

    #[test]
    fn thin_bent_sheet_mirror_and_even_repetition_keep_analytic_closed_faces() {
        use crate::model::{SheetBend, SheetPanelParams};
        let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        object.params = SdfParams::SheetPanelParams(SheetPanelParams {
            thickness: 0.002,
            bends: vec![SheetBend {
                start: 0.0,
                radius: 0.3,
                angle: 1.0,
                direction: 0.0,
            }],
            ..Default::default()
        });
        object.mirror = Some(Mirror::default());
        object.repetition.enabled = true;
        object.repetition.count = [4, 1, 1];
        object.repetition.spacing.x = 3.0;
        assert!(!needs_field(&[object.clone()], 0));
        let cancel = AtomicBool::new(false);
        let source =
            super::super::primitives::build(&[object.clone()], &object, 16, &cancel).unwrap();
        // Exercise the analytic path directly, so a dense-field fallback cannot
        // accidentally make this thin-sheet regression pass.
        let repeated = leaf_repetition(&[object.clone()], &object, &source, 16, &cancel).unwrap();
        let result = apply(&[object.clone()], &object, &repeated, 16, &cancel).unwrap();
        assert!(super::super::manifold(&result).unwrap().is_manifold());
        assert!(result.triangles.len() < 20_000);
        assert!((volume(&result) - 0.016).abs() < 0.0002);
    }

    #[test]
    fn smooth_mirror_linear_lattice_and_external_inlay_export_together() {
        use crate::model::{Lattice, SurfaceInlay};
        let mut root = SdfObject::create_kind(PrimitiveKind::Box);
        root.params = SdfParams::BoxParams(crate::model::BoxParams {
            box_q: Vec3::splat(0.25),
            corner_radius: 0.0,
        });
        root.softness = 0.06;
        root.mirror = Some(Mirror::default());
        root.repetition.enabled = true;
        root.repetition.count = [2, 1, 1];
        root.repetition.spacing.x = 1.2;
        let mut cage = Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
        cage.offsets.fill(Vec3::new(0.02, 0.01, 0.0));
        root.lattice = Some(cage);
        let host = SdfObject::create_kind(PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation.x = 0.4;
        child.surface_inlay = Some(SurfaceInlay {
            host: host.uuid,
            offset: 0.0,
            thickness: 0.06,
        });
        let scene = [root.clone(), child.clone(), host];
        let mesh = super::super::build(
            &scene,
            root.uuid,
            16,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!(!mesh.triangles.is_empty());
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        assert!(mesh
            .owners
            .iter()
            .all(|owner| *owner == root.uuid || *owner == child.uuid));
    }

    #[test]
    fn hard_thin_group_mirrors_and_rotates_implicit_inlay_child() {
        use crate::model::{SheetBend, SheetPanelParams, SurfaceInlay};
        let mut root = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        root.softness = 0.0;
        root.params = SdfParams::SheetPanelParams(SheetPanelParams {
            thickness: 0.002,
            bends: vec![SheetBend {
                start: 0.0,
                radius: 0.3,
                angle: 0.8,
                direction: 0.0,
            }],
            ..Default::default()
        });
        root.mirror = Some(Mirror::default());
        root.repetition.enabled = true;
        root.repetition.mode = RepetitionMode::Radial {
            axis: RepetitionAxis::Z,
            count: 2,
            pivot: Vec3::new(3.0, 0.0, 0.0),
        };
        let mut host = SdfObject::create_kind(PrimitiveKind::Sphere);
        host.transform.translation = Vec3::new(0.4, 2.0, 0.0);
        let mut child = host.clone();
        child.uuid = Uuid::new_v4();
        child.boolean_parent = Some(root.uuid);
        child.surface_inlay = Some(SurfaceInlay {
            host: host.uuid,
            offset: 0.0,
            thickness: 0.08,
        });
        let scene = [root.clone(), child.clone(), host];
        assert!(!needs_field(&scene, 0));
        assert!(needs_field(&scene, 1));
        let mesh = super::super::build(
            &scene,
            root.uuid,
            16,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        assert!(mesh.owners.contains(&root.uuid));
        assert!(mesh.owners.contains(&child.uuid));
    }

    #[test]
    fn smooth_group_even_linear_caps_are_meshed_after_continuous_base() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Box);
        root.params = SdfParams::BoxParams(crate::model::BoxParams {
            box_q: Vec3::splat(0.3),
            corner_radius: 0.0,
        });
        root.transform.translation.x = -0.5;
        root.softness = 0.08;
        root.mirror = Some(Mirror {
            axes: [false, true, false],
        });
        root.repetition.enabled = true;
        root.repetition.count = [2, 1, 1];
        root.repetition.spacing.x = 1.0;
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 0.22 });
        child.transform.translation = Vec3::new(-0.4, 0.1, 0.0);
        child.boolean_parent = Some(root.uuid);
        let scene = [root.clone(), child];
        let mesh = super::super::build(
            &scene,
            root.uuid,
            16,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        assert!(!mesh.triangles.is_empty());
        // Native round-then-clamp has a sign jump at the outside cell seam.
        // Closed geometry includes that clipped cap although phi has no zero.
        assert!(mesh
            .positions
            .iter()
            .any(|point| (point.x + 0.5).abs() < 1e-5));
    }

    #[test]
    fn cycle_and_singular_ancestor_are_rejected_before_field_preparation() {
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(parent.uuid);
        parent.group_transform.scale.y = 0.0;
        assert!(validate_hierarchy(&[parent.clone(), child.clone()], 1)
            .unwrap_err()
            .contains("invertible"));
        parent.group_transform = Default::default();
        parent.boolean_parent = Some(child.uuid);
        assert!(validate_hierarchy(&[parent, child], 1)
            .unwrap_err()
            .contains("cycle"));
    }
}
