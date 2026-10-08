use super::*;
use std::sync::{atomic::{AtomicBool, AtomicU32, Ordering}, Arc};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub owners: Vec<uuid::Uuid>,
    pub normals: Vec<[Vec3; 3]>,
}

impl Mesh {
    pub fn from_parts(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>,
        owners: Vec<uuid::Uuid>) -> Self {
        let normals = corner_normals(&positions, &triangles, &owners);
        Self { positions, triangles, owners, normals }
    }

    pub fn triangle(&self, index: usize) -> [Vec3; 3] {
        self.triangles[index].map(|corner| self.positions[corner as usize])
    }
}

#[derive(Clone)]
struct Sample {
    point: Vec3,
    normal: Vec3,
    owner: uuid::Uuid,
}

/// Angle-weighted corner normals. Only manifold edges join a smooth fan;
/// touching at one vertex and sharp creases stay separate.
pub(crate) fn corner_normals(positions: &[Vec3], triangles: &[[u32; 3]],
    _owners: &[uuid::Uuid]) -> Vec<[Vec3; 3]> {
    const CREASE_COSINE: f32 = 0.5; // 60 degrees.
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    let faces: Vec<Vec3> = triangles.iter().map(|triangle| {
        let [a, b, c] = triangle.map(|index| positions[index as usize]);
        (b - a).cross(c - a).normalize_or_zero()
    }).collect();
    let mut parent: Vec<usize> = (0..triangles.len() * 3).collect();
    let mut size = vec![1usize; parent.len()];
    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    fn join(parent: &mut [usize], size: &mut [usize], a: usize, b: usize) {
        let mut a = root(parent, a);
        let mut b = root(parent, b);
        if a == b { return; }
        if size[a] < size[b] { std::mem::swap(&mut a, &mut b); }
        parent[b] = a;
        size[a] += size[b];
    }
    let mut edges = Vec::with_capacity(triangles.len() * 3);
    for (face, triangle) in triangles.iter().enumerate() {
        if face % 128 == 0 { budget.checkpoint(); }
        for edge in 0..3 {
            let a = triangle[edge];
            let b = triangle[(edge + 1) % 3];
            edges.push((a.min(b), a.max(b), face));
        }
    }
    edges.sort_unstable();
    let mut first = 0;
    while first < edges.len() {
        if first % 384 == 0 { budget.checkpoint(); }
        let mut end = first + 1;
        while end < edges.len() && edges[end].0 == edges[first].0
            && edges[end].1 == edges[first].1 { end += 1; }
        if end == first + 2 {
            let a = edges[first].2;
            let b = edges[first + 1].2;
            if a != b && faces[a].dot(faces[b]) >= CREASE_COSINE {
                for vertex in [edges[first].0, edges[first].1] {
                    let corner_a = triangles[a].iter().position(|&index| index == vertex).unwrap();
                    let corner_b = triangles[b].iter().position(|&index| index == vertex).unwrap();
                    join(&mut parent, &mut size, a * 3 + corner_a, b * 3 + corner_b);
                }
            }
        }
        first = end;
    }
    let mut sums = vec![Vec3::ZERO; parent.len()];
    for (face, triangle) in triangles.iter().enumerate() {
        if face % 128 == 0 { budget.checkpoint(); }
        for corner in 0..3 {
            let point = positions[triangle[corner] as usize];
            let left = positions[triangle[(corner + 1) % 3] as usize] - point;
            let right = positions[triangle[(corner + 2) % 3] as usize] - point;
            let angle = left.cross(right).length().atan2(left.dot(right));
            let component = root(&mut parent, face * 3 + corner);
            sums[component] += faces[face] * angle;
        }
    }
    triangles.iter().enumerate().map(|(face, _)| {
        if face % 128 == 0 { budget.checkpoint(); }
        std::array::from_fn(|corner| {
            let sum = sums[root(&mut parent, face * 3 + corner)].normalize_or_zero();
            if sum.length_squared() > 0.5 { sum } else { faces[face] }
        })
    }).collect()
}

pub(crate) fn containing_root(source: &[SdfObject], requested: uuid::Uuid) -> uuid::Uuid {
    let mut root = requested;
    for _ in 0..source.len() {
        let Some(parent) = source.iter().find(|object| object.uuid == root)
            .and_then(|object| object.boolean_parent) else { break };
        root = parent;
    }
    root
}

