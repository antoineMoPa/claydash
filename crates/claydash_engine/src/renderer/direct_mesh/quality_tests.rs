use super::*;
use crate::model::{PrimitiveKind, SdfParams, SheetPanelParams, SheetSurface};
use glam::{Vec2, Vec3};
use std::collections::BTreeMap;
fn mesh(object: &SdfObject, error: f32) -> Mesh {
    build_with_error(
        &[object.clone()],
        object.uuid,
        64,
        error,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap()
}
fn closed(mesh: &Mesh) {
    let mut edges = BTreeMap::new();
    for &[a, b, c] in &mesh.triangles {
        for (a, b) in [(a, b), (b, c), (c, a)] {
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    assert!(edges.values().all(|n| *n == 2));
}
#[test]
fn rounded_mirror_uses_curved_bands_and_respects_world_error() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(p) = &mut object.params {
        p.box_q = Vec3::new(0.045, 0.135, 0.095);
        p.corner_radius = 0.04;
    }
    let mesh = mesh(&object, 0.0005);
    closed(&mesh);
    assert!(
        mesh.triangles.len() < 2500,
        "{} triangles",
        mesh.triangles.len()
    );
    for face in &mesh.triangles {
        let [a, b, c] = face.map(|i| mesh.positions[i as usize]);
        for p in [(a + b + c) / 3., (a + b) / 2., (b + c) / 2., (c + a) / 2.] {
            assert!(
                object.distance(p).abs() < 0.0005,
                "surface error {}",
                object.distance(p)
            );
        }
    }
}
#[test]
fn tessellation_tracks_world_scale_without_subdividing_flat_faces() {
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    if let SdfParams::BoxParams(p) = &mut object.params {
        p.box_q = Vec3::new(0.04, 0.12, 0.08);
        p.corner_radius = 0.02;
    }
    let small = mesh(&object, 0.001);
    object.transform.scale = Vec3::splat(10.);
    let large = mesh(&object, 0.001);
    assert!(large.triangles.len() > small.triangles.len());
    if let SdfParams::BoxParams(p) = &mut object.params {
        p.corner_radius = 0.;
    }
    assert_eq!(mesh(&object, 0.00001).triangles.len(), 12);
}
#[test]
fn constant_and_affine_crowns_are_not_tessellated_like_curves() {
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    object.params = SdfParams::SheetPanelParams(SheetPanelParams {
        size: Vec2::new(8., 0.4),
        surface: Some(SheetSurface {
            heights: [0.04; 16],
        }),
        ..Default::default()
    });
    assert_eq!(mesh(&object, 0.0005).triangles.len(), 12);
    if let SdfParams::SheetPanelParams(p) = &mut object.params {
        for j in 0..4 {
            for i in 0..4 {
                p.surface.as_mut().unwrap().heights[j * 4 + i] = i as f32 * 0.05 + j as f32 * 0.02;
            }
        }
    }
    assert_eq!(mesh(&object, 0.0005).triangles.len(), 12);
}
#[test]
fn crown_refines_with_tolerance_and_keeps_closed_topology() {
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    object.params = SdfParams::SheetPanelParams(SheetPanelParams {
        size: Vec2::new(2., 1.),
        surface: Some(SheetSurface {
            heights: [
                0., 0., 0., 0., 0., 0.2, 0.2, 0., 0., 0.2, 0.2, 0., 0., 0., 0., 0.,
            ],
        }),
        ..Default::default()
    });
    let coarse = mesh(&object, 0.005);
    let fine = mesh(&object, 0.0005);
    closed(&coarse);
    closed(&fine);
    assert!(fine.triangles.len() > coarse.triangles.len());
    assert!(fine.triangles.len() < 10000);
}

#[test]
fn curved_tubes_follow_error_and_world_scale_without_uniform_resolution_grid() {
    let mut object = SdfObject::create_kind(PrimitiveKind::BezierCurve);
    if let SdfParams::BezierCurveParams(p) = &mut object.params {
        p.points = vec![Vec3::ZERO, Vec3::new(0.,1.,0.), Vec3::new(1.,2.,0.), Vec3::new(2.,2.,0.)];
    }
    object.path_extrusion = Some(crate::model::PathExtrusion { radius: 0.12, ..Default::default() });
    object.transform.scale = Vec3::splat(0.4);
    let coarse = mesh(&object, 0.004);
    let fine = mesh(&object, 0.0005);
    closed(&coarse);
    closed(&fine);
    assert!(fine.triangles.len() > coarse.triangles.len());
    assert!(fine.triangles.len() < 8_000, "{} triangles", fine.triangles.len());
    let curve = match &object.params { SdfParams::BezierCurveParams(p) => p, _ => unreachable!() };
    for face in &fine.triangles {
        let [a,b,c] = face.map(|i| fine.positions[i as usize] / 0.4);
        for point in [(a+b+c)/3., (a+b)/2., (b+c)/2., (c+a)/2.] {
            let error = (curve.closest(point).unwrap().0.distance(point) - 0.12).abs() * 0.4;
            assert!(error <= 0.00051, "world surface error {error}");
        }
    }
    object.transform.scale = Vec3::splat(4.0);
    assert!(mesh(&object, 0.0005).triangles.len() > fine.triangles.len());
}
