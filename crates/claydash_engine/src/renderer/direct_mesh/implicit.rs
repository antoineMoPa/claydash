//! Closed indexed isosurfaces for nonlinear geometry fields. No reconstruction
//! or oriented point cloud is involved: every vertex lies on a field grid edge.
use super::super::poisson_mesh::geometry::Mesh;
use super::fields::PreparedDirectField;
use crate::model::{BooleanOperation, RepetitionMode, SdfObject, SdfParams};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use uuid::Uuid;

const MAX_GRID_POINTS: u64 = 16_000_000;
const MAX_FACES: usize = 2_000_000;
// Freudenthal decomposition: neighboring cubes share the same face diagonal.
const TETRAHEDRA: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 3, 2, 7],
    [0, 2, 6, 7],
    [0, 6, 4, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
];

struct Grid {
    minimum: Vec3,
    step: Vec3,
    cells: [usize; 3],
    frame: Mat4,
}

impl Grid {
    fn point(&self, x: usize, y: usize, z: usize) -> Vec3 {
        self.frame
            .transform_point3(self.minimum + self.step * Vec3::new(x as f32, y as f32, z as f32))
    }
    fn id(&self, x: usize, y: usize, z: usize) -> u64 {
        ((z * (self.cells[1] + 1) + y) * (self.cells[0] + 1) + x) as u64
    }
}

fn belongs(scene: &[SdfObject], object: &SdfObject, root: Uuid) -> bool {
    let mut ancestor = Some(object.uuid);
    for _ in 0..scene.len() {
        let Some(id) = ancestor else { break };
        if id == root {
            return true;
        }
        ancestor = scene
            .iter()
            .find(|node| node.uuid == id)
            .and_then(|node| node.boolean_parent);
    }
    false
}

/// Thin planar features require refinement along their own normal. Curved
/// shells require it in all directions. Refuse requests beyond the budget
/// rather than dropping a shell that falls between grid samples.
fn primitive_hidden_by_union_child(scene: &[SdfObject], object: &SdfObject) -> bool {
    // This is a visibility proof for sampling hints only. Keep the operand in
    // the field itself, and refuse to infer visibility through nonlinear cages.
    let mut ancestor = Some(object.uuid);
    for _ in 0..scene.len() {
        let Some(node) = ancestor.and_then(|id| scene.iter().find(|node| node.uuid == id)) else {
            break;
        };
        if node.lattice.is_some()
            || node.mirror.is_some()
            || node.repetition.enabled
            || node.surface_inlay.is_some()
        {
            return false;
        }
        ancestor = node.boolean_parent;
    }
    let children: Vec<_> = scene
        .iter()
        .filter(|child| child.boolean_parent == Some(object.uuid))
        .collect();
    if children.is_empty()
        || children
            .iter()
            .any(|child| child.operation != BooleanOperation::Union)
    {
        return false;
    }
    let local_extent = match &object.params {
        SdfParams::BoxParams(p) => p.box_q + Vec3::splat(p.corner_radius.max(0.0)),
        SdfParams::SphereParams(p) => Vec3::splat(p.radius),
        _ => return false,
    };
    let matrix = crate::model::object_world_matrix(scene, object.uuid);
    let distance_scale = matrix
        .x_axis
        .truncate()
        .length()
        .min(matrix.y_axis.truncate().length())
        .min(matrix.z_axis.truncate().length());
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return false;
    }
    let local_extent = local_extent + Vec3::splat(object.softness.max(0.0) / distance_scale);
    let center = matrix.transform_point3(Vec3::ZERO);
    let half = matrix.x_axis.truncate().abs() * local_extent.x
        + matrix.y_axis.truncate().abs() * local_extent.y
        + matrix.z_axis.truncate().abs() * local_extent.z;
    children.iter().any(|child| {
        if child.lattice.is_some()
            || child.mirror.is_some()
            || child.repetition.enabled
            || child.surface_inlay.is_some()
            || child
                .image_stencil
                .as_ref()
                .is_some_and(|stencil| stencil.image_plane)
            || scene
                .iter()
                .any(|node| node.boolean_parent == Some(child.uuid))
            || !matches!(
                child.params,
                SdfParams::BoxParams(_) | SdfParams::SphereParams(_)
            )
        {
            return false;
        }
        let Ok(mut field) = PreparedDirectField::new(scene, child.uuid) else {
            return false;
        };
        // A convex solid containing all eight corners contains the entire AABB.
        (0..8).all(|corner| {
            let signs = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            field.sample(center + half * signs).0 < -object.softness.max(0.0)
        })
    })
}

