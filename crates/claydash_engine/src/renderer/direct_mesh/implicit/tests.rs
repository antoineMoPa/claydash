use super::*;
use crate::model::{BooleanOperation, Lattice, PrimitiveKind, SphereParams, SurfaceInlay};
use std::sync::atomic::Ordering;

fn sphere(radius: f32) -> SdfObject {
    let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
    object.params = SdfParams::SphereParams(SphereParams { radius });
    object.softness = 0.0;
    object
}

fn closed(mesh: &Mesh) {
    let mut edges = HashMap::<(u32, u32), (usize, i32)>::new();
    for face in &mesh.triangles {
        let [a, b, c] = face.map(|index| mesh.positions[index as usize]);
        assert!(
            (b - a).cross(c - a).length_squared() > 0.0,
            "degenerate face"
        );
        for (a, b) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
            let edge = edges.entry((a.min(b), a.max(b))).or_default();
            edge.0 += 1;
            edge.1 += if a < b { 1 } else { -1 };
        }
    }
    assert!(!edges.is_empty());
    assert!(
        edges
            .values()
            .all(|&(count, orientation)| count == 2 && orientation == 0),
        "open or inconsistently wound surface"
    );
    assert_eq!(mesh.normals.len(), mesh.triangles.len());
    assert_eq!(mesh.owners.len(), mesh.triangles.len());
}

