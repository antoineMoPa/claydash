use super::*;
use crate::model::{
    SheetBend, SheetEmboss, SheetPathEmboss, SheetPathProfile, SheetPathStation, SheetSurface,
};
use glam::Vec2;

fn export(panel: &SheetPanelParams) -> Mesh {
    build(panel, Uuid::nil(), 32, &AtomicBool::new(false)).unwrap()
}

fn closed(mesh: &Mesh) {
    assert!(!mesh.triangles.is_empty());
    let mut edges = BTreeMap::<_, (usize, i32)>::new();
    for &[a, b, c] in &mesh.triangles {
        let [pa, pb, pc] = [a, b, c].map(|i| mesh.positions[i as usize]);
        assert!((pb - pa).cross(pc - pa).length_squared() > 0.0);
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let entry = edges.entry(edge(a, b)).or_default();
            entry.0 += 1;
            entry.1 += if a < b { 1 } else { -1 };
        }
    }
    assert!(edges
        .values()
        .all(|&(count, winding)| count == 2 && winding == 0));
    assert_eq!(mesh.owners.len(), mesh.triangles.len());
    assert_eq!(mesh.normals.len(), mesh.triangles.len());
    assert!(
        volume(mesh) > 0.0,
        "outward orientation must enclose positive volume"
    );
}

fn volume(mesh: &Mesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|&t| {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize].as_dvec3());
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

#[test]
fn wheel_arch_samples_connect_locally_instead_of_forming_long_fans() {
    let panel = SheetPanelParams {
        size: Vec2::new(4.0, 2.0),
        cuts: vec![SheetCut::Circle { center: Vec2::new(-1.0, -0.9), radius: 0.5 },
                   SheetCut::Circle { center: Vec2::new(1.0, -0.9), radius: 0.5 }],
        surface: Some(SheetSurface { heights: [0.0,0.0,0.0,0.0,0.04,0.04,0.04,0.04,0.04,0.04,0.04,0.04,0.0,0.0,0.0,0.0] }),
        ..Default::default()
    };
    let mesh=export(&panel);
    closed(&mesh);
    let mut count=0;let mut slivers=0;
    for face in &mesh.triangles {
        let p=face.map(|i|mesh.positions[i as usize]);
        // Exclude extrusion walls, whose thinness is intentional.
        if (p[1]-p[0]).cross(p[2]-p[0]).z.abs()<1e-9 {continue;}
        count+=1;
        let lengths=[p[0].distance(p[1]),p[1].distance(p[2]),p[2].distance(p[0])];
        if lengths.into_iter().fold(0.0,f32::max)/lengths.into_iter().fold(f32::INFINITY,f32::min)>8.0 {slivers+=1;}
    }
    assert!((slivers as f32 / count as f32) < 0.05,"{slivers}/{count} long thin faces");
    assert!(mesh.triangles.len()<10_000,"{} triangles",mesh.triangles.len());
}

#[test]
fn thin_blank_preserves_exact_thickness_and_stage() {
    let mut panel = SheetPanelParams {
        thickness: 0.00001,
        stage: SheetStage::Blank,
        ..Default::default()
    };
    panel.cuts.push(SheetCut::Circle {
        center: Vec2::ZERO,
        radius: 0.25,
    });
    let mesh = export(&panel);
    closed(&mesh);
    assert_eq!(mesh.triangles.len(), 12);
    for p in &mesh.positions {
        assert_eq!(p.z.abs(), panel.thickness * 0.5);
    }
    assert!(
        (volume(&mesh) - panel.size.x as f64 * panel.size.y as f64 * panel.thickness as f64).abs()
            < 1e-12
    );
    panel.stage = SheetStage::Cut;
    let cut = export(&panel);
    closed(&cut);
    assert!(volume(&cut) < volume(&mesh));
}

