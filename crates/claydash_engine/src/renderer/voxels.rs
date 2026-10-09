//! Six orthographic box captures quantized to a component-local cube grid.
use super::*;
use std::collections::{HashMap, HashSet};
#[cfg(target_arch = "wasm32")]
use std::sync::atomic::Ordering;
use super::computation::{Computation, Stage};

mod geometry;
#[cfg(target_arch = "wasm32")]
mod web_worker;

#[derive(Clone, Copy)]
pub enum VoxelAction { Build(uuid::Uuid), Cancel(uuid::Uuid) }
#[derive(Clone)]
pub enum VoxelStatus {
    Sampling { percent: u32 },
    Ready { cubes: usize, resolution: u32 },
    Failed(String),
}

struct Ready { component: uuid::Uuid, key: u64, geometry: geometry::VoxelGeometry }
struct Job {
    root: uuid::Uuid, component: uuid::Uuid, key: u64, work: Computation,
    #[cfg(not(target_arch = "wasm32"))]
    receiver: std::sync::mpsc::Receiver<Result<geometry::VoxelGeometry, String>>,
    #[cfg(target_arch = "wasm32")]
    worker: web_worker::WebJob,
}
#[derive(Default)]
pub(super) struct VoxelState {
    ready: HashMap<uuid::Uuid, Ready>,
    attempted: HashMap<uuid::Uuid, u64>,
    failed: HashMap<uuid::Uuid, String>,
    job: Option<Job>,
    source_revision: Option<i32>,
    keys: HashMap<uuid::Uuid, u64>,
}

fn source_key(source: &[SdfObject], object: &SdfObject) -> u64 {
    let component = super::poisson_mesh::geometry::containing_root(source, object.uuid);
    let mut local_source = source.to_vec();
    // A leaf's object pose is its capture frame. Remove only rigid pose when
    // there are no world-space references; geometry and scale remain keyed.
    if super::poisson_mesh::geometry::mesh_pose_is_independent(source, component)
        && !crate::model::has_boolean_children(source, component) {
        if let Some(root) = local_source.iter_mut().find(|entry| entry.uuid == component) {
            root.transform.translation = Vec3::ZERO;
            root.transform.rotation = glam::Quat::IDENTITY;
        }
    }
    let root = local_source.iter().find(|entry| entry.uuid == component).unwrap_or(object);
    let key = super::group_capture::capture_key(&local_source, root);
    // Separate identity from depth/Gaussian captures, including schema version.
    (key ^ u64::from(object.voxels.resolution) ^ 0x564f58454c000001).wrapping_mul(0x100000001b3)
}

impl VoxelState {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn seed_pending_scene_reset_test(&mut self) -> Arc<std::sync::atomic::AtomicBool> {
        let root = uuid::Uuid::new_v4();
        let (_, receiver) = std::sync::mpsc::channel();
        let work = Computation::new(Stage::Capture);
        let cancel = work.cancel.clone();
        self.job = Some(Job { root, component: root, key: 1, work, receiver });
        cancel
    }

    pub fn is_busy(&self) -> bool { self.job.is_some() }
    pub fn shown_components(&self) -> HashSet<uuid::Uuid> {
        self.ready.values().map(|ready| ready.component).collect()
    }
    pub fn is_visible(&self) -> bool { !self.ready.is_empty() }

    pub fn handle(&mut self, action: VoxelAction) {
        match action {
            VoxelAction::Build(root) => {
                if self.job.as_ref().is_some_and(|job| job.root == root) { self.job.take(); }
                self.attempted.remove(&root);
                self.failed.remove(&root);
            }
            VoxelAction::Cancel(root) => {
                if self.job.as_ref().is_some_and(|job| job.root == root) { self.job.take(); }
                self.failed.insert(root, "Capture cancelled. Click Recompute to retry.".into());
                // Keep the attempted key: cancellation must not restart a bake.
            }
        }
    }