fn feature_steps(scene: &[SdfObject], root: Uuid, inverse: Mat4, base: Vec3) -> Vec3 {
    let mut step = base;
    for object in scene.iter().filter(|object| belongs(scene, object, root)) {
        if primitive_hidden_by_union_child(scene, object) {
            continue;
        }
        let matrix = inverse * crate::model::object_world_matrix(scene, object.uuid);
        let mut constrain_axis = |axis: usize, thickness: f32| {
            if thickness <= 0.0 || !thickness.is_finite() {
                return;
            }
            // The inverse transpose maps a plane normal without inventing a
            // normalization function or changing the transform semantics.
            let normal = matrix
                .inverse()
                .transpose()
                .transform_vector3(Vec3::AXES[axis]);
            for coordinate in 0..3 {
                if normal[coordinate].abs() > 1e-5 {
                    step[coordinate] =
                        step[coordinate].min(thickness / (4.0 * normal[coordinate].abs()));
                }
            }
        };
        match &object.params {
            SdfParams::SphereParams(p) => {
                for axis in 0..3 {
                    constrain_axis(axis, 2.0 * p.radius);
                }
            }
            SdfParams::BoxParams(p) => {
                for axis in 0..3 {
                    constrain_axis(axis, 2.0 * p.box_q[axis]);
                }
            }
            SdfParams::CylinderParams {
                radius,
                half_height,
            } => {
                constrain_axis(0, 2.0 * radius);
                constrain_axis(1, 2.0 * half_height);
                constrain_axis(2, 2.0 * radius);
            }
            SdfParams::TorusParams { minor_radius, .. } => {
                for axis in 0..3 {
                    constrain_axis(axis, 2.0 * minor_radius);
                }
            }
            SdfParams::SheetPanelParams(p) => {
                constrain_axis(2, p.thickness);
                if !p.bends.is_empty()
                    || p.surface.is_some()
                    || !p.embosses.is_empty()
                    || !p.path_embosses.is_empty()
                {
                    // Forming tilts the thin faces away from the original normal.
                    constrain_axis(0, p.thickness);
                    constrain_axis(1, p.thickness);
                }
            }
            SdfParams::PolygonPrismParams(p) => constrain_axis(2, 2.0 * p.half_depth),
            _ => {}
        }
        if let Some(inlay) = object.surface_inlay {
            // PreparedSubtreeSampler compares the host's world-distance field
            // against inlay.thickness (a half-width), regardless of host scale.
            let frame = inverse.inverse();
            let frame_scale = frame
                .x_axis
                .truncate()
                .length()
                .max(frame.y_axis.truncate().length())
                .max(frame.z_axis.truncate().length());
            step = step.min(Vec3::splat(inlay.thickness.abs() / (1.5 * frame_scale)));
        }
    }
    step
}