fn volume(mesh: &Mesh) -> f32 {
    mesh.triangles
        .iter()
        .map(|face| {
            let [a, b, c] = face.map(|index| mesh.positions[index as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

fn run(scene: &[SdfObject], root: Uuid, resolution: u32) -> Mesh {
    build(
        scene,
        root,
        resolution,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap()
}

#[test]
fn smooth_union_subtraction_intersection_are_closed_and_track_exact_fields() {
    for operation in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ] {
        let mut left = sphere(0.5);
        left.softness = 0.13;
        let mut right = sphere(0.5);
        right.boolean_parent = Some(left.uuid);
        right.transform.translation.x = 0.4;
        right.operation = operation;
        let scene = [left.clone(), right.clone()];
        let mesh = run(&scene, left.uuid, 24);
        closed(&mesh);
        assert!(volume(&mesh) > 0.0);
        let mut field = PreparedDirectField::new(&scene, left.uuid).unwrap();
        for &point in &mesh.positions {
            assert!(
                field.sample(point).0.abs() < 2e-4,
                "vertex is off source surface"
            );
        }
        assert!(mesh
            .owners
            .iter()
            .all(|&owner| owner == left.uuid || owner == right.uuid));
        if operation == BooleanOperation::Subtract {
            assert!(mesh.owners.iter().all(|&owner| owner == left.uuid));
        }
    }
}

#[test]
fn lattice_mesh_uses_deformed_field_and_preserves_world_frame() {
    let mut object = sphere(0.5);
    object.transform.translation = Vec3::new(2.0, -1.0, 0.5);
    object.transform.scale = Vec3::new(-1.0, 1.0, 1.0);
    let mut lattice = Lattice::new(Vec3::splat(-0.7), Vec3::splat(0.7), 2);
    for offset in &mut lattice.offsets {
        *offset = Vec3::new(0.15, 0.0, 0.0);
    }
    object.lattice = Some(lattice);
    let scene = [object.clone()];
    let mesh = run(&scene, object.uuid, 20);
    closed(&mesh);
    assert!(volume(&mesh) > 0.0);
    let mut field = PreparedDirectField::new(&scene, object.uuid).unwrap();
    assert!(mesh
        .positions
        .iter()
        .all(|&point| field.sample(point).0.abs() < 2e-4));
    assert!(mesh.positions.iter().all(|point| point.x > 1.0));
}

#[test]
fn deformed_ring_field_closes_without_edge_resolution_failure() {
    let mut ring = SdfObject::create_kind(PrimitiveKind::Torus);
    ring.params = SdfParams::TorusParams {
        major_radius: 0.38,
        minor_radius: 0.09,
    };
    let mut lattice = Lattice::new(Vec3::splat(-0.6), Vec3::splat(0.6), 3);
    for z in 0..3 {
        for y in 0..3 {
            for x in 0..3 {
                let index = lattice.index(x, y, z);
                lattice.offsets[index] = Vec3::new(
                    0.035 * (y as f32 - 1.0),
                    0.02 * (x as f32 - 1.0),
                    0.065 * (x as f32 - 1.0) * (y as f32 - 1.0),
                );
            }
        }
    }
    ring.lattice = Some(lattice);
    let scene = [ring.clone()];
    let mesh = run(&scene, ring.uuid, 32);
    closed(&mesh);
    let mut field = PreparedDirectField::new(&scene, ring.uuid).unwrap();
    assert!(mesh
        .positions
        .iter()
        .all(|&point| field.sample(point).0.abs() < 2e-4));
}

#[test]
fn practical_thin_inlay_has_both_closed_shell_surfaces() {
    let host = sphere(0.4);
    let mut shell = sphere(0.6);
    shell.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.0,
        thickness: 0.02,
    });
    let scene = [shell.clone(), host];
    let mesh = run(&scene, shell.uuid, 16);
    closed(&mesh);
    assert!(mesh.positions.iter().any(|point| point.length() < 0.385));
    assert!(mesh.positions.iter().any(|point| point.length() > 0.415));
    let expected = 4.0 * std::f32::consts::PI / 3.0 * (0.42f32.powi(3) - 0.38f32.powi(3));
    assert!((volume(&mesh) - expected).abs() < expected * 0.03);
    assert!(mesh.owners.iter().all(|&owner| owner == shell.uuid));
}

#[test]
fn anisotropic_rotated_thin_box_is_preserved_at_low_resolution() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(params) = &mut object.params {
        params.box_q = Vec3::new(1.0, 0.5, 0.0005);
    }
    object.softness = 0.0;
    object.transform.rotation = glam::Quat::from_rotation_y(0.7);
    let mesh = run(&[object.clone()], object.uuid, 8);
    closed(&mesh);
    assert!((volume(&mesh) - 0.002).abs() < 0.0003);
}

#[test]
fn unresolved_thin_shell_budget_and_cancellation_are_explicit() {
    let host = sphere(0.5);
    let mut shell = sphere(0.6);
    shell.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.0,
        thickness: 0.0001,
    });
    let scene = [shell.clone(), host];
    let error = build(
        &scene,
        shell.uuid,
        16,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .err()
    .unwrap();
    assert!(error.contains("thin features"));
    let progress = AtomicU32::new(0);
    let error = build(&scene, shell.uuid, 16, &AtomicBool::new(true), &progress)
        .err()
        .unwrap();
    assert!(error.contains("cancelled"));
    assert_eq!(progress.load(Ordering::Relaxed), 0);
}

#[test]
fn selected_child_inherits_ancestor_cage_without_cropping() {
    let mut parent = sphere(0.1);
    let mut child = sphere(0.2);
    child.boolean_parent = Some(parent.uuid);
    child.transform.translation.x = 2.0;
    let mut lattice = Lattice::new(Vec3::splat(-4.0), Vec3::splat(4.0), 2);
    for offset in &mut lattice.offsets {
        *offset = Vec3::new(0.7, 0.0, 0.0);
    }
    parent.lattice = Some(lattice);
    let scene = [parent, child.clone()];
    let mesh = run(&scene, child.uuid, 16);
    closed(&mesh);
    let min = mesh
        .positions
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |a, &b| a.min(b));
    let max = mesh
        .positions
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, &b| a.max(b));
    assert!(((min + max) * 0.5).distance(Vec3::new(2.7, 0.0, 0.0)) < 0.01);
}

#[test]
fn completed_linear_group_repetition_includes_distant_operands() {
    let mut parent = sphere(0.2);
    parent.repetition.enabled = true;
    parent.repetition.count = [3, 1, 1];
    parent.repetition.spacing = Vec3::new(4.0, 1.0, 1.0);
    let mut child = sphere(0.2);
    child.boolean_parent = Some(parent.uuid);
    child.transform.translation.x = 3.0;
    let scene = [parent.clone(), child];
    let mesh = run(&scene, parent.uuid, 16);
    closed(&mesh);
    assert!(mesh.positions.iter().any(|point| point.x > 7.1));
    assert!(mesh.positions.iter().any(|point| point.x < -4.1));
}