#[test]
fn straight_cut_is_exact_and_hole_walls_are_outward() {
    let panel = SheetPanelParams {
        stage: SheetStage::Cut,
        cuts: vec![
            SheetCut::Straight {
                from: Vec2::new(0.5, -0.5),
                to: Vec2::new(0.5, 0.5),
            },
            SheetCut::Circle {
                center: Vec2::ZERO,
                radius: 0.2,
            },
        ],
        ..Default::default()
    };
    let mesh = export(&panel);
    closed(&mesh);
    assert!(mesh.positions.iter().all(|p| p.x <= 0.5));
    let expected_area = 1.5 - 0.5 * 32.0 * 0.2_f64.powi(2) * (std::f64::consts::TAU / 32.0).sin();
    assert!((volume(&mesh) - expected_area * panel.thickness as f64).abs() < 1e-7);
}

#[test]
fn overlapping_boundary_circles_and_disconnected_islands_are_closed() {
    for cuts in [
        vec![
            SheetCut::Circle {
                center: Vec2::new(0.8, 0.0),
                radius: 0.4,
            },
            SheetCut::Circle {
                center: Vec2::new(0.5, 0.2),
                radius: 0.35,
            },
        ],
        vec![SheetCut::Circle {
            center: Vec2::ZERO,
            radius: 0.7,
        }],
    ] {
        let panel = SheetPanelParams {
            cuts,
            stage: SheetStage::Cut,
            ..Default::default()
        };
        closed(&export(&panel));
    }
}

#[test]
fn parallel_and_directional_bends_use_native_forming_map() {
    for direction in [0.0, std::f32::consts::FRAC_PI_2] {
        let panel = SheetPanelParams {
            bends: vec![SheetBend {
                start: 0.0,
                radius: 0.3,
                angle: 1.0,
                direction,
            }],
            ..Default::default()
        };
        let mesh = export(&panel);
        closed(&mesh);
        assert!(mesh.positions.iter().any(|p| p.z > 0.1));
        assert!(
            mesh.positions.len() > 8,
            "bends must add samples beyond the blank corners"
        );
        let half = panel.size * 0.5;
        for x in [-half.x, half.x] {
            for y in [-half.y, half.y] {
                for z in [-panel.thickness * 0.5, panel.thickness * 0.5] {
                    let expected = panel.form_point(glam::Vec3::new(x, y, z));
                    assert!(mesh.positions.iter().any(|p| p.distance(expected) < 1e-7));
                }
            }
        }
        let analytic_volume = panel.size.x as f64 * panel.size.y as f64 * panel.thickness as f64;
        assert!((volume(&mesh) / analytic_volume - 1.0).abs() < 0.01);
        // With no relief, the paired faces stay one thickness apart under bends.
        let count = mesh.positions.len() / 2;
        for i in 0..count {
            assert!(
                (mesh.positions[i].distance(mesh.positions[i + count]) - panel.thickness).abs()
                    < 1e-6
            );
        }
    }
}

#[test]
fn compound_surface_emboss_and_path_relief_are_tessellated() {
    let panel = SheetPanelParams {
        surface: Some(SheetSurface { heights: [0.1; 16] }),
        embosses: vec![SheetEmboss::Circle {
            center: Vec2::ZERO,
            radius: 0.1,
            depth: 0.05,
            transition: 0.1,
        }],
        path_embosses: vec![SheetPathEmboss {
            stations: vec![
                SheetPathStation {
                    point: Vec2::new(-0.7, 0.3),
                    width: 0.1,
                    depth: 0.03,
                    profile: SheetPathProfile::Rounded,
                },
                SheetPathStation {
                    point: Vec2::new(0.7, 0.3),
                    width: 0.1,
                    depth: 0.03,
                    profile: SheetPathProfile::Crease,
                },
            ],
            taper_start: 0.2,
            taper_end: 0.2,
        }],
        ..Default::default()
    };
    let mesh = export(&panel);
    closed(&mesh);
    assert!(mesh.positions.iter().any(|p| p.z > 0.16));
    assert!(mesh
        .positions
        .iter()
        .any(|p| p.y > 0.25 && p.y < 0.35 && p.z > 0.14));
    let mut cut = panel.clone();
    cut.stage = SheetStage::Cut;
    assert!(export(&cut)
        .positions
        .iter()
        .all(|p| p.z.abs() == cut.thickness * 0.5));
}