pub fn mesh_pose_is_independent(source: &[SdfObject], root: uuid::Uuid) -> bool {
    if source.iter().find(|object| object.uuid == root).is_none_or(|object| object.boolean_parent.is_some()) {
        return false;
    }
    source.iter().all(|object| {
        let text_path = match &object.params { crate::model::SdfParams::TextParams(text) => text.path, _ => None };
        [text_path, object.path_extrusion.and_then(|path| path.profile_curve),
            object.surface_inlay.map(|inlay| inlay.host)].into_iter().flatten()
            .all(|reference| (containing_root(source, object.uuid) == root)
                == (containing_root(source, reference) == root))
    })
}

pub fn mesh_uses_object_pose(source: &[SdfObject], root: uuid::Uuid) -> bool {
    source.iter().find(|object| object.uuid == root).is_some_and(|object| {
        object.mirror.is_none() && object.surface_inlay.is_none()
            && !crate::model::has_boolean_children(source, root)
    })
}

pub(crate) fn mesh_world_frame(source: &[SdfObject], root: uuid::Uuid) -> glam::Mat4 {
    if mesh_uses_object_pose(source, root) { crate::model::object_world_matrix(source, root) }
    else { crate::model::group_world_matrix(source, root) }
}

pub(crate) fn belongs_to_component(source: &[SdfObject], object: uuid::Uuid,
    components: &std::collections::HashSet<uuid::Uuid>) -> bool {
    let mut current = Some(object);
    for _ in 0..source.len() {
        let Some(id) = current else { break };
        if components.contains(&id) { return true; }
        current = source.iter().find(|candidate| candidate.uuid == id)
            .and_then(|candidate| candidate.boolean_parent);
    }
    false
}

pub fn export_roots(source: &[SdfObject], selection: &[uuid::Uuid]) -> Vec<uuid::Uuid> {
    source.iter().filter(|object| {
        if selection.is_empty() { return object.boolean_parent.is_none(); }
        if !selection.contains(&object.uuid) { return false; }
        let mut parent = object.boolean_parent;
        for _ in 0..source.len() {
            let Some(id) = parent else { break };
            if selection.contains(&id) { return false; }
            parent = source.iter().find(|candidate| candidate.uuid == id)
                .and_then(|candidate| candidate.boolean_parent);
        }
        true
    }).map(|object| object.uuid).collect()
}

pub fn build(
    source: &[SdfObject],
    root: uuid::Uuid,
    resolution: u32,
    mesh_resolution: u32,
    cached: Option<Arc<super::super::box_depth_atlas::BoxDepthAtlas>>,
    progress: &AtomicU32,
    cancel: Option<&AtomicBool>,
) -> Result<Mesh, String> {
    // A viewport recompute and an export may be requested together. Keep their
    // CPU-heavy work serialized, and let a queued export cancel promptly.
    #[cfg(not(target_arch = "wasm32"))]
    static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[cfg(not(target_arch = "wasm32"))]
    let _build = loop {
        match BUILD.try_lock() {
            Ok(guard) => break guard,
            Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                    return Err("Export cancelled".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    };
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) { return Err("Export cancelled".into()); }
    let mut worker_budget = super::super::cooperative_work::WorkerBudget::new();
    let atlas = if let Some(atlas) = cached { atlas } else {
        let capture = super::super::box_depth_atlas::bake_box_depth_atlas_with_progress(
            source, root, resolution,
            super::super::box_depth_atlas::BoxCaptureStart::OutsideBounds,
            |percent| {
                worker_budget.checkpoint();
                let capture_progress = percent.min(100) * 99 / 100;
                progress.store(capture_progress, Ordering::Relaxed);
                #[cfg(target_arch = "wasm32")]
                super::web_worker::post_progress(capture_progress);
                !cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed))
            },
        ).ok_or_else(|| {
            if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
                "Export cancelled"
            } else {
                "Could not capture the requested Boolean component"
            }
        })?;
        Arc::new(capture)
    };
    progress.store(100, Ordering::Relaxed);
    #[cfg(target_arch = "wasm32")]
    super::web_worker::post_progress(100);
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) { return Err("Export cancelled".into()); }
    let samples = samples_from_atlas(source, root, &atlas);
    let mesh = reconstruct(samples, mesh_resolution, cancel, |fraction| {
        let value = 100 + (fraction.clamp(0.0, 1.0) * 99.0) as u32;
        progress.store(value, Ordering::Relaxed);
        #[cfg(target_arch = "wasm32")]
        super::web_worker::post_progress(value);
    })?;
    progress.store(200, Ordering::Relaxed);
    #[cfg(target_arch = "wasm32")]
    super::web_worker::post_progress(200);
    Ok(mesh)
}