    pub fn advance(&mut self, source: &[SdfObject], revision: i32, context: &egui::Context) -> bool {
        let roots: Vec<_> = source.iter().filter(|object| object.render_representation
            == crate::model::GroupRenderRepresentation::Voxels).collect();
        // Source revisions are stable across viewport frames. Fingerprinting
        // serialized geometry belongs on edits, not on every repaint/timer tick.
        if self.source_revision != Some(revision) {
            self.keys = roots.iter().map(|object| (object.uuid, source_key(source, object))).collect();
            self.source_revision = Some(revision);
        }
        let keys = &self.keys;
        let previous = self.ready.len();
        self.ready.retain(|id, ready| keys.get(id) == Some(&ready.key));
        self.attempted.retain(|id, key| keys.get(id) == Some(key));
        self.failed.retain(|id, _| self.attempted.contains_key(id));
        let mut changed = previous != self.ready.len();
        if self.job.as_ref().is_some_and(|job| keys.get(&job.root) != Some(&job.key)) {
            self.job.take();
        }
        if let Some(job) = self.job.as_ref() {
            #[cfg(not(target_arch = "wasm32"))]
            let result = match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("Voxel worker stopped".into())),
            };
            #[cfg(target_arch = "wasm32")]
            let result = { job.work.progress.store(job.worker.progress(), Ordering::Relaxed); job.worker.take_result() };
            if let Some(result) = result {
                let job = self.job.take().unwrap();
                match result {
                    Ok(geometry) => {
                        let other_triangles: usize = self.ready.values()
                            .filter(|ready| ready.component != job.component)
                            .map(|ready| ready.geometry.mesh.triangles.len()).sum();
                        if other_triangles + geometry.mesh.triangles.len() > 400_000 {
                            self.failed.insert(job.root, "Voxel scene exceeds geometry budget; lower resolution".into());
                            self.publish(source, context);
                            return changed;
                        }
                        self.ready.retain(|_, ready| ready.component != job.component);
                        self.ready.insert(job.root, Ready { component: job.component, key: job.key, geometry });
                    }
                    Err(error) => { self.failed.insert(job.root, error); }
                }
                changed = true;
            }
        }
        if self.job.is_none() {
            if let Some(object) = roots.into_iter().find(|object| !self.attempted.contains_key(&object.uuid)) {
                let root = object.uuid;
                let component = super::poisson_mesh::geometry::containing_root(source, root);
                let key = keys[&root];
                self.attempted.insert(root, key);
                let resolution = object.voxels.resolution;
                let work = Computation::new(Stage::Sampling);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let source = source.to_vec();
                    let progress = work.progress.clone();
                    let cancel = work.cancel.clone();
                    std::thread::spawn(move || {
                        super::cooperative_work::background_priority();
                        let result = std::panic::catch_unwind(|| geometry::build(&source, component,
                            resolution, &progress, Some(&cancel)))
                            .unwrap_or_else(|_| Err("Voxel capture failed unexpectedly".into()));
                        let _ = sender.send(result);
                    });
                    self.job = Some(Job { root, component, key, work, receiver });
                }
                #[cfg(target_arch = "wasm32")]
                match web_worker::WebJob::start(source.to_vec(), component, resolution, context) {
                    Ok(worker) => { self.job = Some(Job { root, component, key, work, worker }); }
                    Err(error) => { self.failed.insert(root, error); }
                }
            }
        }
        self.publish(source, context);
        changed
    }

    pub fn append_vertices(&self, source: &[SdfObject], vertices: &mut Vec<super::hybrid_splats::GpuMeshVertex>) {
        let owners: HashMap<_, _> = super::bvh::boolean_postorder(source).iter().enumerate()
            .map(|(index, object)| (object.uuid, index as u32)).collect();
        for ready in self.ready.values() {
            let frame = crate::model::lattice_world_matrix(source, ready.component);
            let normal_frame = frame.inverse().transpose();
            let mesh = &ready.geometry.mesh;
            for (index, triangle) in mesh.triangles.iter().enumerate() {
                let Some(&owner) = owners.get(&mesh.owners[index]) else { continue };
                let corners = if frame.determinant() < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
                for corner in corners {
                    let point = frame.transform_point3(mesh.positions[triangle[corner] as usize]);
                    let normal = normal_frame.transform_vector3(mesh.normals[index][corner]);
                    vertices.push(super::hybrid_splats::GpuMeshVertex {
                        position: point.extend(1.0).to_array(), normal: normal.extend(owner as f32).to_array(),
                        captured_color: ready.geometry.colors[index],
                    });
                }
            }
        }
    }

    pub fn computation_status(&self) -> Option<(uuid::Uuid, super::computation::Snapshot)> {
        self.job.as_ref().map(|job| (job.root, job.work.snapshot(Stage::Sampling, Some(job.work.percent()))))
    }
    fn publish(&self, source: &[SdfObject], context: &egui::Context) {
        let mut status = HashMap::new();
        for (id, ready) in &self.ready {
            if let Some(object) = source.iter().find(|object| object.uuid == *id) {
                status.insert(*id, VoxelStatus::Ready { cubes: ready.geometry.cubes, resolution: object.voxels.resolution });
            }
        }
        for (id, error) in &self.failed { status.insert(*id, VoxelStatus::Failed(error.clone())); }
        if let Some(job) = &self.job {
            status.insert(job.root, VoxelStatus::Sampling { percent: job.work.percent() });
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
        context.data_mut(|data| data.insert_temp(egui::Id::new("voxel-status"), status));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GroupRenderRepresentation, PrimitiveKind};

    #[test]
    fn cache_identity_tracks_voxel_settings_geometry_color_and_reuses_rigid_pose() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.render_representation = GroupRenderRepresentation::Voxels;
        let source = vec![object.clone()];
        let key = source_key(&source, &object);
        object.transform.translation = Vec3::new(7.0, -3.0, 2.0);
        object.transform.rotation = glam::Quat::from_rotation_y(0.7);
        assert_eq!(key, source_key(&[object.clone()], &object));
        object.voxels.resolution += 1;
        assert_ne!(key, source_key(&[object.clone()], &object));
        object.voxels.resolution -= 1;
        object.color.x *= 0.5;
        assert_ne!(key, source_key(&[object.clone()], &object));
        object.color = source[0].color;
        object.transform.scale.x = 2.0;
        assert_ne!(key, source_key(&[object.clone()], &object));
    }

    #[test]
    fn reflected_cube_upload_preserves_outward_normals_and_captured_colors() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.render_representation = GroupRenderRepresentation::Voxels;
        object.color = glam::Vec4::new(0.8, 0.2, 0.1, 1.0);
        let geometry = geometry::build(&[object.clone()], object.uuid, 8,
            &std::sync::atomic::AtomicU32::new(0), None).unwrap();
        let mut state = VoxelState::default();
        state.ready.insert(object.uuid, Ready { component: object.uuid, key: 0, geometry });
        object.transform.scale = Vec3::new(-2.0, 1.0, 0.5);
        let mut vertices = Vec::new();
        state.append_vertices(&[object], &mut vertices);
        assert!(!vertices.is_empty());
        for face in vertices.chunks_exact(3) {
            let [a, b, c] = std::array::from_fn(|corner| Vec3::from_slice(&face[corner].position[..3]));
            let normal = Vec3::from_slice(&face[0].normal[..3]);
            assert!((b - a).cross(c - a).dot(normal) > 0.0);
            assert_eq!(face[0].captured_color, [0.8, 0.2, 0.1, 1.0]);
        }
    }

    #[test]
    fn automatic_build_cancel_recompute_and_mode_removal_have_distinct_lifecycles() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.render_representation = GroupRenderRepresentation::Voxels;
        object.voxels.resolution = 8;
        let context = egui::Context::default();
        let mut state = VoxelState::default();
        assert!(!state.advance(&[object.clone()], 0, &context));
        assert!(state.job.is_some(), "selecting Voxels starts an actual bake");
        state.handle(VoxelAction::Cancel(object.uuid));
        state.advance(&[object.clone()], 0, &context);
        assert!(state.job.is_none(), "cancel must suppress automatic retry");
        state.handle(VoxelAction::Build(object.uuid));
        state.advance(&[object.clone()], 0, &context);
        assert!(state.job.is_some(), "explicit compute retries a cancelled build");
        object.render_representation = GroupRenderRepresentation::ExactSdf;
        state.advance(&[object], 1, &context);
        assert!(state.job.is_none());
        assert!(state.attempted.is_empty());
    }
}