#[test]
fn cancelled_and_fully_removed_panels_are_handled() {
    assert!(build(
        &SheetPanelParams::default(),
        Uuid::nil(),
        32,
        &AtomicBool::new(true)
    )
    .is_err());
    let panel = SheetPanelParams {
        cuts: vec![SheetCut::Circle {
            center: Vec2::ZERO,
            radius: 2.0,
        }],
        ..Default::default()
    };
    let mesh = export(&panel);
    assert!(mesh.positions.is_empty() && mesh.triangles.is_empty());
}

fn boolean_manifold(mesh: &Mesh) {
    let coordinates: Vec<f64> = mesh
        .positions
        .iter()
        .flat_map(|p| [p.x as f64, p.y as f64, p.z as f64])
        .collect();
    let indices: Vec<usize> = mesh
        .triangles
        .iter()
        .flatten()
        .map(|&i| i as usize)
        .collect();
    let manifold = boolmesh::prelude::Manifold::new(&coordinates, &indices)
        .expect("sheet output must be accepted as a Boolean operand");
    assert!(manifold.is_manifold());
}

#[test]
fn parallel_bend_refinement_tracks_curvature_without_subdividing_flat_sheet() {
    let mut panel = SheetPanelParams {
        thickness: 0.002,
        bends: vec![SheetBend {
            start: 0.4,
            radius: 0.2,
            angle: 0.8,
            direction: 0.0,
        }],
        ..Default::default()
    };
    // Both ends of the initial diagonal lie outside the curved zone. Its
    // interior must still be refined, including both tangent seam crossings.
    let mut material = vec![
        DVec2::new(-1.0, -0.5),
        DVec2::new(1.0, -0.5),
        DVec2::new(1.0, 0.5),
        DVec2::new(-1.0, 0.5),
    ];
    let mut faces = vec![[0, 1, 2], [0, 2, 3]];
    let strips = panel.strips();
    refine(
        &mut material,
        &mut faces,
        &panel,
        super::super::TessellationQuality::from_resolution(32),
        Some(&strips),
        &AtomicBool::new(false),
    )
    .unwrap();
    for &[a, b, c] in &faces {
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let a = material[a as usize];
            let b = material[b as usize];
            let map = |uv: DVec2| panel.form_point(uv.as_vec2().extend(panel.thickness * 0.5));
            let midpoint = map((a + b) * 0.5);
            let linear = (map(a) + map(b)) * 0.5;
            assert!(
                midpoint.distance(linear) < 0.0015,
                "curved edge deviates from native forming map: {a:?} to {b:?}"
            );
        }
    }
    panel.cuts.push(SheetCut::Circle {
        center: Vec2::new(0.2, 0.0),
        radius: 0.2,
    });
    let mesh = build(&panel, Uuid::nil(), 16, &AtomicBool::new(false)).unwrap();
    closed(&mesh);
    boolean_manifold(&mesh);
    // This quality setting formerly generated 38,400 faces by refining the
    // whole blank isotropically, even though material Y is exactly affine.
    assert!(
        mesh.triangles.len() < 5000,
        "flat zones were unnecessarily subdivided"
    );
}

#[test]
fn refined_bent_circular_openings_are_boolean_manifolds() {
    for circles in [
        vec![SheetCut::Circle {
            center: Vec2::new(0.2, 0.0),
            radius: 0.2,
        }],
        vec![
            SheetCut::Circle {
                center: Vec2::new(0.2, 0.0),
                radius: 0.2,
            },
            SheetCut::Circle {
                center: Vec2::new(0.35, 0.1),
                radius: 0.2,
            },
        ],
        vec![SheetCut::Circle {
            center: Vec2::new(0.3, 0.4),
            radius: 0.2,
        }],
    ] {
        let panel = SheetPanelParams {
            thickness: 0.002,
            cuts: circles,
            bends: vec![SheetBend {
                start: 0.0,
                radius: 0.3,
                angle: 1.0,
                direction: 0.0,
            }],
            ..Default::default()
        };
        let mesh = export(&panel);
        closed(&mesh);
        boolean_manifold(&mesh);
    }
}

