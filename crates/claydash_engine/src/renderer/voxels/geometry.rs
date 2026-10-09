use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use glam::{IVec3, Vec3};
use uuid::Uuid;

use crate::model::{lattice_bounds, SdfObject};
use super::super::box_depth_atlas::{bake_box_depth_atlas_with_progress, BoxCaptureStart};
use super::super::poisson_mesh::geometry::Mesh;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct VoxelGeometry {
    pub mesh: Mesh,
    /// One flat sampled color per triangle.
    pub colors: Vec<[f32; 4]>,
    pub cubes: usize,
}

impl VoxelGeometry {
    /// Move the same component-local cubes used by the viewport into GLB world space.
    pub fn into_world(mut self, source: &[SdfObject], root: Uuid,
        cancel: &AtomicBool) -> Result<Self, String> {
        self.mesh = super::super::poisson_mesh::mesh_at_pose(self.mesh,
            crate::model::lattice_world_matrix(source, root), cancel)?;
        Ok(self)
    }
}

fn report_progress(progress: &AtomicU32, percent: u32) {
    progress.store(percent, Ordering::Relaxed);
    #[cfg(target_arch = "wasm32")]
    super::web_worker::post_progress(percent);
}

pub fn build(
    source: &[SdfObject], root: Uuid, resolution: u32,
    progress: &AtomicU32, cancel: Option<&AtomicBool>,
) -> Result<VoxelGeometry, String> {
    let cancelled = || cancel.is_some_and(|flag| flag.load(Ordering::Relaxed));
    if cancelled() { return Err("Voxel build cancelled".into()); }
    if !(8..=128).contains(&resolution) {
        return Err("Voxel resolution must be between 8 and 128".into());
    }
    let atlas = bake_box_depth_atlas_with_progress(
        source, root, resolution, BoxCaptureStart::OutsideBounds,
        |percent| {
            report_progress(progress, percent * 80 / 100);
            !cancelled()
        },
    ).ok_or_else(|| if cancelled() { "Voxel build cancelled" } else {
        "Unable to capture voxel surface"
    }.to_string())?;
    let (minimum, maximum) = lattice_bounds(source, root)
        .ok_or("Unable to determine voxel frame")?;
    let center = (minimum + maximum) * 0.5;
    let cell_size = (atlas.local_max - atlas.local_min).max_element() / resolution as f32;
    if !cell_size.is_finite() || cell_size <= 0.0 {
        return Err("Invalid voxel cell size".into());
    }
    let face_size = resolution as usize * resolution as usize;
    let mut cells = BTreeMap::new();
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    for (index, texel) in atlas.texels.iter().enumerate() {
        if index % 512 == 0 {
            budget.checkpoint();
            if cancelled() { return Err("Voxel build cancelled".into()); }
        }
        let Some(owner) = atlas.owners[index] else { continue };
        if texel[0] < 0.0 { continue; }
        let face = (index / face_size) % 6;
        let pixel = index % face_size;
        let (axis, u, v, sign) = face_axes(face);
        let mut point = Vec3::ZERO;
        point[axis] = if sign > 0.0 { atlas.local_max[axis] - texel[0] }
            else { atlas.local_min[axis] + texel[0] };
        point[u] = atlas.local_min[u] + ((pixel % resolution as usize) as f32 + 0.5)
            / resolution as f32 * (atlas.local_max[u] - atlas.local_min[u]);
        point[v] = atlas.local_min[v] + ((pixel / resolution as usize) as f32 + 0.5)
            / resolution as f32 * (atlas.local_max[v] - atlas.local_min[v]);
        let key = ((point - atlas.local_min) / cell_size).floor().as_ivec3().to_array();
        cells.entry(key).or_insert((owner, [texel[1], texel[2], texel[3], 1.0]));
    }
    report_progress(progress, 85);
    let geometry = cube_mesh(&cells, center + atlas.local_min, cell_size, || {
        budget.checkpoint();
        !cancelled()
    })?;
    report_progress(progress, 100);
    Ok(geometry)
}

fn face_axes(face: usize) -> (usize, usize, usize, f32) {
    match face {
        0 => (0, 1, 2, 1.0), 1 => (0, 1, 2, -1.0),
        2 => (1, 0, 2, 1.0), 3 => (1, 0, 2, -1.0),
        4 => (2, 0, 1, 1.0), 5 => (2, 0, 1, -1.0),
        _ => unreachable!("six box projections"),
    }
}