fn samples_from_atlas(
    source: &[SdfObject], root: uuid::Uuid,
    atlas: &super::super::box_depth_atlas::BoxDepthAtlas,
) -> Vec<Sample> {
    let n = atlas.resolution as usize;
    let face_size = n * n;
    let matrix = crate::model::lattice_world_matrix(source, root);
    let normal_matrix = matrix.inverse().transpose();
    let bounds_center = crate::model::lattice_bounds(source, root)
        .map_or(Vec3::ZERO, |(minimum, maximum)| (minimum + maximum) * 0.5);
    let axes = [(0, 1, 2, 1.0), (0, 1, 2, -1.0),
        (1, 0, 2, 1.0), (1, 0, 2, -1.0),
        (2, 0, 1, 1.0), (2, 0, 1, -1.0)];
    let mut output = Vec::new();
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    for (index, owner) in atlas.owners.iter().enumerate() {
        if index % 512 == 0 { budget.checkpoint(); }
        let Some(owner) = owner else { continue };
        let depth = atlas.texels[index][0];
        if !depth.is_finite() || depth < 0.0 { continue; }
        let face = (index / face_size) % 6;
        let (axis, u_axis, v_axis, sign) = axes[face];
        let pixel = index % face_size;
        let mut local = Vec3::ZERO;
        local[axis] = if sign > 0.0 { atlas.local_max[axis] } else { atlas.local_min[axis] } - sign * depth;
        local[u_axis] = atlas.local_min[u_axis]
            + ((pixel % n) as f32 + 0.5) / n as f32 * (atlas.local_max[u_axis] - atlas.local_min[u_axis]);
        local[v_axis] = atlas.local_min[v_axis]
            + ((pixel / n) as f32 + 0.5) / n as f32 * (atlas.local_max[v_axis] - atlas.local_min[v_axis]);
        let point = matrix.transform_point3(bounds_center + local);
        let normal = normal_matrix.transform_vector3(atlas.normals[index]).normalize_or_zero();
        if point.is_finite() && normal.is_finite() && normal.length_squared() > 0.5 {
            output.push(Sample { point, normal, owner: *owner });
        }
    }
    output
}

struct KdNode {
    sample: usize,
    left: Option<usize>,
    right: Option<usize>,
    axis: usize,
}

fn kd_build(indices: &mut [usize], samples: &[Sample], nodes: &mut Vec<KdNode>, depth: usize) -> Option<usize> {
    if indices.is_empty() { return None; }
    let axis = depth % 3;
    let middle = indices.len() / 2;
    indices.select_nth_unstable_by(middle, |a, b| samples[*a].point[axis].total_cmp(&samples[*b].point[axis]));
    let index = nodes.len();
    nodes.push(KdNode { sample: indices[middle], left: None, right: None, axis });
    let (left, rest) = indices.split_at_mut(middle);
    let (_, right) = rest.split_at_mut(1);
    nodes[index].left = kd_build(left, samples, nodes, depth + 1);
    nodes[index].right = kd_build(right, samples, nodes, depth + 1);
    Some(index)
}

fn kd_nearest(point: Vec3, node: Option<usize>, nodes: &[KdNode], samples: &[Sample], best: &mut (usize, f32)) {
    let Some(index) = node else { return };
    let entry = &nodes[index];
    let delta = point - samples[entry.sample].point;
    let distance = delta.length_squared();
    if distance < best.1 { *best = (entry.sample, distance); }
    let axis_delta = delta[entry.axis];
    let (near, far) = if axis_delta <= 0.0 { (entry.left, entry.right) } else { (entry.right, entry.left) };
    kd_nearest(point, near, nodes, samples, best);
    if axis_delta * axis_delta < best.1 { kd_nearest(point, far, nodes, samples, best); }
}

