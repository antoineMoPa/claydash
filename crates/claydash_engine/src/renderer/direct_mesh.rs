//! Direct surfaces and hard mesh CSG, with field tessellation for nonlinear recipes.
use super::poisson_mesh::geometry::Mesh;
use crate::model::{BooleanOperation, SdfObject};
use boolmesh::prelude::{compute_boolean, Manifold, OpType};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use uuid::Uuid;

pub const MIN_RESOLUTION: u32 = 8;
pub const MAX_RESOLUTION: u32 = 256;
pub const DEFAULT_RESOLUTION: u32 = 32;

mod fields;
mod implicit;
mod lattice;
mod modifiers;
mod primitives;
mod quality;
#[cfg(test)]
mod quality_tests;
pub use quality::TessellationQuality;
mod sheet;

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Export cancelled".into())
    } else {
        Ok(())
    }
}

fn report(progress: &AtomicU32, value: u32) {
    let _reported = progress.fetch_max(value, Ordering::Relaxed).max(value);
    #[cfg(target_arch = "wasm32")]
    super::poisson_mesh::web_worker::post_progress(_reported);
}

/// Build world-space geometry using analytic surfaces or direct field tessellation.
pub fn build(
    scene: &[SdfObject],
    root: Uuid,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Mesh, String> {
    build_with_error(scene, root, resolution, TessellationQuality::from_resolution(resolution).position_error, cancel, progress)
}

pub fn build_with_error(scene: &[SdfObject], root: Uuid, resolution: u32,
    world_error: f32, cancel: &AtomicBool, progress: &AtomicU32) -> Result<Mesh, String> {
    check_cancel(cancel)?;
    if !world_error.is_finite() || world_error <= 0.0 { return Err("Direct mesh error must be positive and finite".into()); }
    let quality = TessellationQuality { position_error: world_error, ..TessellationQuality::from_resolution(resolution) };
    if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&resolution) {
        return Err(format!(
            "Direct mesh resolution must be between {MIN_RESOLUTION} and {MAX_RESOLUTION}"
        ));
    }
    report(progress, 0);
    let index = scene
        .iter()
        .position(|object| object.uuid == root)
        .ok_or_else(|| "Direct mesh root no longer exists".to_owned())?;
    modifiers::validate_hierarchy(scene, index)?;
    let mut sampler = fields::PreparedDirectField::new(scene, root)?;
    let mut stack = Vec::new();
    let mut finished = 0;
    let mut mesh = subtree(
        scene,
        index,
        resolution,
        cancel,
        progress,
        &mut stack,
        &mut finished,
        quality,
    )?;
    check_cancel(cancel)?;
    // Boolmesh carries source-face owners through union/intersection. Only new
    // subtraction walls need field ownership: their material comes from the left
    // operand, not from the cutter.
    let mut worker_budget = super::cooperative_work::WorkerBudget::new();
    for (face, triangle) in mesh.triangles.iter().enumerate() {
        if face % 256 == 0 {
            check_cancel(cancel)?;
            worker_budget.checkpoint();
        }
        if !mesh.owners[face].is_nil() {
            continue;
        }
        let [a, b, c] = triangle.map(|vertex| mesh.positions[vertex as usize]);
        mesh.owners[face] = sampler.sample((a + b + c) / 3.0).1;
    }
    mesh.normals = super::poisson_mesh::geometry::corner_normals(
        &mesh.positions,
        &mesh.triangles,
        &mesh.owners,
    );
    report(progress, 200);
    Ok(mesh)
}