fn cube_mesh(
    cells: &BTreeMap<[i32; 3], (Uuid, [f32; 4])>, origin: Vec3, cell_size: f32,
    mut keep_working: impl FnMut() -> bool,
) -> Result<VoxelGeometry, String> {
    let mut mesh = Mesh { positions: Vec::new(), triangles: Vec::new(),
        owners: Vec::new(), normals: Vec::new() };
    let mut colors = Vec::new();
    for (index, (key, (owner, color))) in cells.iter().enumerate() {
        if index % 128 == 0 && !keep_working() { return Err("Voxel build cancelled".into()); }
        let minimum = origin + IVec3::from_array(*key).as_vec3() * cell_size;
        for face in 0..6 {
            let (axis, u, v, sign) = face_axes(face);
            let mut neighbor = *key;
            neighbor[axis] += sign as i32;
            if cells.contains_key(&neighbor) { continue; }
            // Uploaded raster vertices include position, normal and RGBA.
            // Bound the buffer to about 58 MiB, including adversarial captures.
            if mesh.triangles.len() >= 400_000 {
                return Err("Voxel mesh exceeds the 400,000 triangle memory budget; reduce resolution".into());
            }
            let mut a = minimum;
            if sign > 0.0 { a[axis] += cell_size; }
            let mut b = a; b[u] += cell_size;
            let mut c = b; c[v] += cell_size;
            let mut d = a; d[v] += cell_size;
            let mut normal = Vec3::ZERO; normal[axis] = sign;
            let vertices = if (b - a).cross(c - a).dot(normal) > 0.0 {
                [a, b, c, d]
            } else { [a, d, c, b] };
            let base = mesh.positions.len() as u32;
            mesh.positions.extend(vertices);
            mesh.triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
            mesh.normals.extend([[normal; 3]; 2]);
            mesh.owners.extend([*owner; 2]);
            colors.extend([*color; 2]);
        }
    }
    Ok(VoxelGeometry { mesh, colors, cubes: cells.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubes_have_exact_flat_faces_and_cull_shared_faces() {
        let cells = BTreeMap::from([
            ([0, 0, 0], (Uuid::nil(), [0.2, 0.4, 0.6, 1.0])),
            ([1, 0, 0], (Uuid::nil(), [0.8, 0.4, 0.6, 1.0])),
        ]);
        let result = cube_mesh(&cells, Vec3::splat(3.0), 0.5, || true).unwrap();
        assert_eq!(result.cubes, 2);
        assert_eq!(result.mesh.triangles.len(), 20);
        assert_eq!(result.colors.len(), 20);
        for (triangle, normals) in result.mesh.triangles.iter().zip(&result.mesh.normals) {
            let [a, b, c] = triangle.map(|index| result.mesh.positions[index as usize]);
            assert!((b - a).cross(c - a).dot(normals[0]) > 0.0);
            assert_eq!(normals[0].abs().element_sum(), 1.0);
            assert_eq!(*normals, [normals[0]; 3]);
        }
        for point in result.mesh.positions {
            assert!(point.cmpge(Vec3::splat(3.0)).all());
            assert!(point.cmple(Vec3::new(4.0, 3.5, 3.5)).all());
        }
    }

    #[test]
    fn sphere_captures_all_six_sides_and_keeps_color() {
        let mut sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        sphere.color = glam::Vec4::new(0.2, 0.4, 0.6, 1.0);
        let result = build(&[sphere.clone()], sphere.uuid, 12, &AtomicU32::new(0), None).unwrap();
        assert!(result.cubes > 12);
        for face in 0..6 {
            let (axis, _, _, sign) = face_axes(face);
            assert!(result.mesh.normals.iter().any(|normal| normal[0][axis] == sign));
        }
        assert!(result.colors.iter().all(|color| *color == [0.2, 0.4, 0.6, 1.0]));
        let atlas = bake_box_depth_atlas_with_progress(&[sphere.clone()], sphere.uuid,
            12, BoxCaptureStart::OutsideBounds, |_| true).unwrap();
        let captured_hits = atlas.owners.iter().filter(|owner| owner.is_some()).count();
        assert!(result.cubes < captured_hits, "overlapping box projections share cube cells");
    }

    #[test]
    fn group_capture_includes_child_geometry_and_material() {
        let root = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::new(2.0, 0.0, 0.0);
        child.color = glam::Vec4::new(0.12, 0.34, 0.56, 1.0);
        let result = build(&[root.clone(), child.clone()], root.uuid, 16,
            &AtomicU32::new(0), None).unwrap();
        assert!(result.mesh.owners.contains(&root.uuid));
        assert!(result.mesh.owners.contains(&child.uuid));
        assert!(result.colors.contains(&[0.12, 0.34, 0.56, 1.0]));
        assert!(result.mesh.positions.iter().any(|point| point.x > 2.0));
    }

    #[test]
    fn cancellation_stops_before_sampling_or_meshing() {
        let sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        assert!(build(&[sphere.clone()], sphere.uuid, 12, &AtomicU32::new(0),
            Some(&AtomicBool::new(true))).err().unwrap().contains("cancelled"));
        let cells = BTreeMap::from([([0, 0, 0], (Uuid::nil(), [1.0; 4]))]);
        assert!(cube_mesh(&cells, Vec3::ZERO, 1.0, || false).is_err());
    }
}