fn reconstruct(samples: Vec<Sample>, mesh_resolution: u32,
    cancel: Option<&AtomicBool>, mut progress: impl FnMut(f32)) -> Result<Mesh, String> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
    if samples.len() < 3 { return Err("Too few valid oriented samples".into()); }
    let oriented: Vec<_> = samples.iter().map(|sample| screened_poisson::Sample {
        point: sample.point.to_array(), normal: sample.normal.to_array(),
    }).collect();
    let solved = screened_poisson::reconstruct(&oriented,
        screened_poisson::Settings { cells: mesh_resolution as usize,
            ..screened_poisson::Settings::default() },
        cancel, |fraction| progress(fraction * 0.9))?;
    let positions: Vec<Vec3> = solved.positions.into_iter().map(Vec3::from_array).collect();
    let solved_triangles = solved.triangles;
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
    let mut indices: Vec<_> = (0..samples.len()).collect();
    let mut nodes = Vec::with_capacity(samples.len());
    let kd_root = kd_build(&mut indices, &samples, &mut nodes, 0);
    let mut triangles = Vec::new();
    let mut owners = Vec::new();
    let mut budget = super::super::cooperative_work::WorkerBudget::new();
    let solved_face_count = solved_triangles.len();
    for (face, index) in solved_triangles.into_iter().enumerate() {
        if face % 128 == 0 {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
            budget.checkpoint();
            progress(0.9 + 0.1 * face as f32 / solved_face_count as f32);
        }
        let Some((&a, &b, &c)) = positions.get(index[0] as usize)
            .zip(positions.get(index[1] as usize))
            .zip(positions.get(index[2] as usize))
            .map(|((a, b), c)| (a, b, c)) else { continue };
        if !a.is_finite() || !b.is_finite() || !c.is_finite() || (b-a).cross(c-a).length_squared() < 1e-20 { continue; }
        let center = (a + b + c) / 3.0;
        let mut best = (0, f32::INFINITY);
        kd_nearest(center, kd_root, &nodes, &samples, &mut best);
        triangles.push(index);
        owners.push(samples[best.0].owner);
    }
    if triangles.is_empty() { return Err("Poisson reconstruction returned no valid triangles".into()); }
    progress(1.0);
    Ok(Mesh::from_parts(positions, triangles, owners))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_resolution_changes_output_with_identical_samples() {
        let owner = uuid::Uuid::new_v4();
        let samples: Vec<_> = (1..12).flat_map(|latitude| {
            let theta = std::f32::consts::PI * latitude as f32 / 12.0;
            (0..24).map(move |longitude| {
                let phi = std::f32::consts::TAU * longitude as f32 / 24.0;
                let point = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
                Sample { point, normal: point, owner }
            })
        }).collect();
        let coarse = reconstruct(samples.clone(), 16, None, |_| {}).unwrap();
        let fine = reconstruct(samples, 32, None, |_| {}).unwrap();
        assert!(fine.triangles.len() > coarse.triangles.len(),
            "a finer output grid should produce more faces from the same samples");
    }



    #[test]
    fn rust_solver_builds_outward_indexed_sphere() {
        let owner = uuid::Uuid::new_v4();
        let mut samples = Vec::new();
        for latitude in 1..12 {
            let theta = std::f32::consts::PI * latitude as f32 / 12.0;
            for longitude in 0..24 {
                let phi = std::f32::consts::TAU * longitude as f32 / 24.0;
                let point = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
                samples.push(Sample { point, normal: point, owner });
            }
        }
        let mesh = reconstruct(samples, 64, None, |_| {}).unwrap();
        assert!(mesh.triangles.len() > 100);
        assert!(mesh.positions.len() < mesh.triangles.len() * 3,
            "iso extraction should share vertices across neighboring triangles");
        let outward = mesh.triangles.iter().filter(|triangle| {
            let [a, b, c] = triangle.map(|index| mesh.positions[index as usize]);
            (b - a).cross(c - a).dot((a + b + c) / 3.0) > 0.0
        }).count();
        let mut edges = std::collections::HashMap::<(u32, u32), (usize, usize)>::new();
        for triangle in &mesh.triangles {
            for edge in 0..3 {
                let a = triangle[edge]; let b = triangle[(edge + 1) % 3];
                let entry = edges.entry((a.min(b), a.max(b))).or_default();
                entry.0 += 1;
                if a < b { entry.1 += 1; }
            }
        }
        let incoherent = edges.values().filter(|(count, forward)| *count == 2 && *forward != 1).count();
        assert!(outward * 10 > mesh.triangles.len() * 9,
            "most triangles should face the source normals: {outward}/{}, incoherent edges: {incoherent}", mesh.triangles.len());
    }

    #[test]
    fn rust_solver_builds_from_captured_sdf_sphere() {
        let sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let progress = AtomicU32::new(0);
        let mesh = build(&[sphere.clone()], sphere.uuid, 12, 64,
            None, &progress, None).unwrap();
        assert_eq!(progress.load(Ordering::Relaxed), 200);
        assert!(mesh.triangles.len() > 100);
        assert_eq!(mesh.triangles.len(), mesh.owners.len());
        assert!(mesh.owners.iter().all(|owner| *owner == sphere.uuid));
    }

    #[test]
    fn rust_solver_default_sphere_quality() {
        let sphere = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let started = std::time::Instant::now();
        let mesh = build(&[sphere.clone()], sphere.uuid, 64, 64, None, &AtomicU32::new(0), None).unwrap();
        let mut radii = Vec::with_capacity(mesh.triangles.len());
        let mut outward = 0;
        for triangle in &mesh.triangles {
            let [a,b,c] = triangle.map(|index| mesh.positions[index as usize]);
            let center = (a+b+c)/3.0;
            radii.push(center.length());
            if (b-a).cross(c-a).dot(center) > 0.0 { outward += 1; }
        }
        radii.sort_by(f32::total_cmp);
        eprintln!("Rust sphere64: {:?}, {} vertices, {} triangles, {outward} outward, radius p10/p50/p90 {}/{}/{}",
            started.elapsed(), mesh.positions.len(), mesh.triangles.len(),
            radii[radii.len()/10], radii[radii.len()/2], radii[radii.len()*9/10]);
        assert!(outward * 100 > mesh.triangles.len() * 99);
        assert!((radii[radii.len()/2]-0.25).abs() < 0.02);
    }

    fn folded_faces(degrees: f32, same_owner: bool) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<uuid::Uuid>) {
        let radians = degrees.to_radians();
        let positions = vec![Vec3::ZERO, Vec3::X, Vec3::Y,
            Vec3::new(0.0, -radians.cos(), -radians.sin())];
        let triangles = vec![[0, 1, 2], [1, 0, 3]];
        let first = uuid::Uuid::new_v4();
        let second = if same_owner { first } else { uuid::Uuid::new_v4() };
        (positions, triangles, vec![first, second])
    }

    #[test]
    fn adjacent_faces_smooth_below_sixty_degrees() {
        let (positions, triangles, owners) = folded_faces(30.0, true);
        let normals = corner_normals(&positions, &triangles, &owners);
        assert!((normals[0][0] - normals[1][1]).length() < 1e-5);
        assert!(normals[0][0].y < -0.1 && normals[0][0].z > 0.8);
        assert_eq!(normals[0][2], Vec3::Z);
    }

    #[test]
    fn sharp_edges_keep_separate_normals() {
        let (positions, triangles, owners) = folded_faces(90.0, true);
        let normals = corner_normals(&positions, &triangles, &owners);
        assert!((normals[0][0] - Vec3::Z).length() < 1e-5);
        assert!((normals[1][1] - Vec3::Z).length() > 0.25);
    }

    #[test]
    fn material_owner_changes_do_not_split_smooth_fan() {
        let (positions, triangles, owners) = folded_faces(30.0, false);
        let normals = corner_normals(&positions, &triangles, &owners);
        assert!((normals[0][0] - normals[1][1]).length() < 1e-5);
    }

    #[test]
    fn disconnected_vertex_fans_do_not_smooth() {
        let (mut positions, mut triangles, mut owners) = folded_faces(30.0, true);
        positions.extend([Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 1.0)]);
        triangles.push([0, 4, 5]);
        owners.push(owners[0]);
        let normals = corner_normals(&positions, &triangles, &owners);
        assert!((normals[2][0] - Vec3::NEG_X).length() < 1e-5);
        assert!(normals[0][0].z > 0.8);
    }
    #[test]
    fn nested_request_uses_containing_boolean_component() {
        let root = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        assert_eq!(containing_root(&[root.clone(), child.clone()], child.uuid), root.uuid);
        assert_eq!(export_roots(&[root.clone(), child.clone()], &[root.uuid, child.uuid]), vec![root.uuid]);
        assert_eq!(export_roots(&[root.clone(), child.clone()], &[child.uuid]), vec![child.uuid]);
        assert_eq!(export_roots(&[root.clone(), child], &[]), vec![root.uuid]);
    }

    #[test]
    fn default_scene_export_builds_every_root() {
        let source = crate::test_support::fixture_objects(crate::test_support::DEFAULT_DUCK.as_bytes());
        for root in export_roots(&source, &[]) {
            let object = source.iter().find(|object| object.uuid == root).unwrap();
            eprintln!("exporting {}", object.name);
            let mesh = build(&source, root, object.gaussian_splats.resolution,
                object.poisson_mesh.resolution,
                None, &AtomicU32::new(0), None)
                .unwrap_or_else(|error| panic!("{}: {error}", object.name));
            assert!(!mesh.triangles.is_empty());
        }
    }

    #[test]
    fn asymmetric_repeated_capture_keeps_world_position() {
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        object.transform.translation = Vec3::new(2.0, 0.3, -0.5);
        object.repetition.enabled = true;
        object.repetition.count = [3, 1, 1];
        object.repetition.spacing = Vec3::new(1.5, 1.0, 1.0);
        object.mirror = Some(crate::model::Mirror { axes: [true, false, false] });
        let id = object.uuid;
        let scene = [object];
        let atlas = super::super::super::box_depth_atlas::bake_box_depth_atlas(
            &scene, id, 12, super::super::super::box_depth_atlas::BoxCaptureStart::OutsideBounds)
            .expect("capture modified sphere");
        let samples = samples_from_atlas(&scene, id, &atlas);
        assert!(samples.iter().any(|sample| sample.point.x > 2.8));
        assert!(samples.iter().any(|sample| sample.point.x < 1.2));
        let center = samples.iter().map(|sample| sample.point).sum::<Vec3>() / samples.len() as f32;
        assert!(center.x.abs() < 0.5, "mirrored capture was shifted: {center:?}");
        let mesh = build(&scene, id, 12, 64, Some(std::sync::Arc::new(atlas)),
            &AtomicU32::new(0), None).expect("reconstruct modified sphere");
        let minimum = mesh.positions.iter().map(|point| point.x).fold(f32::INFINITY, f32::min);
        let maximum = mesh.positions.iter().map(|point| point.x).fold(f32::NEG_INFINITY, f32::max);
        assert!(minimum < -2.8 && maximum > 2.8, "mesh bounds {minimum}..{maximum}");
    }

    #[test]
    fn nested_mirror_capture_preserves_both_copies_and_subtraction() {
        use crate::model::{PrimitiveKind, SdfParams, SphereParams, BooleanOperation, Mirror};
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.softness = 0.0;
        root.group_transform.translation = Vec3::new(1.0, 2.0, -1.0);
        root.group_transform.rotation = glam::Quat::from_rotation_y(0.6);
        let mut part = SdfObject::create_kind(PrimitiveKind::Sphere);
        part.boolean_parent = Some(root.uuid);
        part.params = SdfParams::SphereParams(SphereParams { radius: 0.7 });
        part.transform.translation = Vec3::new(3.0, 0.0, 0.0);
        part.mirror = Some(Mirror { axes: [true, false, false] });
        part.softness = 0.0;
        let mut cut = SdfObject::create_kind(PrimitiveKind::Sphere);
        cut.boolean_parent = Some(part.uuid);
        cut.operation = BooleanOperation::Subtract;
        cut.params = SdfParams::SphereParams(SphereParams { radius: 0.25 });
        cut.transform.translation = part.transform.translation;
        let id = root.uuid;
        let scene = [root, part, cut];
        let world = crate::model::group_world_matrix(&scene, id);
        let mut sampler = crate::model::PreparedSubtreeSampler::new(&scene, id).unwrap();
        for sign in [-1.0, 1.0] {
            let hole = sampler.sample(world.transform_point3(Vec3::new(sign * 3.0, 0.0, 0.0))).0;
            let shell = sampler.sample(world.transform_point3(Vec3::new(sign * 3.5, 0.0, 0.0))).0;
            assert!((hole - 0.25).abs() < 1e-4, "cut missing on side {sign}: {hole}");
            assert!((shell + 0.2).abs() < 1e-4, "sphere missing on side {sign}: {shell}");
        }
        let atlas = super::super::super::box_depth_atlas::bake_box_depth_atlas(
            &scene, id, 20, super::super::super::box_depth_atlas::BoxCaptureStart::OutsideBounds).unwrap();
        let samples = samples_from_atlas(&scene, id, &atlas);
        let inverse = world.inverse();
        for sign in [-1.0, 1.0] {
            assert!(samples.iter().any(|sample| {
                let point = inverse.transform_point3(sample.point);
                let normal = inverse.transform_vector3(sample.normal);
                point.x * sign > 3.5 && normal.x * sign > 0.5
            }), "missing reflected surface or normal on side {sign}");
        }
    }
}
