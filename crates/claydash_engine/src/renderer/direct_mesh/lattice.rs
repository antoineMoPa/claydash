//! Surface deformation matching the native four-step cage inverse.
use super::{check_cancel, Mesh};
use crate::model::{Lattice, SdfObject, SdfParams};
use glam::{Mat3, Mat4, Vec3};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

struct Cage<'a> {
    lattice: &'a Lattice,
    offsets: Vec<Vec3>,
    frame: Mat4,
    inverse: Mat4,
}
impl Cage<'_> {
    fn native_rest(&self, point: Vec3) -> Vec3 {
        let mut rest = point;
        for _ in 0..4 {
            let next = point - self.lattice.displacement_with_offsets(rest, &self.offsets);
            let change = next - rest;
            rest = next;
            if change.length_squared() < 1e-8 {
                break;
            }
        }
        rest
    }
    fn deform(&self, world: Vec3) -> Result<Vec3, String> {
        let rest = self.inverse.transform_point3(world);
        let mut point = rest + self.lattice.displacement_with_offsets(rest, &self.offsets);
        let epsilon = (self.lattice.max - self.lattice.min).min_element() * 1e-4;
        for _ in 0..12 {
            let error = rest - self.native_rest(point);
            if error.length() <= 2e-6 {
                let result = self.frame.transform_point3(point);
                if result.is_finite() {
                    return Ok(result);
                }
                break;
            }
            let column = |axis: Vec3| {
                (self.native_rest(point + axis * epsilon)
                    - self.native_rest(point - axis * epsilon))
                    / (2.0 * epsilon)
            };
            let jacobian = Mat3::from_cols(column(Vec3::X), column(Vec3::Y), column(Vec3::Z));
            if !jacobian.is_finite() || jacobian.determinant() <= 1e-8 {
                break;
            }
            let step = jacobian.inverse() * error;
            let before = error.length_squared();
            let mut accepted = false;
            for factor in [1.0, 0.5, 0.25, 0.125] {
                let candidate = point + step * factor;
                if (rest - self.native_rest(candidate)).length_squared() < before {
                    point = candidate;
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                break;
            }
        }
        Err("Direct lattice cannot match native cage inversion to surface precision; reduce cage deformation".into())
    }
}

pub(super) fn apply(
    scene: &[SdfObject],
    object: &SdfObject,
    mesh: &mut Mesh,
    resolution: u32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut ancestor = Some(object.uuid);
    let mut cage = None;
    for _ in 0..scene.len() {
        let Some(id) = ancestor else { break };
        let Some(candidate) = scene.iter().find(|candidate| candidate.uuid == id) else {
            break;
        };
        if let Some(lattice) = &candidate.lattice {
            // A valid zero cage deliberately overrides a parent's deformation.
            let offsets = lattice.effective_offsets();
            if offsets.iter().all(|offset| *offset == Vec3::ZERO) {
                return Ok(());
            }
            let frame = crate::model::lattice_world_matrix(scene, candidate.uuid);
            cage = Some(Cage {
                lattice,
                offsets,
                frame,
                inverse: frame.inverse(),
            });
            break;
        }
        ancestor = candidate.boolean_parent;
    }
    let Some(cage) = cage else {
        return Ok(());
    };
    if mesh.triangles.is_empty() {
        return Ok(());
    }
    let (minimum, maximum) = mesh.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), point| {
            let point = cage.inverse.transform_point3(*point);
            (min.min(point), max.max(point))
        },
    );
    let target = ((maximum - minimum).max_element() / resolution.max(8) as f32)
        .min(
            (cage.lattice.max - cage.lattice.min).min_element()
                / (cage.lattice.resolution - 1) as f32
                * 0.25,
        )
        .max(1e-5);
    refine(
        mesh,
        &cage,
        object,
        crate::model::object_world_matrix(scene, object.uuid),
        target,
        cancel,
    )?;
    for (index, point) in mesh.positions.iter_mut().enumerate() {
        if index % 256 == 0 {
            check_cancel(cancel)?;
        }
        *point = cage.deform(*point)?;
    }
    mesh.normals.clear();
    Ok(())
}