fn subtree(
    scene: &[SdfObject],
    index: usize,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicU32,
    stack: &mut Vec<Uuid>,
    finished: &mut usize,
    quality: TessellationQuality,
) -> Result<Mesh, String> {
    check_cancel(cancel)?;
    let object = &scene[index];
    if stack.contains(&object.uuid) {
        return Err("Direct mesh encountered a Boolean hierarchy cycle".into());
    }
    stack.push(object.uuid);
    let children: Vec<_> = scene
        .iter()
        .enumerate()
        .filter_map(|(index, child)| (child.boolean_parent == Some(object.uuid)).then_some(index))
        .collect();
    if modifiers::needs_field(scene, index) {
        let mesh = field_subtree(scene, index, resolution, cancel, progress)?;
        stack.pop();
        *finished += 1;
        return Ok(mesh);
    }
    let mut mesh = primitives::build_with_quality(scene, object, resolution, quality.local(scene, object), cancel)?;
    let matrix = crate::model::object_world_matrix(scene, object.uuid);
    if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
        return Err(format!(
            "Direct mesh requires a finite, invertible transform on {}",
            object.name
        ));
    }
    for (index, position) in mesh.positions.iter_mut().enumerate() {
        if index % 256 == 0 {
            check_cancel(cancel)?;
        }
        *position = matrix.transform_point3(*position);
    }
    if matrix.determinant() < 0.0 {
        for triangle in &mut mesh.triangles {
            triangle.swap(1, 2);
        }
    }
    mesh = match modifiers::leaf_repetition(scene, object, &mesh, resolution, cancel) {
        Ok(mesh) => mesh,
        Err(_) => {
            check_cancel(cancel)?;
            let mesh = field_subtree(scene, index, resolution, cancel, progress)?;
            stack.pop();
            return Ok(mesh);
        }
    };
    if let Err(_) = lattice::apply_with_error(scene, object, &mut mesh, resolution, quality.position_error, cancel) {
        check_cancel(cancel)?;
        let mesh = field_subtree(scene, index, resolution, cancel, progress)?;
        stack.pop();
        return Ok(mesh);
    }
    *finished += 1;
    report(
        progress,
        ((*finished * 99) / scene.len().max(1)).min(99) as u32,
    );
    for child in children {
        let operand = subtree(scene, child, resolution, cancel, progress, stack, finished, quality)?;
        check_cancel(cancel)?;
        report(progress, 100);
        mesh = match combine(&mesh, &operand, scene[child].operation, object.uuid) {
            Ok(mesh) => mesh,
            Err(_) => {
                check_cancel(cancel)?;
                let mesh = field_subtree(scene, index, resolution, cancel, progress)?;
                stack.pop();
                return Ok(mesh);
            }
        };
        check_cancel(cancel)?;
    }
    mesh = match modifiers::apply(scene, object, &mesh, resolution, cancel) {
        Ok(mesh) => mesh,
        Err(_) => {
            check_cancel(cancel)?;
            field_subtree(scene, index, resolution, cancel, progress)?
        }
    };
    stack.pop();
    Ok(mesh)
}

