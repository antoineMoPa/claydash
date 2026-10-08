use super::*;

impl Renderer {
    // Shared by foreground drawing and the native background timer. This
    // advances CPU/GPU jobs and scene resources without drawing a viewport.
    pub fn advance_computations(
        &mut self, camera: &Camera, objects: &[SdfObject], selected: &[uuid::Uuid],
        scene_versions: [i32; 2], mesh_source_revision: i32, world: World,
        egui: &egui::Context, upload_scene: bool,
    ) {
        {
            let mut mesh_changed = false;
            let actions = egui.data_mut(|data| data.remove_temp::<Vec<super::poisson_mesh::PoissonMeshAction>>(
                egui::Id::new("poisson-mesh-actions"))).unwrap_or_default();
            for action in actions {
                mesh_changed |= self.poisson_mesh.handle(action);
            }
            mesh_changed |= self.poisson_mesh.invalidate_for_revision(mesh_source_revision);
            mesh_changed |= self.poisson_mesh.poll(mesh_source_revision);
            if mesh_changed {
                self.poisson_mesh.rebuild_buffer(&self.device, objects, &mut self.mesh_buffer, &mut self.mesh_vertex_count);
                self.invalidate_scene();
            }
            if let Some(requested) = self.poisson_mesh.next_request() {
                if let Some(object) = objects.iter().find(|object| object.uuid == requested &&
                    object.render_representation == crate::model::GroupRenderRepresentation::PoissonMesh) {
                    let component = super::poisson_mesh::geometry::containing_root(objects, requested);
                    self.poisson_mesh.start(requested, component, mesh_source_revision, objects.to_vec(),
                        object.gaussian_splats.resolution, object.poisson_mesh.resolution, None,
                        #[cfg(target_arch = "wasm32")]
                        egui);
                } else {
                    self.poisson_mesh.requests.pop_front();
                }
            }
        }
        let requests = egui
            .data_mut(|data| {
                data.remove_temp::<std::collections::HashSet<uuid::Uuid>>(egui::Id::new(
                    "group-optimization-recompute",
                ))
            })
            .unwrap_or_default();
        if !requests.is_empty() {
            self.neural_jobs
                .reconcile(&objects[..objects.len().min(MAX_OBJECTS)]);
            for id in requests {
                self.cancelled_capture_keys.remove(&id);
                if self.poisson_mesh.clear_for_root(id) { self.mesh_vertex_count = 0; }
                if !self.neural_jobs.recompute(id) {
                    self.group_capture_cache.remove(&id);
                    self.group_compute_requests.insert(id);
                }
            }
            self.invalidate_scene();
        }
        let cancellations = egui
            .data_mut(|data| {
                data.remove_temp::<std::collections::HashSet<uuid::Uuid>>(egui::Id::new(
                    "group-optimization-cancel",
                ))
            })
            .unwrap_or_default();
        let mut cancelled = false;
        for id in cancellations {
            cancelled |= self.neural_jobs.cancel(id);
            self.group_compute_requests.remove(&id);
            self.poisson_mesh.handle(super::poisson_mesh::PoissonMeshAction::Cancel(id));
            #[cfg(not(target_arch = "wasm32"))]
            {
                if self.group_capture_bake.as_ref().is_some_and(|job| job.root == id) {
                    let job = self.group_capture_bake.as_ref().unwrap();
                    self.cancelled_capture_keys.insert(id, job.key);
                    self.group_capture_bake = None;
                }
            }
        }
        if cancelled {
            self.invalidate_scene();
        }
        if upload_scene {
            self.upload_scene_with_world(camera, objects, selected, scene_versions, world);
        }
        self.poisson_mesh.publish(egui, self.mesh_vertex_count > 0);
        self.neural_jobs.publish(egui);
        #[cfg(not(target_arch = "wasm32"))]
        let capture_progress = self.group_capture_bake.as_ref().map(|job| {
            (
                job.root,
                job.source_revision,
                job.work.percent(),
            )
        });
        #[cfg(target_arch = "wasm32")]
        let capture_progress = None;
        super::group_capture::publish_depth_accelerator_status(
            egui,
            objects,
            &self.group_capture_cache,
            capture_progress,
        );
        let mut computations = super::computation::Statuses::new();
        if let Some((root, status)) = self.neural_jobs.computation_status() {
            computations.insert(root, status);
        }
        if let Some((root, status)) = self.poisson_mesh.computation_status() {
            computations.insert(root, status);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(job) = &self.group_capture_bake {
                computations.insert(job.root, job.work.snapshot(super::computation::Stage::Capture, Some(job.work.percent())));
            }
        }
        egui.data_mut(|data| data.insert_temp(super::computation::status_id(), computations));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_pending_computations(&self, context: &egui::Context) -> bool {
        self.poisson_mesh.is_busy() || self.neural_jobs.is_busy()
            || self.group_capture_bake.is_some() || !self.group_compute_requests.is_empty()
            || context.data(|data| {
                data.get_temp::<Vec<poisson_mesh::PoissonMeshAction>>(egui::Id::new("poisson-mesh-actions"))
                    .is_some_and(|actions| !actions.is_empty())
                    || ["group-optimization-recompute", "group-optimization-cancel"].iter().any(|key|
                        data.get_temp::<std::collections::HashSet<uuid::Uuid>>(egui::Id::new(*key))
                            .is_some_and(|requests| !requests.is_empty()))
            })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll_background_gpu(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }
}