fn edge(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn sheet_midpoint(object: &SdfObject, matrix: Mat4, mut point: Vec3) -> Vec3 {
    if object.repetition.enabled || !matches!(object.params, SdfParams::SheetPanelParams(_)) {
        return point;
    }
    let sample =
        |point| object.distance_with_matrix_with_geometry(point, matrix, false, None, None);
    let epsilon = 1e-5;
    for _ in 0..6 {
        let distance = sample(point);
        if distance.abs() <= 1e-6 {
            break;
        }
        let gradient = Vec3::new(
            sample(point + Vec3::X * epsilon) - sample(point - Vec3::X * epsilon),
            sample(point + Vec3::Y * epsilon) - sample(point - Vec3::Y * epsilon),
            sample(point + Vec3::Z * epsilon) - sample(point - Vec3::Z * epsilon),
        ) / (2.0 * epsilon);
        if !gradient.is_finite() || gradient.length_squared() < 1e-8 {
            break;
        }
        point -= gradient * (distance / gradient.length_squared());
    }
    point
}

/// Shared edge refinement preserves closed material faces without subdividing
/// through sheet thickness as a volume grid would.
fn refine(
    mesh: &mut Mesh,
    cage: &Cage<'_>,
    object: &SdfObject,
    matrix: Mat4,
    target: f32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    for _ in 0..24 {
        check_cancel(cancel)?;
        if mesh.triangles.len() > 1_000_000 {
            return Err(
                "Direct lattice surface exceeds one million faces; reduce mesh resolution".into(),
            );
        }
        let mut midpoints = BTreeMap::new();
        for (index, &[a, b, c]) in mesh.triangles.iter().enumerate() {
            if index % 1024 == 0 {
                check_cancel(cancel)?;
            }
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let pa = cage.inverse.transform_point3(mesh.positions[a as usize]);
                let pb = cage.inverse.transform_point3(mesh.positions[b as usize]);
                if pa.distance(pb) > target {
                    midpoints.entry(edge(a, b)).or_insert(0);
                }
            }
        }
        if midpoints.is_empty() {
            return Ok(());
        }
        for ((a, b), midpoint) in &mut midpoints {
            *midpoint = mesh.positions.len() as u32;
            let point = (mesh.positions[*a as usize] + mesh.positions[*b as usize]) * 0.5;
            mesh.positions.push(sheet_midpoint(object, matrix, point));
        }
        let mut faces = Vec::new();
        let mut owners = Vec::new();
        for (index, &[a, b, c]) in mesh.triangles.iter().enumerate() {
            let start = faces.len();
            let ab = midpoints.get(&edge(a, b)).copied();
            let bc = midpoints.get(&edge(b, c)).copied();
            let ca = midpoints.get(&edge(c, a)).copied();
            match (ab, bc, ca) {
                (None, None, None) => faces.push([a, b, c]),
                (Some(m), None, None) => faces.extend([[a, m, c], [m, b, c]]),
                (None, Some(m), None) => faces.extend([[b, m, a], [m, c, a]]),
                (None, None, Some(m)) => faces.extend([[c, m, b], [m, a, b]]),
                (Some(m), Some(n), None) => faces.extend([[b, n, m], [a, m, c], [m, n, c]]),
                (None, Some(m), Some(n)) => faces.extend([[c, n, m], [b, m, a], [m, n, a]]),
                (Some(n), None, Some(m)) => faces.extend([[a, n, m], [c, m, b], [m, n, b]]),
                (Some(m), Some(n), Some(p)) => {
                    faces.extend([[a, m, p], [m, b, n], [p, n, c], [m, n, p]])
                }
            }
            owners.extend(std::iter::repeat_n(mesh.owners[index], faces.len() - start));
        }
        mesh.triangles = faces;
        mesh.owners = owners;
    }
    Err("Direct lattice surface refinement did not converge".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mirror, PrimitiveKind, SheetBend, SheetCut, SheetPanelParams};
    use glam::Vec2;
    use std::sync::atomic::AtomicU32;

    fn variable_cage() -> Lattice {
        let mut cage = Lattice::new(Vec3::new(-1.2, -0.8, -0.4), Vec3::new(1.2, 0.8, 0.8), 3);
        for z in 0..3 {
            for y in 0..3 {
                for x in 0..3 {
                    cage.offsets[x + y * 3 + z * 9] = Vec3::new(
                        0.03 * y as f32,
                        0.015 * z as f32,
                        0.045 * (x as f32 - 1.0) * (y as f32 - 1.0),
                    );
                }
            }
        }
        cage
    }

    #[test]
    fn forward_surface_matches_native_four_step_inverse() {
        let lattice = variable_cage();
        let cage = Cage {
            offsets: lattice.effective_offsets(),
            lattice: &lattice,
            frame: Mat4::IDENTITY,
            inverse: Mat4::IDENTITY,
        };
        for x in -10..=10 {
            for y in -6..=6 {
                let rest = Vec3::new(x as f32 * 0.1, y as f32 * 0.1, 0.07);
                let point = cage.deform(rest).unwrap();
                assert!(cage.native_rest(point).distance(rest) < 1e-5);
            }
        }
    }

    #[test]
    fn thin_bent_cut_sheet_deforms_and_retains_source_field_vertices() {
        let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        object.params = SdfParams::SheetPanelParams(SheetPanelParams {
            thickness: 0.002,
            cuts: vec![SheetCut::Circle {
                center: Vec2::new(-0.4, 0.0),
                radius: 0.12,
            }],
            bends: vec![SheetBend {
                start: 0.0,
                radius: 0.3,
                angle: 0.8,
                direction: 0.0,
            }],
            ..Default::default()
        });
        object.lattice = Some(variable_cage());
        let cancel = AtomicBool::new(false);
        let scene = [object.clone()];
        let mut mesh = super::super::primitives::build(&scene, &object, 32, &cancel).unwrap();
        apply(&scene, &object, &mut mesh, 32, &cancel).unwrap();
        assert!(super::super::manifold(&mesh).unwrap().is_manifold());
        assert!(mesh.triangles.len() < 100_000);
        let mut field =
            super::super::fields::PreparedDirectField::new(&scene, object.uuid).unwrap();
        for point in &mesh.positions {
            assert!(
                field.sample(*point).0.abs() < 2e-5,
                "source field residual {}",
                field.sample(*point).0
            );
        }
        object.mirror = Some(Mirror::default());
        object.repetition.enabled = true;
        object.repetition.count = [3, 1, 1];
        object.repetition.spacing.x = 3.0;
        let combined = super::super::build(
            &[object.clone()],
            object.uuid,
            16,
            &cancel,
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!(super::super::manifold(&combined).unwrap().is_manifold());
        assert!(combined.triangles.len() < 200_000);
    }

    #[test]
    fn selected_child_uses_nearest_zero_cage_override() {
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.lattice = Some(variable_cage());
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(parent.uuid);
        child.lattice = Some(Lattice::new(Vec3::splat(-1.0), Vec3::splat(1.0), 2));
        let scene = [parent, child.clone()];
        let cancel = AtomicBool::new(false);
        let mut mesh = super::super::primitives::build(&scene, &child, 16, &cancel).unwrap();
        let original = mesh.positions.clone();
        apply(&scene, &child, &mut mesh, 16, &cancel).unwrap();
        assert_eq!(mesh.positions, original);
    }
}