#[test]
fn hidden_tiny_smooth_group_placeholder_does_not_force_unneeded_sampling() {
    let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(params) = &mut parent.params {
        params.box_q = Vec3::splat(0.0001);
        params.corner_radius = 0.0;
    }
    parent.softness = 0.1;
    let mut child = sphere(0.5);
    child.boolean_parent = Some(parent.uuid);
    let scene = [parent.clone(), child.clone()];
    let mesh = super::super::build(
        &scene,
        parent.uuid,
        16,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap();
    closed(&mesh);
    assert!(mesh.owners.iter().all(|&owner| owner == child.uuid));
    assert!((volume(&mesh) - 4.0 * std::f32::consts::PI * 0.5f32.powi(3) / 3.0).abs() < 0.025);
    assert!(mesh
        .positions
        .iter()
        .all(|point| (point.length() - 0.5).abs() < 2e-4));
}

fn assert_production_field_surface(object: SdfObject, resolution: u32) {
    let scene = [object.clone()];
    let started = web_time::Instant::now();
    let mesh = super::super::build(
        &scene,
        object.uuid,
        resolution,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap();
    eprintln!(
        "production field {} resolution {resolution}: {:?}, {} faces",
        object.name,
        started.elapsed(),
        mesh.triangles.len()
    );
    closed(&mesh);
    assert!(volume(&mesh) > 0.0);
    assert!(mesh.owners.iter().all(|&owner| owner == object.uuid));
    let mut field = PreparedDirectField::new(&scene, object.uuid).unwrap();
    for &point in &mesh.positions {
        let residual = field.sample(point).0.abs();
        assert!(
            residual < 5e-4,
            "resolution {resolution}: source surface residual {residual} at {point:?}"
        );
    }
    for (face, normals) in mesh.triangles.iter().zip(&mesh.normals) {
        let [a, b, c] = face.map(|index| mesh.positions[index as usize]);
        let face_normal = (b - a).cross(c - a);
        assert!(
            normals
                .iter()
                .all(|normal| normal.is_finite() && normal.dot(face_normal) > 0.0),
            "corner normal disagrees with outward face winding"
        );
    }
}

#[test]
fn production_horn_and_spindle_tori_are_closed_accurate_field_meshes() {
    for resolution in [16, 24] {
        for (major_radius, minor_radius) in [(0.5, 0.5), (0.3, 0.5)] {
            let mut object = SdfObject::create_kind(PrimitiveKind::Torus);
            object.softness = 0.0;
            object.params = SdfParams::TorusParams {
                major_radius,
                minor_radius,
            };
            assert_production_field_surface(object, resolution);
        }
    }
}

#[test]
fn production_rounded_concave_prism_is_closed_accurate_field_mesh() {
    for resolution in [16, 24] {
        let mut object = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
        object.softness = 0.0;
        object.params = SdfParams::PolygonPrismParams(crate::model::PolygonPrismParams {
            vertices: vec![
                glam::Vec2::new(-0.8, -0.8),
                glam::Vec2::new(0.8, -0.8),
                glam::Vec2::new(0.8, 0.0),
                glam::Vec2::new(0.0, 0.0),
                glam::Vec2::new(0.0, 0.8),
                glam::Vec2::new(-0.8, 0.8),
            ],
            half_depth: 0.3,
            edge_softness: 0.08,
        });
        assert_production_field_surface(object, resolution);
    }
}

#[test]
fn nonlinear_inlay_leaf_even_linear_repetition_keeps_discontinuous_cell_caps() {
    let host = sphere(0.5);
    let mut object = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    object.softness = 0.0;
    object.params = SdfParams::PolygonPrismParams(crate::model::PolygonPrismParams {
        vertices: vec![
            glam::Vec2::new(0.1, -0.2),
            glam::Vec2::new(0.6, -0.2),
            glam::Vec2::new(0.6, 0.2),
            glam::Vec2::new(0.1, 0.2),
        ],
        half_depth: 0.2,
        edge_softness: 0.0,
    });
    object.repetition.enabled = true;
    object.repetition.count = [2, 1, 1];
    object.repetition.spacing = Vec3::ONE;
    object.surface_inlay = Some(SurfaceInlay {
        host: host.uuid,
        offset: 0.0,
        thickness: 0.1,
    });
    let scene = [object.clone(), host];
    let mesh = super::super::build(
        &scene,
        object.uuid,
        24,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap();
    closed(&mesh);
    assert!(volume(&mesh) > 0.0);
    assert!(mesh
        .positions
        .iter()
        .any(|point| (point.x - 0.5).abs() < 1e-5));
    // The source jumps at x=.5: a cap exists even though phi is never zero.
    let mut field = PreparedDirectField::new(&scene, object.uuid).unwrap();
    assert!(field.has_cell_discontinuities());
    assert!(field.sample(Vec3::new(0.499, 0.0, 0.0)).0 < 0.0);
    assert!(field.sample(Vec3::new(0.501, 0.0, 0.0)).0 > 0.0);
}
