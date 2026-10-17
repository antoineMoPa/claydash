use super::*;
use super::computation::{Computation, Stage};
use std::{collections::{HashMap, VecDeque}, sync::Arc};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
use std::sync::atomic::Ordering;
use wgpu::util::DeviceExt;

pub mod geometry;
pub mod glb;
pub mod textured_glb;
pub mod solid_glb;
#[cfg(target_arch = "wasm32")]
pub mod web_worker;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildProgress {
    Sampling(u32),
    Reconstruction(u32),
    Complete,
}

impl BuildProgress {
    pub fn from_code(code: u32) -> Self {
        match code {
            0..=99 => Self::Sampling(code),
            100..=199 => Self::Reconstruction(code - 100),
            _ => Self::Complete,
        }
    }

    pub fn snapshot(self, work: &Computation) -> super::computation::Snapshot {
        match self {
            Self::Sampling(percent) => work.snapshot(Stage::Sampling, Some(percent)),
            Self::Reconstruction(percent) => work.snapshot(Stage::Reconstruction, Some(percent)),
            Self::Complete => work.snapshot(Stage::Reconstruction, Some(100)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshAlgorithm { Poisson, Direct }

#[derive(Clone, Copy)]
pub enum PoissonMeshAction {
    Build(uuid::Uuid),
    Cancel(uuid::Uuid),
    Show(uuid::Uuid),
    Hide(uuid::Uuid),
}

#[derive(Clone)]
pub enum PoissonMeshStatus {
    Sampling { percent: u32 },
    Reconstructing { percent: u32 },
    Ready { vertices: usize, triangles: usize, showing: bool, available: bool,
        sample_resolution: u32, mesh_resolution: u32 },
    Failed(String),
}

struct Job {
    root: uuid::Uuid,
    component: uuid::Uuid,
    revision: i32,
    sample_resolution: u32,
    mesh_resolution: u32,
    algorithm: MeshAlgorithm,
    baked_frame_inverse: glam::Mat4,
    work: Computation,
    #[cfg(not(target_arch = "wasm32"))]
    receiver: mpsc::Receiver<Result<geometry::Mesh, String>>,
    #[cfg(target_arch = "wasm32")]
    worker: web_worker::WebJob,
    invalidated: bool,
}

struct Ready {
    root: uuid::Uuid,
    component: uuid::Uuid,
    revision: i32,
    sample_resolution: u32,
    mesh_resolution: u32,
    algorithm: MeshAlgorithm,
    baked_frame_inverse: glam::Mat4,
    vertices: usize,
    triangles: usize,
    showing: bool,
    mesh: Arc<geometry::Mesh>,
}

pub struct CachedExportMesh {
    mesh: Arc<geometry::Mesh>,
    pose: glam::Mat4,
}

impl CachedExportMesh {
    pub fn world_mesh(&self, cancel: &std::sync::atomic::AtomicBool) -> Result<geometry::Mesh, String> {
        mesh_at_pose((*self.mesh).clone(), self.pose, cancel)
    }
}

/// Apply a component frame, preserving flat/smooth normals and reflected winding.
pub(crate) fn mesh_at_pose(mut mesh: geometry::Mesh, pose: glam::Mat4,
    cancel: &std::sync::atomic::AtomicBool) -> Result<geometry::Mesh, String> {
    let normal_pose = pose.inverse().transpose();
    let reflected = pose.determinant() < 0.0;
    let mut budget = super::cooperative_work::WorkerBudget::new();
    for (index, point) in mesh.positions.iter_mut().enumerate() {
        if index % 512 == 0 {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) { return Err("Export cancelled".into()); }
            budget.checkpoint();
        }
        *point = pose.transform_point3(*point);
    }
    for (index, normals) in mesh.normals.iter_mut().enumerate() {
        if index % 512 == 0 {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) { return Err("Export cancelled".into()); }
            budget.checkpoint();
        }
        for normal in normals.iter_mut() {
            *normal = normal_pose.transform_vector3(*normal).normalize_or_zero();
        }
        if reflected {
            mesh.triangles[index].swap(1, 2);
            normals.swap(1, 2);
        }
    }
    Ok(mesh)
}

impl Renderer {
    pub fn cached_mesh_for_export(&self, source: &[SdfObject], root: uuid::Uuid, revision: i32) -> Option<CachedExportMesh> {
        self.poisson_mesh.cached_for_export(source, root, revision, MeshAlgorithm::Poisson)
    }
    pub fn cached_direct_mesh_for_export(&self, source: &[SdfObject], root: uuid::Uuid, revision: i32) -> Option<CachedExportMesh> {
        self.poisson_mesh.cached_for_export(source, root, revision, MeshAlgorithm::Direct)
    }
}

#[derive(Default)]
pub(super) struct PoissonMeshState {
    pub requests: VecDeque<uuid::Uuid>,
    job: Option<Job>,
    ready: HashMap<uuid::Uuid, Ready>,
    failure: HashMap<uuid::Uuid, String>,
    failure_sources: HashMap<uuid::Uuid, (i32, MeshAlgorithm, u32)>,
    cancelled: std::collections::HashSet<uuid::Uuid>,
    last_revision: Option<i32>,
}

impl PoissonMeshState {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn seed_pending_scene_reset_test(&mut self) -> Arc<std::sync::atomic::AtomicBool> {
        let root = uuid::Uuid::new_v4();
        let (_, receiver) = mpsc::channel();
        let work = Computation::new(Stage::Sampling);
        let cancel = work.cancel.clone();
        self.requests.push_back(root);
        self.job = Some(Job { root, component: root, revision: 1,
            sample_resolution: 16, mesh_resolution: 16, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY,
            work, receiver, invalidated: false });
        cancel
    }

    pub(super) fn is_busy(&self) -> bool { self.job.is_some() || !self.requests.is_empty() }

    fn cached_for_export(&self, source: &[SdfObject], root: uuid::Uuid, revision: i32, algorithm: MeshAlgorithm) -> Option<CachedExportMesh> {
        // A cached whole-component Boolean result cannot safely be sliced by
        // triangle owner to export an arbitrary selected child subtree.
        let object = source.iter().find(|object| object.uuid == root)?;
        let ready = self.ready.values().find(|ready| ready.component == root && ready.revision == revision
            && ready.algorithm == algorithm
            && (algorithm == MeshAlgorithm::Direct || ready.sample_resolution == object.gaussian_splats.resolution)
            && ready.mesh_resolution == object.poisson_mesh.resolution)?;
        Some(CachedExportMesh { mesh: ready.mesh.clone(),
            pose: geometry::mesh_world_frame(source, root) * ready.baked_frame_inverse })
    }
    pub fn next_request(&self) -> Option<uuid::Uuid> {
        self.job.is_none().then(|| self.requests.front().copied()).flatten()
    }
    /// Direct meshes are a live representation: build on first use and again
    /// after an edit. A failed source is left as exact SDF until it changes.
    pub fn queue_missing_direct_meshes(&mut self, source: &[SdfObject], revision: i32) {
        for object in source.iter().filter(|object| object.boolean_parent.is_none()
            && object.render_representation == crate::model::GroupRenderRepresentation::DirectMesh) {
            let id = object.uuid;
            if self.ready.get(&id).is_some_and(|ready| ready.revision == revision
                && ready.mesh_resolution == object.poisson_mesh.resolution)
                || self.job.as_ref().is_some_and(|job| job.root == id && job.revision == revision
                    && job.mesh_resolution == object.poisson_mesh.resolution && !job.invalidated)
                || self.requests.contains(&id) || self.cancelled.contains(&id)
                || self.failure_sources.get(&id).is_some_and(|(failed_revision, algorithm, resolution)|
                    *failed_revision == revision && *algorithm == MeshAlgorithm::Direct
                        && *resolution == object.poisson_mesh.resolution) {
                continue;
            }
            self.requests.push_back(id);
        }
    }
    pub fn shown_root(&self) -> Option<uuid::Uuid> {
        self.ready.values().find(|ready| ready.showing).map(|ready| ready.component)
    }

    pub fn shown_components(&self) -> std::collections::HashSet<uuid::Uuid> {
        self.ready.values().filter(|ready| ready.showing).map(|ready| ready.component).collect()
    }

    pub fn handle(&mut self, action: PoissonMeshAction) -> bool {
        match action {
            PoissonMeshAction::Cancel(root) => {
                self.cancelled.insert(root);
                self.requests.retain(|id| *id != root);
                if self.job.as_ref().is_some_and(|job| job.root == root) {
                    self.job.take(); // Drop signals cancellation; late results are discarded.
                }
                self.failure.remove(&root);
                self.failure_sources.remove(&root);
                false
            }
            PoissonMeshAction::Build(root) => {
                self.cancelled.remove(&root);
                if self.job.as_ref().is_some_and(|job| job.root == root) { return false; }
                self.requests.retain(|id| *id != root);
                self.requests.push_back(root);
                self.failure.remove(&root);
                self.failure_sources.remove(&root);
                // Scheduling background work does not change the displayed
                // scene. Keep a ready mesh until its replacement arrives, and
                // avoid re-uploading/recompiling the SDF fallback on a click.
                false
            }
            PoissonMeshAction::Show(root) => {
                if let Some(ready) = self.ready.get_mut(&root) {
                    ready.showing = true;
                    return true;
                }
                false
            }
            PoissonMeshAction::Hide(root) => {
                if let Some(ready) = self.ready.get_mut(&root) {
                    ready.showing = false;
                    return true;
                }
                false
            }
        }
    }

    pub fn reconcile_representation(&mut self, source: &[SdfObject]) -> bool {
        let matches_mode = |root, algorithm| source.iter().find(|object| object.uuid == root)
            .is_some_and(|object| match algorithm {
                MeshAlgorithm::Poisson => object.render_representation == crate::model::GroupRenderRepresentation::PoissonMesh,
                MeshAlgorithm::Direct => object.render_representation == crate::model::GroupRenderRepresentation::DirectMesh,
            });
        self.cancelled.retain(|root| source.iter().any(|object| object.uuid == *root
            && object.render_representation == crate::model::GroupRenderRepresentation::DirectMesh));
        self.failure.retain(|root, _| self.failure_sources.get(root)
            .is_some_and(|(_, algorithm, resolution)| matches_mode(*root, *algorithm)
                && source.iter().find(|object| object.uuid == *root)
                    .is_some_and(|object| *algorithm != MeshAlgorithm::Direct
                        || object.poisson_mesh.resolution == *resolution)));
        self.failure_sources.retain(|root, _| self.failure.contains_key(root));
        let count = self.ready.len();
        self.ready.retain(|_, ready| matches_mode(ready.root, ready.algorithm)
            && (ready.algorithm != MeshAlgorithm::Direct || source.iter().find(|object| object.uuid == ready.root)
                .is_some_and(|object| object.poisson_mesh.resolution == ready.mesh_resolution)));
        let mut changed = count != self.ready.len();
        if let Some(job) = self.job.as_mut().filter(|job| !job.invalidated && !matches_mode(job.root, job.algorithm)) {
            job.invalidated = true;
            job.work.cancel();
            changed = true;
        }
        #[cfg(target_arch = "wasm32")]
        if self.job.as_ref().is_some_and(|job| job.invalidated) { self.job.take(); }
        changed
    }

    pub fn invalidate_for_revision(&mut self, revision: i32) -> bool {
        if self.last_revision != Some(revision) {
            self.cancelled.clear();
            self.last_revision = Some(revision);
        }
        self.failure.retain(|root, _| self.failure_sources.get(root)
            .is_some_and(|(built_revision, _, _)| *built_revision == revision));
        self.failure_sources.retain(|root, _| self.failure.contains_key(root));
        let original = self.ready.len();
        self.ready.retain(|_, ready| ready.revision == revision);
        if self.job.as_ref().is_some_and(|job| job.revision != revision && !job.invalidated) {
            #[cfg(target_arch = "wasm32")]
            {
                self.job.take();
                return true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(job) = self.job.as_mut() {
                job.invalidated = true;
                job.work.cancel();
            }
            #[cfg(not(target_arch = "wasm32"))]
            return true;
        }
        self.ready.len() != original
    }

    pub fn clear_for_root(&mut self, root: uuid::Uuid) -> bool {
        self.cancelled.remove(&root);
        let active = self.requests.iter().any(|id| *id == root)
            || self.job.as_ref().is_some_and(|job| job.root == root)
            || self.ready.contains_key(&root)
            || self.failure.contains_key(&root);
        if active {
            self.requests.retain(|id| *id != root);
            if let Some(job) = self.job.as_mut().filter(|job| job.root == root) {
                job.invalidated = true;
                job.work.cancel();
            }
            #[cfg(target_arch = "wasm32")]
            if self.job.as_ref().is_some_and(|job| job.root == root) { self.job.take(); }
            self.ready.remove(&root);
            self.failure.remove(&root);
            self.failure_sources.remove(&root);
        }
        active
    }

    pub fn start(&mut self, root: uuid::Uuid, component: uuid::Uuid, revision: i32,
        source: Vec<SdfObject>, resolution: u32, mesh_resolution: u32, algorithm: MeshAlgorithm,
        cached: Option<Arc<super::box_depth_atlas::BoxDepthAtlas>>,
        #[cfg(target_arch = "wasm32")] context: &egui::Context,
    ) {
        self.requests.retain(|id| *id != root);
        #[cfg(not(target_arch = "wasm32"))]
        let (sender, receiver) = mpsc::channel();
        let work = Computation::new(Stage::Sampling);
        let baked_frame_inverse = geometry::mesh_world_frame(&source, component).inverse();
        #[cfg(not(target_arch = "wasm32"))]
        let worker_progress = work.progress.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let worker_cancel = work.cancel.clone();
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            super::cooperative_work::background_priority();
            let result = std::panic::catch_unwind(|| match algorithm {
                MeshAlgorithm::Poisson => geometry::build(&source, component, resolution,
                    mesh_resolution, cached, &worker_progress, Some(&worker_cancel)),
                MeshAlgorithm::Direct => super::direct_mesh::build(&source, component, mesh_resolution,
                    &worker_cancel, &worker_progress),
            })
                .unwrap_or_else(|_| Err("Mesh construction failed unexpectedly".into()));
            let _ = sender.send(result);
        });
        #[cfg(target_arch = "wasm32")]
        let worker = match web_worker::WebJob::start_with_algorithm(source, component, resolution, mesh_resolution, algorithm, context) {
            Ok(worker) => worker,
            Err(error) => {
                self.failure.insert(root, error);
                self.failure_sources.insert(root, (revision, algorithm, mesh_resolution));
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        let _ = cached;
        self.job = Some(Job { root, component, revision, sample_resolution: resolution,
            mesh_resolution, algorithm, baked_frame_inverse, work,
            #[cfg(not(target_arch = "wasm32"))]
            receiver,
            #[cfg(target_arch = "wasm32")]
            worker,
            invalidated: false });
    }

    pub fn poll(&mut self, revision: i32) -> bool {
        let Some(job) = self.job.as_ref() else { return false; };
        #[cfg(target_arch = "wasm32")]
        job.work.progress.store(job.worker.progress(), Ordering::Relaxed);
        #[cfg(target_arch = "wasm32")]
        let Some(result) = job.worker.take_result() else { return false };
        #[cfg(not(target_arch = "wasm32"))]
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("Mesh construction worker stopped".into()),
        };
        let job = self.job.take().unwrap();
        if job.invalidated || job.revision != revision { return false; }
        match result {
            Ok(mesh) if !mesh.triangles.is_empty() => {
                self.ready.retain(|_, ready| ready.component != job.component);
                self.ready.insert(job.root, Ready {
                    root: job.root, component: job.component, revision,
                    sample_resolution: job.sample_resolution, mesh_resolution: job.mesh_resolution, algorithm: job.algorithm,
                    vertices: mesh.positions.len(),
                    baked_frame_inverse: job.baked_frame_inverse,
                    triangles: mesh.triangles.len(),
                    showing: true, mesh: Arc::new(mesh),
                });
            }
            Ok(_) => {
                if job.algorithm == MeshAlgorithm::Direct { self.ready.remove(&job.root); }
                self.failure.insert(job.root, "Mesh construction returned an empty mesh".into());
                self.failure_sources.insert(job.root, (revision, job.algorithm, job.mesh_resolution));
            }
            Err(error) => {
                if job.algorithm == MeshAlgorithm::Direct { self.ready.remove(&job.root); }
                self.failure.insert(job.root, error);
                self.failure_sources.insert(job.root, (revision, job.algorithm, job.mesh_resolution));
            }
        }
        true
    }

    pub fn rebuild_buffer(&self, device: &wgpu::Device, source: &[SdfObject], buffer: &mut wgpu::Buffer, count: &mut u32, voxels: &super::voxels::VoxelState) {
        let owners: HashMap<_, _> = super::bvh::boolean_postorder(source).iter().enumerate()
            .map(|(index, object)| (object.uuid, (index as u32, object.color.to_array()))).collect();
        let capacity = self.ready.values().filter(|ready| ready.showing)
            .map(|ready| ready.mesh.triangles.len()).sum::<usize>() * 3;
        let mut vertices = Vec::with_capacity(capacity);
        for ready in self.ready.values().filter(|ready| ready.showing) {
            let pose = geometry::mesh_world_frame(source, ready.component) * ready.baked_frame_inverse;
            let normal_pose = pose.inverse().transpose();
            for (triangle_index, triangle) in ready.mesh.triangles.iter().enumerate() {
                let Some(&(owner, color)) = owners.get(&ready.mesh.owners[triangle_index]) else { continue; };
                let points = triangle.map(|index| pose.transform_point3(ready.mesh.positions[index as usize]));
                for (corner, point) in points.into_iter().enumerate() {
                    let normal = normal_pose.transform_vector3(ready.mesh.normals[triangle_index][corner]);
                    vertices.push(super::hybrid_splats::GpuMeshVertex {
                        position: point.extend(1.0).to_array(), normal: normal.extend(owner as f32).to_array(),
                        captured_color: color,
                    });
                }
            }
        }
        voxels.append_vertices(source, &mut vertices);
        *count = vertices.len() as u32;
        if !vertices.is_empty() {
            *buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Poisson mesh vertices"), contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        }
    }

    pub fn computation_status(&self) -> Option<(uuid::Uuid, super::computation::Snapshot)> {
        let job = self.job.as_ref().filter(|job| !job.invalidated)?;
        let progress = BuildProgress::from_code(job.work.progress.load(Ordering::Relaxed));
        Some((job.root, progress.snapshot(&job.work)))
    }

    pub fn publish(&self, context: &egui::Context, available: bool) {
        let mut statuses = std::collections::HashMap::new();
        for ready in self.ready.values() {
            statuses.insert(ready.root, PoissonMeshStatus::Ready {
                vertices: ready.vertices, triangles: ready.triangles,
                showing: ready.showing, available,
                sample_resolution: ready.sample_resolution, mesh_resolution: ready.mesh_resolution,
            });
        }
        for root in &self.requests { statuses.insert(*root, PoissonMeshStatus::Sampling { percent: 0 }); }
        if let Some(job) = self.job.as_ref().filter(|job| !job.invalidated) {
            let progress = BuildProgress::from_code(job.work.progress.load(Ordering::Relaxed));
            statuses.insert(job.root, match progress {
                BuildProgress::Sampling(percent) => PoissonMeshStatus::Sampling { percent },
                BuildProgress::Reconstruction(percent) => PoissonMeshStatus::Reconstructing { percent },
                BuildProgress::Complete => PoissonMeshStatus::Reconstructing { percent: 100 },
            });
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
        for (root, error) in &self.failure {
            statuses.insert(*root, PoissonMeshStatus::Failed(error.clone()));
        }
        context.data_mut(|data| data.insert_temp(egui::Id::new("poisson-mesh-status"), statuses));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_mesh_queues_once_and_retries_after_source_or_resolution_change() {
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::SheetPanel);
        object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
        let root = object.uuid;
        let mut state = PoissonMeshState::default();
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        assert_eq!(state.requests.iter().copied().collect::<Vec<_>>(), vec![root]);
        state.handle(PoissonMeshAction::Cancel(root));
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        assert!(state.requests.is_empty(), "cancel must stop the automatic retry loop");
        state.invalidate_for_revision(7);
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        assert_eq!(state.next_request(), Some(root));
        state.requests.clear();
        state.failure.insert(root, "failed".into());
        state.failure_sources.insert(root, (7, MeshAlgorithm::Direct, object.poisson_mesh.resolution));
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        assert!(state.requests.is_empty(), "failed geometry should fall back to exact SDF");
        object.poisson_mesh.resolution += 1;
        state.reconcile_representation(&[object.clone()]);
        state.queue_missing_direct_meshes(&[object.clone()], 7);
        assert_eq!(state.next_request(), Some(root));
        state.requests.clear();
        state.invalidate_for_revision(8);
        state.queue_missing_direct_meshes(&[object], 8);
        assert_eq!(state.next_request(), Some(root));
    }

    #[test]
    fn reconstruction_progress_has_its_own_percentage_and_stage() {
        let work = Computation::new(Stage::Sampling);
        let sampling = BuildProgress::from_code(90).snapshot(&work);
        assert_eq!(sampling.stage, Stage::Sampling);
        assert_eq!(sampling.percent, Some(90));
        let reconstruction = BuildProgress::from_code(130).snapshot(&work);
        assert_eq!(reconstruction.stage, Stage::Reconstruction);
        assert_eq!(reconstruction.percent, Some(30));
        assert_eq!(BuildProgress::from_code(200).snapshot(&work).percent, Some(100));
    }

    #[test]
    fn direct_cache_keeps_algorithm_and_drops_on_mode_switch() {
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
        let root = object.uuid;
        let mut state = PoissonMeshState::default();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64,
            algorithm: MeshAlgorithm::Direct, baked_frame_inverse: glam::Mat4::IDENTITY,
            vertices: 3, triangles: 1, showing: true,
            mesh: Arc::new(geometry::Mesh::from_parts(
                vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root])),
        });
        assert!(state.cached_for_export(&[object.clone()], root, 7, MeshAlgorithm::Direct).is_some());
        assert!(state.cached_for_export(&[object.clone()], root, 7, MeshAlgorithm::Poisson).is_none());
        object.gaussian_splats.resolution = 96;
        assert!(state.cached_for_export(&[object.clone()], root, 7, MeshAlgorithm::Direct).is_some(),
            "direct construction does not depend on capture sample resolution");
        assert!(!state.reconcile_representation(&[object.clone()]));
        object.render_representation = crate::model::GroupRenderRepresentation::PoissonMesh;
        assert!(state.reconcile_representation(&[object]));
        assert!(state.shown_components().is_empty(), "a previous algorithm cannot hide the new source");
    }

    #[test]
    fn direct_failure_restores_exact_source_and_invalidated_results_never_publish() {
        let root = uuid::Uuid::new_v4();
        let mut state = PoissonMeshState::default();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64,
            algorithm: MeshAlgorithm::Direct, baked_frame_inverse: glam::Mat4::IDENTITY,
            vertices: 3, triangles: 1, showing: true,
            mesh: Arc::new(geometry::Mesh::from_parts(
                vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root])),
        });
        let (sender, receiver) = mpsc::channel();
        state.job = Some(Job {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64,
            algorithm: MeshAlgorithm::Direct, baked_frame_inverse: glam::Mat4::IDENTITY,
            work: Computation::new(Stage::Sampling), receiver, invalidated: false,
        });
        sender.send(Err("unsupported source modifier".into())).unwrap();
        assert!(state.poll(7));
        assert!(state.shown_components().is_empty(), "failed direct builds show editable exact geometry");
        assert_eq!(state.failure[&root], "unsupported source modifier");
        let (sender, receiver) = mpsc::channel();
        state.job = Some(Job {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64,
            algorithm: MeshAlgorithm::Direct, baked_frame_inverse: glam::Mat4::IDENTITY,
            work: Computation::new(Stage::Sampling), receiver, invalidated: false,
        });
        assert!(state.invalidate_for_revision(8));
        sender.send(Ok(geometry::Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root]))).unwrap();
        assert!(!state.poll(8));
        assert!(state.shown_components().is_empty());
    }

    #[test]
    #[ignore = "requires a GPU adapter and native readback"]
    fn direct_thin_cut_sheet_displays_mesh_and_preserves_the_hole() {
        use crate::model::{GroupRenderRepresentation, PrimitiveKind, SdfParams, SheetPanelParams, SheetCut};
        use std::time::{Duration, Instant};
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 192, height: 192, ..Default::default()
        })).expect("headless direct mesh renderer");
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 3.0);
        camera.target = Vec3::ZERO;
        let world = World { screen_space_ambient_occlusion: false, ..World::default() };
        let background = renderer.render_offscreen(&camera, &[], world, &[], Duration::from_secs(60)).unwrap();
        let mut sheet = SdfObject::create_kind(PrimitiveKind::SheetPanel);
        sheet.render_representation = GroupRenderRepresentation::DirectMesh;
        sheet.poisson_mesh.resolution = 16;
        sheet.color = glam::Vec4::new(0.95, 0.1, 0.04, 1.0);
        sheet.params = SdfParams::SheetPanelParams(SheetPanelParams {
            size: glam::Vec2::new(1.4, 1.4), thickness: 0.002,
            cuts: vec![SheetCut::Circle { center: glam::Vec2::ZERO, radius: 0.2 }],
            ..Default::default()
        });
        let source = vec![sheet.clone()];
        let context = egui::Context::default();
        renderer.poisson_mesh.handle(PoissonMeshAction::Build(sheet.uuid));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            renderer.advance_computations(&camera, &source, &[], [1, 1], 0, world, &context, true);
            assert!(renderer.poisson_mesh.failure.is_empty(), "direct sheet build failed");
            if renderer.poisson_mesh.ready.contains_key(&sheet.uuid) { break; }
            assert!(Instant::now() < deadline, "direct mesh optimization timeout");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(renderer.poisson_mesh.ready[&sheet.uuid].algorithm, MeshAlgorithm::Direct);
        assert!(renderer.mesh_vertex_count > 0, "completed direct geometry uploads raster vertices");
        let frame = renderer.render_offscreen(&camera, &source, world, &[], Duration::from_secs(60)).unwrap();
        assert!(renderer.poisson_mesh.shown_components().contains(&sheet.uuid));
        let center = (96 * 192 + 96) * 4;
        assert!(frame.rgba[center..center + 3].iter().zip(&background.rgba[center..center + 3])
            .all(|(a, b)| a.abs_diff(*b) <= 5), "the cut must expose the background");
        let colored = frame.rgba.chunks_exact(4).filter(|pixel|
            pixel[0] > pixel[1].saturating_add(20) && pixel[0] > pixel[2].saturating_add(20)).count();
        assert!(colored > 200, "thin sheet raster mesh must remain visible: {colored}");
        image::save_buffer("/tmp/claydash-direct-sheet-review.png", &frame.rgba,
            frame.width, frame.height, image::ColorType::Rgba8).unwrap();
    }

    #[test]
    #[ignore = "requires a GPU adapter and native readback"]
    fn direct_smooth_modifier_optimization_displays_closed_mesh() {
        use crate::model::{GroupRenderRepresentation, PrimitiveKind};
        use std::time::{Duration, Instant};
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 192, height: 192, ..Default::default()
        })).expect("headless direct mesh renderer");
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 3.0);
        camera.target = Vec3::ZERO;
        let world = World { screen_space_ambient_occlusion: false, ..World::default() };
        let mut sheet = SdfObject::create_kind(PrimitiveKind::Sphere);
        sheet.render_representation = GroupRenderRepresentation::DirectMesh;
        sheet.poisson_mesh.resolution = 16;
        sheet.softness = 0.12;
        sheet.color = glam::Vec4::new(0.95, 0.1, 0.04, 1.0);
        sheet.mirror = Some(crate::model::Mirror { axes: [true, false, false] });
        sheet.repetition.enabled = true;
        sheet.repetition.count = [2, 1, 1];
        sheet.repetition.spacing = Vec3::new(0.7, 1.0, 1.0);
        let mut cage = crate::model::Lattice::new(Vec3::splat(-2.0), Vec3::splat(2.0), 2);
        cage.offsets.fill(Vec3::new(0.06, 0.08, 0.03));
        sheet.lattice = Some(cage);
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(sheet.uuid);
        child.transform.translation = Vec3::new(0.3, 0.1, 0.0);
        child.transform.scale = Vec3::splat(0.65);
        child.color = sheet.color;
        let source = vec![sheet.clone(), child];
        let context = egui::Context::default();
        renderer.poisson_mesh.handle(PoissonMeshAction::Build(sheet.uuid));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            renderer.advance_computations(&camera, &source, &[], [1, 1], 0, world, &context, true);
            assert!(renderer.poisson_mesh.failure.is_empty(), "direct smooth modifier build failed");
            if renderer.poisson_mesh.ready.contains_key(&sheet.uuid) { break; }
            assert!(Instant::now() < deadline, "direct mesh optimization timeout");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(renderer.poisson_mesh.ready[&sheet.uuid].algorithm, MeshAlgorithm::Direct);
        assert!(renderer.mesh_vertex_count > 0, "completed direct geometry uploads raster vertices");
        let frame = renderer.render_offscreen(&camera, &source, world, &[], Duration::from_secs(60)).unwrap();
        assert!(renderer.poisson_mesh.shown_components().contains(&sheet.uuid));
        let colored = frame.rgba.chunks_exact(4).filter(|pixel|
            pixel[0] > pixel[1].saturating_add(20) && pixel[0] > pixel[2].saturating_add(20)).count();
        assert!(colored > 200, "smooth modified raster mesh must remain visible: {colored}");
        image::save_buffer("/tmp/claydash-direct-smooth-modifier-review.png", &frame.rgba,
            frame.width, frame.height, image::ColorType::Rgba8).unwrap();
    }

    #[test]
    fn cached_mesh_selection_is_explicit() {
        let state = PoissonMeshState::default();
        assert!(state.shown_root().is_none());
    }

    #[test]
    fn recompute_preserves_display_and_publishes_progress_over_ready_mesh() {
        let mut state = PoissonMeshState::default();
        let root = uuid::Uuid::new_v4();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
            showing: true,
            mesh: Arc::new(geometry::Mesh::from_parts(
                vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root])),
        });
        assert!(!state.handle(PoissonMeshAction::Build(root)),
            "starting a worker must not invalidate the viewport or compile the SDF fallback");
        assert_eq!(state.shown_root(), Some(root));
        assert_eq!(state.next_request(), Some(root));
        let context = egui::Context::default();
        state.publish(&context, true);
        let status = context.data(|data| data.get_temp::<HashMap<uuid::Uuid, PoissonMeshStatus>>(
            egui::Id::new("poisson-mesh-status"))).unwrap();
        assert!(matches!(status[&root], PoissonMeshStatus::Sampling { percent: 0, .. }));

        let (sender, receiver) = mpsc::channel();
        state.requests.clear();
        state.job = Some(Job {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY,
            work: Computation::new(Stage::Sampling), receiver, invalidated: false,
        });
        state.job.as_ref().unwrap().work.progress.store(42, Ordering::Relaxed);
        state.publish(&context, true);
        let status = context.data(|data| data.get_temp::<HashMap<uuid::Uuid, PoissonMeshStatus>>(
            egui::Id::new("poisson-mesh-status"))).unwrap();
        assert!(matches!(status[&root], PoissonMeshStatus::Sampling { percent: 42, .. }));
        sender.send(Ok(geometry::Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X * 2.0, Vec3::Y], vec![[0, 1, 2]], vec![root]))).unwrap();
        assert!(state.poll(7), "only the completed replacement should invalidate the viewport");
        assert_eq!(state.ready[&root].mesh.positions[1], Vec3::X * 2.0);
    }

    #[test]
    fn first_build_keeps_exact_view_valid() {
        let mut state = PoissonMeshState::default();
        let root = uuid::Uuid::new_v4();
        assert!(!state.handle(PoissonMeshAction::Build(root)));
        assert_eq!(state.next_request(), Some(root));
        assert!(!state.handle(PoissonMeshAction::Build(root)));
        assert_eq!(state.requests.len(), 1);
    }

    #[test]
    fn cancel_stops_queued_and_active_work_without_replacing_the_displayed_mesh() {
        let mut state = PoissonMeshState::default();
        let root = uuid::Uuid::new_v4();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
            showing: true,
            mesh: Arc::new(geometry::Mesh::from_parts(
                vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root])),
        });
        state.handle(PoissonMeshAction::Build(root));
        assert!(!state.handle(PoissonMeshAction::Cancel(root)));
        assert!(state.next_request().is_none());
        let (sender, receiver) = mpsc::channel();
        let work = Computation::new(Stage::Sampling);
        let cancel = work.cancel.clone();
        state.job = Some(Job {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY,
            work, receiver, invalidated: false,
        });
        assert!(!state.handle(PoissonMeshAction::Cancel(root)));
        assert!(cancel.load(Ordering::Relaxed));
        assert!(sender.send(Err("late result".into())).is_err());
        assert!(!state.poll(7));
        assert_eq!(state.shown_root(), Some(root));
        assert!(state.failure.is_empty());
        state.handle(PoissonMeshAction::Build(root));
        assert_eq!(state.next_request(), Some(root));
        state.handle(PoissonMeshAction::Cancel(root));
    }

    #[test]
    fn ready_mesh_survives_view_changes_until_source_changes() {
        let mut state = PoissonMeshState::default();
        let root = uuid::Uuid::new_v4();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
            showing: true,
            mesh: Arc::new(geometry::Mesh::from_parts(
                vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![root])),
        });
        assert!(!state.invalidate_for_revision(7));
        assert_eq!(state.shown_root(), Some(root));
        assert!(state.invalidate_for_revision(8));
        assert_eq!(state.shown_root(), None);
    }

    #[test]
    fn cached_mesh_pose_moves_vertices_and_preserves_normal_perpendicularity() {
        let mut root = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        root.group_transform.translation = Vec3::new(1.0, 2.0, -1.0);
        root.group_transform.rotation = glam::Quat::from_rotation_y(0.4);
        let id = root.uuid;
        let mut source = vec![root, child];
        let baked = geometry::mesh_world_frame(&source, id);
        let point = Vec3::new(0.3, 0.5, 0.7);
        let baked_point = baked.transform_point3(point);
        let baked_normal = baked.inverse().transpose().transform_vector3(Vec3::Z);
        source[0].group_transform.translation = Vec3::new(-4.0, 2.0, 3.0);
        source[0].group_transform.rotation = glam::Quat::from_rotation_x(0.8);
        source[0].group_transform.scale = Vec3::new(2.0, 0.5, 1.5);
        let current = geometry::mesh_world_frame(&source, id);
        let pose = current * baked.inverse();
        assert!(pose.transform_point3(baked_point).abs_diff_eq(current.transform_point3(point), 1e-5));
        let normal = pose.inverse().transpose().transform_vector3(baked_normal);
        assert!(normal.dot(current.transform_vector3(Vec3::X)).abs() < 1e-5);
        assert!(normal.dot(current.transform_vector3(Vec3::Y)).abs() < 1e-5);
    }

    #[test]
    fn export_reuses_hidden_cached_geometry_with_current_pose_and_rejects_stale_or_partial_results() {
        let mut root = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        let id = root.uuid;
        let child_id = child.uuid;
        let mut state = PoissonMeshState::default();
        let mesh = Arc::new(geometry::Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0, 1, 2]], vec![id]));
        state.ready.insert(id, Ready {
            root: id, component: id, revision: 7, sample_resolution: 64, mesh_resolution: 64, algorithm: MeshAlgorithm::Poisson, baked_frame_inverse: glam::Mat4::IDENTITY,
            vertices: 3, triangles: 1, showing: false, mesh: mesh.clone(),
        });
        root.group_transform.translation = Vec3::new(3.0, 2.0, 1.0);
        root.group_transform.rotation = glam::Quat::from_rotation_y(0.7);
        root.group_transform.scale = Vec3::new(-2.0, 0.5, 1.5);
        let source = [root, child];
        let cached = state.cached_for_export(&source, id, 7, MeshAlgorithm::Poisson).unwrap();
        assert!(Arc::ptr_eq(&cached.mesh, &mesh), "export must share the already computed mesh");
        let result = cached.world_mesh(&std::sync::atomic::AtomicBool::new(false)).unwrap();
        let world = geometry::mesh_world_frame(&source, id);
        assert!(result.positions[1].abs_diff_eq(world.transform_point3(Vec3::X), 1e-5));
        assert_eq!(result.owners, mesh.owners);
        assert_eq!(result.triangles, vec![[0, 2, 1]], "reflection must preserve winding");
        let face = result.triangle(0);
        assert!((face[1] - face[0]).cross(face[2] - face[0]).dot(result.normals[0][0]) > 0.0);
        assert!(cached.world_mesh(&std::sync::atomic::AtomicBool::new(true)).is_err());
        assert!(state.cached_for_export(&source, id, 8, MeshAlgorithm::Poisson).is_none());
        assert!(state.cached_for_export(&source, child_id, 7, MeshAlgorithm::Poisson).is_none());
        let mut changed = source.to_vec();
        changed[0].poisson_mesh.resolution = 96;
        assert!(state.cached_for_export(&changed, id, 7, MeshAlgorithm::Poisson).is_none());
        changed[0].poisson_mesh.resolution = 64;
        changed[0].gaussian_splats.resolution = 96;
        assert!(state.cached_for_export(&changed, id, 7, MeshAlgorithm::Poisson).is_none());
    }
}
