use super::*;
use super::computation::{Computation, Stage};
use std::{collections::{HashMap, VecDeque}, sync::{mpsc, Arc}};
#[cfg(test)]
use std::sync::atomic::Ordering;
use wgpu::util::DeviceExt;

pub(crate) mod geometry;
pub(crate) mod glb;
pub(crate) mod textured_glb;

#[derive(Clone, Copy)]
pub(crate) enum PoissonMeshAction {
    Build(uuid::Uuid),
    Cancel(uuid::Uuid),
    Show(uuid::Uuid),
    Hide(uuid::Uuid),
}

#[derive(Clone)]
pub(crate) enum PoissonMeshStatus {
    Sampling { percent: u32 },
    Reconstructing,
    Ready { vertices: usize, triangles: usize, showing: bool, available: bool },
    Failed(String),
}

struct Job {
    root: uuid::Uuid,
    component: uuid::Uuid,
    revision: i32,
    baked_frame_inverse: glam::Mat4,
    work: Computation,
    receiver: mpsc::Receiver<Result<geometry::Mesh, String>>,
    invalidated: bool,
}

struct Ready {
    root: uuid::Uuid,
    component: uuid::Uuid,
    revision: i32,
    baked_frame_inverse: glam::Mat4,
    vertices: usize,
    triangles: usize,
    showing: bool,
    mesh: Arc<geometry::Mesh>,
}

pub(crate) struct CachedExportMesh {
    mesh: Arc<geometry::Mesh>,
    pose: glam::Mat4,
}

impl CachedExportMesh {
    pub fn world_mesh(&self, cancel: &std::sync::atomic::AtomicBool) -> Result<geometry::Mesh, String> {
        let mut mesh = (*self.mesh).clone();
        let normal_pose = self.pose.inverse().transpose();
        let reflected = self.pose.determinant() < 0.0;
        let mut budget = super::cooperative_work::WorkerBudget::new();
        for (index, point) in mesh.positions.iter_mut().enumerate() {
            if index % 512 == 0 {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) { return Err("Export cancelled".into()); }
                budget.checkpoint();
            }
            *point = self.pose.transform_point3(*point);
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
}

impl Renderer {
    pub(crate) fn cached_mesh_for_export(&self, source: &[SdfObject], root: uuid::Uuid, revision: i32) -> Option<CachedExportMesh> {
        self.poisson_mesh.cached_for_export(source, root, revision)
    }
}

#[derive(Default)]
pub(super) struct PoissonMeshState {
    pub requests: VecDeque<uuid::Uuid>,
    job: Option<Job>,
    ready: HashMap<uuid::Uuid, Ready>,
    failure: HashMap<uuid::Uuid, String>,
}

impl PoissonMeshState {
    fn cached_for_export(&self, source: &[SdfObject], root: uuid::Uuid, revision: i32) -> Option<CachedExportMesh> {
        // A cached whole-component Boolean result cannot safely be sliced by
        // triangle owner to export an arbitrary selected child subtree.
        let ready = self.ready.values().find(|ready| ready.component == root && ready.revision == revision)?;
        Some(CachedExportMesh { mesh: ready.mesh.clone(),
            pose: geometry::mesh_world_frame(source, root) * ready.baked_frame_inverse })
    }
    pub fn next_request(&self) -> Option<uuid::Uuid> {
        self.job.is_none().then(|| self.requests.front().copied()).flatten()
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
                self.requests.retain(|id| *id != root);
                if self.job.as_ref().is_some_and(|job| job.root == root) {
                    self.job.take(); // Drop signals cancellation; late results are discarded.
                }
                self.failure.remove(&root);
                false
            }
            PoissonMeshAction::Build(root) => {
                if self.job.as_ref().is_some_and(|job| job.root == root) { return false; }
                if !self.requests.contains(&root) { self.requests.push_back(root); }
                self.failure.remove(&root);
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

    pub fn invalidate_for_revision(&mut self, revision: i32) -> bool {
        let original = self.ready.len();
        self.ready.retain(|_, ready| ready.revision == revision);
        if self.job.as_ref().is_some_and(|job| job.revision != revision && !job.invalidated) {
            if let Some(job) = self.job.as_mut() {
                job.invalidated = true;
                job.work.cancel();
            }
            return true;
        }
        self.ready.len() != original
    }

    pub fn clear_for_root(&mut self, root: uuid::Uuid) -> bool {
        let active = self.requests.contains(&root)
            || self.job.as_ref().is_some_and(|job| job.root == root)
            || self.ready.contains_key(&root)
            || self.failure.contains_key(&root);
        if active {
            self.requests.retain(|id| *id != root);
            if let Some(job) = self.job.as_mut().filter(|job| job.root == root) {
                job.invalidated = true;
                job.work.cancel();
            }
            self.ready.remove(&root);
            self.failure.remove(&root);
        }
        active
    }

    pub fn start(&mut self, root: uuid::Uuid, component: uuid::Uuid, revision: i32,
        source: Vec<SdfObject>, resolution: u32,
        cached: Option<Arc<super::box_depth_atlas::BoxDepthAtlas>>,
    ) {
        self.requests.retain(|id| *id != root);
        let (sender, receiver) = mpsc::channel();
        let work = Computation::new(Stage::Sampling);
        let baked_frame_inverse = geometry::mesh_world_frame(&source, component).inverse();
        let worker_progress = work.progress.clone();
        let worker_cancel = work.cancel.clone();
        std::thread::spawn(move || {
            super::cooperative_work::background_priority();
            let result = std::panic::catch_unwind(|| geometry::build(&source, component, resolution, cached, &worker_progress, Some(&worker_cancel)))
                .unwrap_or_else(|_| Err("Poisson reconstruction failed unexpectedly".into()));
            let _ = sender.send(result);
        });
        self.job = Some(Job { root, component, revision, baked_frame_inverse, work, receiver, invalidated: false });
    }

    pub fn poll(&mut self, revision: i32) -> bool {
        let Some(job) = self.job.as_ref() else { return false; };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("Poisson reconstruction worker stopped".into()),
        };
        let job = self.job.take().unwrap();
        if job.invalidated || job.revision != revision { return false; }
        match result {
            Ok(mesh) if !mesh.triangles.is_empty() => {
                self.ready.retain(|_, ready| ready.component != job.component);
                self.ready.insert(job.root, Ready {
                    root: job.root, component: job.component, revision, vertices: mesh.positions.len(),
                    baked_frame_inverse: job.baked_frame_inverse,
                    triangles: mesh.triangles.len(),
                    showing: true, mesh: Arc::new(mesh),
                });
            }
            Ok(_) => { self.failure.insert(job.root, "Poisson reconstruction returned an empty mesh".into()); }
            Err(error) => { self.failure.insert(job.root, error); }
        }
        true
    }