pub(super) fn build(
    scene: &[SdfObject],
    root: Uuid,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Mesh, String> {
    super::check_cancel(cancel)?;
    let mut field = PreparedDirectField::new(scene, root)?;
    let frame = crate::model::lattice_world_matrix(scene, root);
    if !frame.is_finite() || frame.determinant().abs() < 1e-12 {
        return Err("Direct field mesh requires a finite, invertible transform".into());
    }
    let (mut minimum, mut maximum) = crate::model::capture_bounds(scene, root)
        .ok_or_else(|| "Direct field mesh has no finite bounds".to_owned())?;
    // Polynomial smooth operators can extend beyond primitive bounds. Summing
    // their radii also covers nested blends, without clipping their surfaces.
    let inverse = frame.inverse();
    let blend_padding: f32 = scene
        .iter()
        .filter(|object| belongs(scene, object, root))
        .filter(|object| {
            scene
                .iter()
                .any(|child| child.boolean_parent == Some(object.uuid))
        })
        .map(|object| object.softness.max(0.0))
        .sum();
    let scale = inverse
        .x_axis
        .truncate()
        .length()
        .max(inverse.y_axis.truncate().length())
        .max(inverse.z_axis.truncate().length());
    minimum -= Vec3::splat(blend_padding * scale);
    maximum += Vec3::splat(blend_padding * scale);
    let displacement = field.deformation_extent(inverse);
    minimum -= displacement;
    maximum += displacement;
    // capture_bounds expands linear repetition on each primitive itself. For
    // completed groups the children also repeat in the owning group's frame.
    for object in scene.iter().filter(|object| belongs(scene, object, root)) {
        if object.repetition.enabled
            && object.repetition.mode == RepetitionMode::Linear
            && scene
                .iter()
                .any(|child| child.boolean_parent == Some(object.uuid))
        {
            let copy_extent = Vec3::from_array(std::array::from_fn(|axis| {
                object.repetition.count[axis].saturating_sub(1) as f32
                    * object.repetition.spacing[axis].max(0.001)
                    * 0.5
            }));
            let copy_frame = inverse * crate::model::group_world_matrix(scene, object.uuid);
            let padding = copy_frame.x_axis.truncate().abs() * copy_extent.x
                + copy_frame.y_axis.truncate().abs() * copy_extent.y
                + copy_frame.z_axis.truncate().abs() * copy_extent.z;
            minimum -= padding;
            maximum += padding;
        }
    }
    let extent = maximum - minimum;
    if !extent.is_finite() || extent.min_element() <= 0.0 {
        return Err("Direct field mesh has invalid sampling bounds".into());
    }
    let base = Vec3::splat(extent.max_element() / resolution.max(8) as f32);
    let step = feature_steps(scene, root, inverse, base).min(extent / 8.0);
    if !step.is_finite() || step.min_element() <= 0.0 {
        return Err("Direct field mesh has invalid feature thickness".into());
    }
    let cells = std::array::from_fn(|axis| (extent[axis] / step[axis]).ceil() as usize);
    let points = cells.iter().try_fold(1u64, |count, &axis| {
        count.checked_mul((axis as u64).checked_add(1)?)
    });
    if points.is_none_or(|points| points > MAX_GRID_POINTS) {
        return Err(format!("Direct field mesh needs more than {MAX_GRID_POINTS} grid samples to preserve its thin features; split the group or increase feature thickness"));
    }
    let grid = Grid {
        minimum,
        step: extent / Vec3::from_array(cells.map(|axis| axis as f32)),
        cells,
        frame,
    };
    extract(&mut field, &grid, cancel, progress)
}

#[derive(Clone, Copy)]
struct Corner {
    id: u64,
    point: Vec3,
    distance: f32,
}

fn extract(
    field: &mut PreparedDirectField<'_>,
    grid: &Grid,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Mesh, String> {
    let [nx, ny, nz] = grid.cells;
    let width = nx + 1;
    let plane_size = width * (ny + 1);
    let mut lower = vec![0.0f32; plane_size];
    let mut upper = vec![0.0f32; plane_size];
    let mut mesh = Mesh {
        positions: vec![],
        triangles: vec![],
        owners: vec![],
        normals: vec![],
    };
    let mut edges = HashMap::<(u64, u64), u32>::new();
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    // A tiny consistent level offset avoids ambiguous exact-zero grid corners.
    let world_extent = grid.frame.x_axis.truncate().abs() * grid.step.x * nx as f32
        + grid.frame.y_axis.truncate().abs() * grid.step.y * ny as f32
        + grid.frame.z_axis.truncate().abs() * grid.step.z * nz as f32;
    let bias = (grid.step.min_element() * 1e-4).max(world_extent.max_element() * 1e-6);
    for z in 0..=nz {
        for y in 0..=ny {
            super::check_cancel(cancel)?;
            budget.checkpoint();
            for x in 0..=nx {
                let distance = field.sample(grid.point(x, y, z)).0 + bias;
                if !distance.is_finite() {
                    return Err("Direct field mesh encountered a non-finite distance".into());
                }
                if (x == 0 || x == nx || y == 0 || y == ny || z == 0 || z == nz) && distance < 0.0 {
                    return Err("Direct field mesh surface exceeds its capture bounds; split the deformed group".into());
                }
                upper[y * width + x] = distance;
            }
        }
        if z > 0 {
            for y in 0..ny {
                super::check_cancel(cancel)?;
                budget.checkpoint();
                for x in 0..nx {
                    let corners: [Corner; 8] = std::array::from_fn(|corner| {
                        let dx = corner & 1;
                        let dy = (corner >> 1) & 1;
                        let dz = (corner >> 2) & 1;
                        Corner {
                            id: grid.id(x + dx, y + dy, z - 1 + dz),
                            point: grid.point(x + dx, y + dy, z - 1 + dz),
                            distance: if dz == 0 {
                                lower[(y + dy) * width + x + dx]
                            } else {
                                upper[(y + dy) * width + x + dx]
                            },
                        }
                    });
                    if corners.iter().all(|corner| corner.distance >= 0.0)
                        || corners.iter().all(|corner| corner.distance < 0.0)
                    {
                        continue;
                    }
                    for tetrahedron in TETRAHEDRA {
                        polygonize(
                            tetrahedron.map(|corner| corners[corner]),
                            field,
                            bias,
                            &mut edges,
                            &mut mesh,
                        )?;
                    }
                    if mesh.triangles.len() > MAX_FACES {
                        return Err(format!("Direct field mesh exceeded its {MAX_FACES} face budget; split the group or lower mesh resolution"));
                    }
                }
            }
        }
        std::mem::swap(&mut lower, &mut upper);
        // A crossing edge cannot be reused after its final z plane was consumed.
        let first_live = grid.id(0, 0, z);
        edges.retain(|&(a, b), _| a >= first_live && b >= first_live);
        super::report(progress, ((z + 1) * 190 / (nz + 1)) as u32);
    }
    super::check_cancel(cancel)?;
    if mesh.triangles.is_empty() {
        return Err("Direct field mesh found no resolved surface; the Boolean result may be empty or features require finer resolution".into());
    }
    mesh.normals = super::super::poisson_mesh::geometry::corner_normals(
        &mesh.positions,
        &mesh.triangles,
        &mesh.owners,
    );
    super::report(progress, 200);
    Ok(mesh)
}

fn crossing(
    a: Corner,
    b: Corner,
    field: &mut PreparedDirectField<'_>,
    bias: f32,
    edges: &mut HashMap<(u64, u64), u32>,
    mesh: &mut Mesh,
) -> Result<u32, String> {
    let key = (a.id.min(b.id), a.id.max(b.id));
    if let Some(&vertex) = edges.get(&key) {
        return Ok(vertex);
    }
    let (mut inside, mut outside) = if a.distance < 0.0 { (a, b) } else { (b, a) };
    // A secant alone can stagnate on spindle/horn tori and nonlinear cage
    // fields. Every second iteration bisects; the intervening guarded secant
    // keeps at most 80% of the bracket, giving guaranteed geometric contraction.
    let tolerance = (a.point.distance(b.point) * 1e-5).max(bias * 0.5).max(1e-7);
    let mut best_point = inside.point.lerp(outside.point, 0.5);
    let mut best_residual = f32::INFINITY;
    let spatial_tolerance = (a.point.distance(b.point) * 1e-6)
        .max(a.point.abs().max(b.point.abs()).max_element() * f32::EPSILON * 2.0)
        .max(1e-7);
    let mut resolved_cell_cap = false;
    for iteration in 0..32 {
        let t = if iteration % 2 == 1 {
            0.5
        } else {
            (inside.distance / (inside.distance - outside.distance)).clamp(0.2, 0.8)
        };
        let point = inside.point.lerp(outside.point, t);
        let distance = field.sample(point).0 + bias;
        if !distance.is_finite() {
            return Err("Direct field mesh encountered a non-finite edge distance".into());
        }
        if distance.abs() < best_residual {
            best_point = point;
            best_residual = distance.abs();
        }
        if best_residual <= tolerance {
            break;
        }
        if distance < 0.0 {
            inside = Corner {
                point,
                distance,
                ..inside
            };
        } else {
            outside = Corner {
                point,
                distance,
                ..outside
            };
        }
        // Native round-before-clamp repetition can jump directly from inside
        // to outside at a cell boundary. That cap has no zero field value.
        // Only explicitly flagged discontinuous fields can use spatial closure;
        // continuous primitives still require the sampled residual above.
        if field.has_cell_discontinuities()
            && inside.point.distance(outside.point) <= spatial_tolerance
        {
            best_point = inside.point.lerp(outside.point, 0.5);
            resolved_cell_cap = true;
            break;
        }
    }
    if best_residual > tolerance && !resolved_cell_cap {
        return Err("Direct field mesh could not resolve an edge to surface accuracy; increase feature thickness or reduce deformation".into());
    }
    let vertex = u32::try_from(mesh.positions.len())
        .map_err(|_| "Direct field mesh exceeded vertex index capacity".to_owned())?;
    mesh.positions.push(best_point);
    edges.insert(key, vertex);
    Ok(vertex)
}

fn polygonize(
    corners: [Corner; 4],
    field: &mut PreparedDirectField<'_>,
    bias: f32,
    edges: &mut HashMap<(u64, u64), u32>,
    mesh: &mut Mesh,
) -> Result<(), String> {
    let inside: Vec<_> = (0..4).filter(|&i| corners[i].distance < 0.0).collect();
    let outside: Vec<_> = (0..4).filter(|&i| corners[i].distance >= 0.0).collect();
    if inside.is_empty() || outside.is_empty() {
        return Ok(());
    }
    let outward = outside.iter().map(|&i| corners[i].point).sum::<Vec3>() / outside.len() as f32
        - inside.iter().map(|&i| corners[i].point).sum::<Vec3>() / inside.len() as f32;
    let mut vertex = |a, b| crossing(corners[a], corners[b], field, bias, edges, mesh);
    let faces = match inside.len() {
        1 => vec![[
            vertex(inside[0], outside[0])?,
            vertex(inside[0], outside[1])?,
            vertex(inside[0], outside[2])?,
        ]],
        3 => vec![[
            vertex(outside[0], inside[0])?,
            vertex(outside[0], inside[1])?,
            vertex(outside[0], inside[2])?,
        ]],
        2 => {
            let ac = vertex(inside[0], outside[0])?;
            let ad = vertex(inside[0], outside[1])?;
            let bc = vertex(inside[1], outside[0])?;
            let bd = vertex(inside[1], outside[1])?;
            vec![[ac, bc, bd], [ac, bd, ad]]
        }
        _ => unreachable!(),
    };
    for mut face in faces {
        let [a, b, c] = face.map(|index| mesh.positions[index as usize]);
        let face_normal = (b - a).cross(c - a);
        if face_normal.length_squared() == 0.0 {
            return Err("Direct field mesh reached floating-point precision limits; move the group closer to the origin or increase feature thickness".into());
        }
        if face_normal.dot(outward) < 0.0 {
            face.swap(1, 2);
        }
        mesh.owners.push(field.sample((a + b + c) / 3.0).1);
        mesh.triangles.push(face);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