/// Group mirror/repetition folds the completed field. Meshing the unfolded
/// base preserves its smooth surfaces while analytic cell clipping supplies
/// the genuine caps at finite repetition's discontinuous cell boundaries.
fn field_subtree(
    scene: &[SdfObject],
    index: usize,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Mesh, String> {
    let object = &scene[index];
    let children = scene
        .iter()
        .any(|child| child.boolean_parent == Some(object.uuid));
    if object.mirror.is_none() && !(children && object.repetition.enabled) {
        return implicit::build(scene, object.uuid, resolution, cancel, progress);
    }
    let mut source = scene.to_vec();
    source[index].mirror = None;
    if children {
        source[index].repetition.enabled = false;
    }
    let base = implicit::build(&source, object.uuid, resolution, cancel, progress)?;
    check_cancel(cancel)?;
    // Leaf repetition remains within the sampled primitive and its inlay host
    // shell; only its outer mirror may be applied to the completed leaf field.
    modifiers::apply(scene, object, &base, resolution, cancel)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FaceOwner {
    owner: Uuid,
    weight: boolmesh::Real,
}
impl std::ops::Mul<boolmesh::Real> for FaceOwner {
    type Output = Self;
    fn mul(self, factor: boolmesh::Real) -> Self {
        Self {
            owner: self.owner,
            weight: self.weight * factor,
        }
    }
}
impl std::ops::Add for FaceOwner {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        if self.owner == other.owner {
            Self {
                owner: self.owner,
                weight: self.weight + other.weight,
            }
        } else if self.weight >= other.weight {
            self
        } else {
            other
        }
    }
}
fn manifold(mesh: &Mesh) -> Result<Manifold<FaceOwner>, String> {
    manifold_with_owners(mesh, false)
}
fn manifold_with_owners(mesh: &Mesh, cut: bool) -> Result<Manifold<FaceOwner>, String> {
    let positions: Vec<f64> = mesh
        .positions
        .iter()
        .flat_map(|point| [point.x as f64, point.y as f64, point.z as f64])
        .collect();
    let indices: Vec<usize> = mesh
        .triangles
        .iter()
        .flatten()
        .map(|&index| index as usize)
        .collect();
    let values: Vec<_> = mesh
        .owners
        .iter()
        .flat_map(|&owner| {
            [FaceOwner {
                owner: if cut { Uuid::nil() } else { owner },
                weight: 1.0,
            }; 3]
        })
        .collect();
    std::panic::catch_unwind(|| Manifold::new_with_values(&positions, &indices, &values))
        .map_err(|_| "Direct mesh contains unsupported degenerate manifold edges".to_owned())?
        .map_err(|error| format!("Direct mesh is not a closed manifold: {error}"))
}

fn empty_mesh() -> Mesh {
    Mesh {
        positions: vec![],
        triangles: vec![],
        owners: vec![],
        normals: vec![],
        colors: vec![],
        baked_texture: None,
    }
}

fn combine(
    left: &Mesh,
    right: &Mesh,
    operation: BooleanOperation,
    _owner: Uuid,
) -> Result<Mesh, String> {
    if left.triangles.is_empty() {
        return Ok(if operation == BooleanOperation::Union {
            right.clone()
        } else {
            empty_mesh()
        });
    }
    if right.triangles.is_empty() {
        return Ok(if operation == BooleanOperation::Intersect {
            empty_mesh()
        } else {
            left.clone()
        });
    }
    if left.positions == right.positions && left.triangles == right.triangles {
        return Ok(if operation == BooleanOperation::Subtract {
            empty_mesh()
        } else {
            left.clone()
        });
    }
    let bounds = |mesh: &Mesh| {
        mesh.positions.iter().fold(
            (
                glam::Vec3::splat(f32::INFINITY),
                glam::Vec3::splat(f32::NEG_INFINITY),
            ),
            |(min, max), &point| (min.min(point), max.max(point)),
        )
    };
    let (left_min, left_max) = bounds(left);
    let (right_min, right_max) = bounds(right);
    if left_max.cmplt(right_min).any() || right_max.cmplt(left_min).any() {
        return Ok(match operation {
            BooleanOperation::Subtract => left.clone(),
            BooleanOperation::Intersect => empty_mesh(),
            BooleanOperation::Union => {
                let mut joined = left.clone();
                let offset = joined.positions.len() as u32;
                joined.positions.extend_from_slice(&right.positions);
                joined.triangles.extend(
                    right
                        .triangles
                        .iter()
                        .map(|face| face.map(|vertex| vertex + offset)),
                );
                joined.owners.extend_from_slice(&right.owners);
                joined.normals.clear();
                joined.colors.clear();
                joined.baked_texture = None;
                joined
            }
        });
    }
    let cut = operation == BooleanOperation::Subtract;
    let operation = match operation {
        BooleanOperation::Union => OpType::Add,
        BooleanOperation::Subtract => OpType::Subtract,
        BooleanOperation::Intersect => OpType::Intersect,
    };
    let left = manifold(left)?;
    let right = manifold_with_owners(right, cut)?;
    let computed =
        std::panic::catch_unwind(|| compute_boolean(&left, &right, operation)).map_err(|_| {
            "Direct mesh Boolean encountered unsupported degenerate intersections".to_owned()
        })?;
    let result = match computed {
        Ok(result) => result,
        // Boolmesh uses its input manifold constructor for the result too; a
        // valid operation that removes every face is reported by that constructor
        // as an empty matrix rather than returned as an empty manifold.
        Err(error) if error == "empty pos matrix" || error == "empty idx matrix" => {
            return Ok(empty_mesh())
        }
        Err(error) => return Err(format!("Direct mesh Boolean failed: {error}")),
    };
    let positions = result
        .ps
        .iter()
        .map(|point| glam::Vec3::new(point.x as f32, point.y as f32, point.z as f32))
        .collect();
    let triangles: Vec<_> = result
        .hs
        .chunks_exact(3)
        .map(|face| {
            [
                face[0].tail as u32,
                face[1].tail as u32,
                face[2].tail as u32,
            ]
        })
        .collect();
    // vals follows the sorted halfedge/corner order. prop is deduplicated seam
    // metadata and must not be used to index this corner-value array.
    let owners = result
        .vals
        .chunks_exact(3)
        .map(|face| face[0].owner)
        .collect();
    Ok(Mesh {
        positions,
        triangles,
        owners,
        normals: vec![],
        colors: vec![],
        baked_texture: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PrimitiveKind, SdfParams};
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
    fn booleans_preserve_volume_and_subtract_material() {
        let mut left = SdfObject::create_kind(PrimitiveKind::Box);
        left.softness = 0.0;
        if let SdfParams::BoxParams(params) = &mut left.params {
            params.box_q = glam::Vec3::ONE;
        }
        let mut right = left.clone();
        right.uuid = Uuid::new_v4();
        right.boolean_parent = Some(left.uuid);
        right.transform.translation.x = 1.0;
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        for (operation, expected) in [
            (BooleanOperation::Union, 12.0),
            (BooleanOperation::Subtract, 4.0),
            (BooleanOperation::Intersect, 4.0),
        ] {
            right.operation = operation;
            let mesh = build(
                &[left.clone(), right.clone()],
                left.uuid,
                16,
                &cancel,
                &progress,
            )
            .unwrap();
            assert!((volume(&mesh) - expected).abs() < 1e-4);
            assert!(manifold(&mesh).unwrap().is_manifold());
            if operation == BooleanOperation::Subtract {
                assert!(mesh.owners.iter().all(|&owner| owner == left.uuid));
            } else {
                assert!(mesh.owners.contains(&left.uuid));
                assert!(mesh.owners.contains(&right.uuid));
                assert!(mesh
                    .owners
                    .iter()
                    .all(|&owner| owner == left.uuid || owner == right.uuid));
            }
        }
    }
    #[test]
    fn reflection_preserves_outward_winding_and_cancel_is_immediate() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.transform.scale.x = -2.0;
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let mesh = build(&[object.clone()], object.uuid, 16, &cancel, &progress).unwrap();
        assert!(volume(&mesh) > 0.0);
        assert!(manifold(&mesh).unwrap().is_manifold());
        cancel.store(true, Ordering::Relaxed);
        assert!(
            build(&[object.clone()], object.uuid, 16, &cancel, &progress)
                .err()
                .unwrap()
                .contains("cancelled")
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "Manual direct-sheet versus sampled Poisson timing comparison"]
    fn benchmark_sheet_against_poisson() {
        let object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        let scene = [object.clone()];
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let start = std::time::Instant::now();
        let direct = build(&scene, object.uuid, 32, &cancel, &progress).unwrap();
        let direct_time = start.elapsed();
        let start = std::time::Instant::now();
        let poisson = super::super::poisson_mesh::geometry::build(
            &scene,
            object.uuid,
            32,
            32,
            None,
            &progress,
            Some(&cancel),
        )
        .unwrap();
        eprintln!(
            "sheet direct {:?}: {} faces; sampled Poisson {:?}: {} faces",
            direct_time,
            direct.triangles.len(),
            start.elapsed(),
            poisson.triangles.len()
        );
        assert!(manifold(&direct).unwrap().is_manifold());
    }
    #[test]
    fn ordered_nested_csg_and_selected_child_keep_ancestor_frame() {
        use glam::{Quat, Vec3};
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.softness = 0.0;
        if let SdfParams::BoxParams(p) = &mut parent.params {
            p.box_q = Vec3::ONE;
        }
        parent.group_transform.translation = Vec3::new(10., 0., 0.);
        parent.group_transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let mut subtract = parent.clone();
        subtract.uuid = Uuid::new_v4();
        subtract.group_transform = Default::default();
        subtract.boolean_parent = Some(parent.uuid);
        subtract.operation = BooleanOperation::Subtract;
        subtract.transform.translation.x = 1.;
        let mut nested = SdfObject::create_kind(PrimitiveKind::Sphere);
        nested.boolean_parent = Some(subtract.uuid);
        nested.transform.translation.x = 1.;
        let mut intersect = parent.clone();
        intersect.uuid = Uuid::new_v4();
        intersect.group_transform = Default::default();
        intersect.boolean_parent = Some(parent.uuid);
        intersect.operation = BooleanOperation::Intersect;
        if let SdfParams::BoxParams(p) = &mut intersect.params {
            p.box_q.x = 0.5;
        }
        let mut union = intersect.clone();
        union.uuid = Uuid::new_v4();
        union.operation = BooleanOperation::Union;
        union.transform.translation.x = 2.;
        if let SdfParams::BoxParams(p) = &mut union.params {
            p.box_q = Vec3::splat(0.25);
        }
        let scene = [parent.clone(), subtract.clone(), nested, intersect, union];
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let mesh = build(&scene, parent.uuid, 16, &cancel, &progress).unwrap();
        assert!((volume(&mesh) - 2.125).abs() < 1e-4);
        let min = mesh
            .positions
            .iter()
            .fold(Vec3::splat(f32::INFINITY), |a, &b| a.min(b));
        let max = mesh
            .positions
            .iter()
            .fold(Vec3::splat(f32::NEG_INFINITY), |a, &b| a.max(b));
        assert!(min.distance(Vec3::new(9., -1., -2.25)) < 1e-4);
        assert!(max.distance(Vec3::new(11., 1., 0.5)) < 1e-4);
        let child_mesh = build(&scene, subtract.uuid, 16, &cancel, &progress).unwrap();
        assert!((volume(&child_mesh) - 8.).abs() < 1e-4);
        let center =
            child_mesh.positions.iter().copied().sum::<Vec3>() / child_mesh.positions.len() as f32;
        assert!(center.distance(Vec3::new(10., 0., -1.)) < 1e-4);
    }
    #[test]
    fn field_geometry_is_supported_and_invalid_requests_are_explicit_errors() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.softness = 0.0;
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        assert!(build(&[object.clone()], object.uuid, 7, &cancel, &progress)
            .err()
            .unwrap()
            .contains("resolution"));
        object.transform.scale.x = 0.;
        assert!(
            build(&[object.clone()], object.uuid, 16, &cancel, &progress)
                .err()
                .unwrap()
                .contains("invertible")
        );
        object.transform = Default::default();
        object.repetition.enabled = true;
        assert!(
            !build(&[object.clone()], object.uuid, 16, &cancel, &progress)
                .unwrap()
                .triangles
                .is_empty()
        );
        object.repetition.enabled = false;
        object.softness = 0.1;
        let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
        child.boolean_parent = Some(object.uuid);
        assert!(!build(
            &[object.clone(), child.clone()],
            object.uuid,
            16,
            &cancel,
            &progress
        )
        .unwrap()
        .triangles
        .is_empty());
        object.softness = 0.;
        object.boolean_parent = Some(child.uuid);
        child.softness = 0.;
        assert!(build(
            &[object.clone(), child],
            object.uuid,
            16,
            &cancel,
            &progress
        )
        .err()
        .unwrap()
        .contains("cycle"));
    }
    #[test]
    fn curved_union_retains_both_source_material_owners() {
        let mut left = SdfObject::create_kind(PrimitiveKind::Sphere);
        left.softness = 0.0;
        let mut right = left.clone();
        right.uuid = Uuid::new_v4();
        right.boolean_parent = Some(left.uuid);
        right.transform.translation.x = 0.25;
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let mesh = build(
            &[left.clone(), right.clone()],
            left.uuid,
            24,
            &cancel,
            &progress,
        )
        .unwrap();
        assert!(manifold(&mesh).unwrap().is_manifold());
        assert!(mesh.owners.contains(&left.uuid));
        assert!(mesh.owners.contains(&right.uuid));
        assert!(mesh
            .owners
            .iter()
            .all(|&owner| owner == left.uuid || owner == right.uuid));
        for (index, triangle) in mesh.triangles.iter().enumerate() {
            let [a, b, c] = triangle.map(|vertex| mesh.positions[vertex as usize]);
            let center = (a + b + c) / 3.0;
            if center.x < -0.01 {
                assert_eq!(mesh.owners[index], left.uuid);
            }
            if center.x > 0.26 {
                assert_eq!(mesh.owners[index], right.uuid);
            }
        }
    }
    #[test]
    fn sphere_union_common_resolutions_and_offsets_are_robust() {
        for resolution in [16, 24, 32] {
            for offset in [0.0, 0.25, 0.5, 1.0, 1.5] {
                let mut left = SdfObject::create_kind(PrimitiveKind::Sphere);
                left.softness = 0.0;
                left.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 1.0 });
                let mut right = left.clone();
                right.uuid = Uuid::new_v4();
                right.boolean_parent = Some(left.uuid);
                right.transform.translation.x = offset;
                let cancel = AtomicBool::new(false);
                let progress = AtomicU32::new(0);
                let mesh = build(
                    &[left.clone(), right],
                    left.uuid,
                    resolution,
                    &cancel,
                    &progress,
                )
                .unwrap_or_else(|error| {
                    panic!("sphere union resolution{resolution} offset{offset}: {error}")
                });
                assert!(manifold(&mesh).unwrap().is_manifold());
                assert!(volume(&mesh) > 0.0);
            }
        }
    }
    #[test]
    fn disjoint_identical_and_contained_csg_return_correct_empty_results() {
        let mut left = SdfObject::create_kind(PrimitiveKind::Sphere);
        left.softness = 0.0;
        left.params = SdfParams::SphereParams(crate::model::SphereParams { radius: 1.0 });
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let source = primitives::build(&[left.clone()], &left, 16, &cancel).unwrap();
        let source_volume = volume(&source);
        let mut inner=left.clone();inner.params=SdfParams::SphereParams(crate::model::SphereParams{radius:0.5});
        let inner_volume=volume(&primitives::build(&[inner.clone()],&inner,16,&cancel).unwrap());
        for (offset, radius, operation, expected_empty, expected_volume) in [
            (3.0, 1.0, BooleanOperation::Intersect, true, 0.),
            (3.0, 1.0, BooleanOperation::Subtract, false, source_volume),
            (3.0, 1.0, BooleanOperation::Union, false, source_volume * 2.),
            (0.0, 1.0, BooleanOperation::Subtract, true, 0.),
            (0.0, 1.0, BooleanOperation::Intersect, false, source_volume),
            (0.0, 1.0, BooleanOperation::Union, false, source_volume),
            (0.0, 2.0, BooleanOperation::Subtract, true, 0.),
            (
                0.0,
                0.5,
                BooleanOperation::Intersect,
                false,
                inner_volume,
            ),
            (
                0.0,
                0.5,
                BooleanOperation::Subtract,
                false,
                source_volume - inner_volume,
            ),
        ] {
            let mut right = left.clone();
            right.uuid = Uuid::new_v4();
            right.boolean_parent = Some(left.uuid);
            right.transform.translation.x = offset;
            right.operation = operation;
            right.params = SdfParams::SphereParams(crate::model::SphereParams { radius });
            let mesh = build(&[left.clone(), right], left.uuid, 16, &cancel, &progress).unwrap();
            assert_eq!(mesh.triangles.is_empty(), expected_empty);
            assert!(
                (volume(&mesh) - expected_volume).abs() < 1e-4,
                "volume {} expected {}",
                volume(&mesh),
                expected_volume
            );
            if !expected_empty {
                assert!(manifold(&mesh).unwrap().is_manifold());
            }
        }
    }
}