    pub fn rebuild_buffer(&self, device: &wgpu::Device, source: &[SdfObject], buffer: &mut wgpu::Buffer, count: &mut u32) {
        let owners: HashMap<_, _> = super::bvh::boolean_postorder(source).iter().enumerate()
            .map(|(index, object)| (object.uuid, index as u32)).collect();
        let capacity = self.ready.values().filter(|ready| ready.showing)
            .map(|ready| ready.mesh.triangles.len()).sum::<usize>() * 3;
        let mut vertices = Vec::with_capacity(capacity);
        for ready in self.ready.values().filter(|ready| ready.showing) {
            let pose = geometry::mesh_world_frame(source, ready.component) * ready.baked_frame_inverse;
            let normal_pose = pose.inverse().transpose();
            for (triangle_index, triangle) in ready.mesh.triangles.iter().enumerate() {
                let Some(&owner) = owners.get(&ready.mesh.owners[triangle_index]) else { continue; };
                let points = triangle.map(|index| pose.transform_point3(ready.mesh.positions[index as usize]));
                for (corner, point) in points.into_iter().enumerate() {
                    let normal = normal_pose.transform_vector3(ready.mesh.normals[triangle_index][corner]);
                    vertices.push(super::hybrid_splats::GpuMeshVertex {
                        position: point.extend(1.0).to_array(), normal: normal.extend(owner as f32).to_array(),
                    });
                }
            }
        }
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
        let percent = job.work.percent();
        Some((job.root, if percent < 100 { job.work.snapshot(Stage::Sampling, Some(percent)) }
            else { job.work.snapshot(Stage::Reconstruction, None) }))
    }

    pub fn publish(&self, context: &egui::Context, available: bool) {
        let mut statuses = std::collections::HashMap::new();
        for ready in self.ready.values() {
            statuses.insert(ready.root, PoissonMeshStatus::Ready {
                vertices: ready.vertices, triangles: ready.triangles,
                showing: ready.showing, available,
            });
        }
        for root in &self.requests { statuses.insert(*root, PoissonMeshStatus::Sampling { percent: 0 }); }
        if let Some(job) = self.job.as_ref().filter(|job| !job.invalidated) {
            let percent = job.work.percent();
            statuses.insert(job.root, if percent < 100 {
                PoissonMeshStatus::Sampling { percent }
            } else { PoissonMeshStatus::Reconstructing });
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
    fn cached_mesh_selection_is_explicit() {
        let state = PoissonMeshState::default();
        assert!(state.shown_root().is_none());
    }

    #[test]
    fn recompute_preserves_display_and_publishes_progress_over_ready_mesh() {
        let mut state = PoissonMeshState::default();
        let root = uuid::Uuid::new_v4();
        state.ready.insert(root, Ready {
            root, component: root, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
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
            root, component: root, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY,
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
            root, component: root, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
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
            root, component: root, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY,
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
            root, component: root, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY, vertices: 3, triangles: 1,
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
            root: id, component: id, revision: 7, baked_frame_inverse: glam::Mat4::IDENTITY,
            vertices: 3, triangles: 1, showing: false, mesh: mesh.clone(),
        });
        root.group_transform.translation = Vec3::new(3.0, 2.0, 1.0);
        root.group_transform.rotation = glam::Quat::from_rotation_y(0.7);
        root.group_transform.scale = Vec3::new(-2.0, 0.5, 1.5);
        let source = [root, child];
        let cached = state.cached_for_export(&source, id, 7).unwrap();
        assert!(Arc::ptr_eq(&cached.mesh, &mesh), "export must share the already computed mesh");
        let result = cached.world_mesh(&std::sync::atomic::AtomicBool::new(false)).unwrap();
        let world = geometry::mesh_world_frame(&source, id);
        assert!(result.positions[1].abs_diff_eq(world.transform_point3(Vec3::X), 1e-5));
        assert_eq!(result.owners, mesh.owners);
        assert_eq!(result.triangles, vec![[0, 2, 1]], "reflection must preserve winding");
        let face = result.triangle(0);
        assert!((face[1] - face[0]).cross(face[2] - face[0]).dot(result.normals[0][0]) > 0.0);
        assert!(cached.world_mesh(&std::sync::atomic::AtomicBool::new(true)).is_err());
        assert!(state.cached_for_export(&source, id, 8).is_none());
        assert!(state.cached_for_export(&source, child_id, 7).is_none());
    }
}