#[test]
fn cylinder_subtracts_from_bent_circular_cut_sheet() {
    use crate::model::{BooleanOperation, PrimitiveKind, SdfObject, SdfParams};
    use std::sync::atomic::AtomicU32;
    let panel = SheetPanelParams {
        thickness: 0.002,
        cuts: vec![SheetCut::Circle {
            center: Vec2::new(0.2, 0.0),
            radius: 0.2,
        }],
        bends: vec![SheetBend {
            start: 0.0,
            radius: 0.3,
            angle: 1.0,
            direction: 0.0,
        }],
        ..Default::default()
    };
    let original = export(&panel);
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    object.params = SdfParams::SheetPanelParams(panel);
    object.softness = 0.0;
    let mut cutter = SdfObject::create_kind(PrimitiveKind::Cylinder);
    cutter.params = SdfParams::CylinderParams {
        radius: 0.12,
        half_height: 0.2,
    };
    cutter.boolean_parent = Some(object.uuid);
    cutter.operation = BooleanOperation::Subtract;
    cutter.softness = 0.0;
    cutter.transform.translation = glam::Vec3::new(-0.6, 0.0, 0.0);
    cutter.transform.rotation = glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let root = object.uuid;
    let mesh = crate::renderer::direct_mesh::build(
        &[object, cutter],
        root,
        32,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
    .unwrap();
    closed(&mesh);
    boolean_manifold(&mesh);
    assert!(volume(&mesh) < volume(&original) * 0.99);
    assert!(mesh.owners.iter().all(|&owner| owner == root));
}

#[test]
#[ignore = "advisory direct/Poisson timing comparison; run with --ignored --nocapture"]
fn benchmark_direct_sheet_against_poisson() {
    use crate::model::{PrimitiveKind, SdfObject, SdfParams};
    use std::sync::atomic::AtomicU32;
    let panel = SheetPanelParams {
        thickness: 0.002,
        cuts: vec![SheetCut::Circle {
            center: Vec2::new(0.2, 0.0),
            radius: 0.2,
        }],
        bends: vec![SheetBend {
            start: 0.4,
            radius: 0.2,
            angle: 0.8,
            direction: 0.0,
        }],
        ..Default::default()
    };
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    object.params = SdfParams::SheetPanelParams(panel.clone());
    let owner = object.uuid;
    let cancel = AtomicBool::new(false);
    let started = web_time::Instant::now();
    let direct = crate::renderer::direct_mesh::build(
        &[object.clone()],
        owner,
        16,
        &cancel,
        &AtomicU32::new(0),
    )
    .unwrap();
    let direct_ms = started.elapsed().as_secs_f64() * 1000.0;
    closed(&direct);
    let started = web_time::Instant::now();
    let reconstructed = crate::renderer::poisson_mesh::geometry::build(
        &[object],
        owner,
        16,
        16,
        None,
        &AtomicU32::new(0),
        Some(&cancel),
    );
    let poisson_ms = started.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "thin cut/bent sheet: direct resolution=16: {direct_ms:.2} ms, {} triangles, volume {:.6}",
        direct.triangles.len(),
        volume(&direct)
    );
    match reconstructed {
        Ok(mesh) => eprintln!("Poisson capture=16/cells=16: {poisson_ms:.2} ms, {} triangles, volume {:.6}; Poisson/direct elapsed ratio {:.2}",
            mesh.triangles.len(), volume(&mesh), poisson_ms / direct_ms),
        Err(error) => eprintln!("Poisson capture=16/cells=16: {poisson_ms:.2} ms, failed: {error}"),
    }
}
