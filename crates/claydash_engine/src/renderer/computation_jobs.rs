use super::*;

impl Renderer {
    // Shared by foreground drawing and the native background timer. This
    // advances CPU/GPU jobs and scene resources without drawing a viewport.
    pub fn advance_computations(
        &mut self, camera: &Camera, objects: &[SdfObject], selected: &[uuid::Uuid],
        scene_versions: [i32; 2], mesh_source_revision: i32, world: World,
        egui: &egui::Context, upload_scene: bool,
    ) {
        // A pending scene batch also owns its input buffers. Poll first and
        // defer rebuilding/uploading mesh buffers until that fence completes.
        // Otherwise slow frames can accumulate large replacement buffers even
        // though presentation itself is correctly limited to one batch.
        if self.viewport.gpu_work_pending(&self.device) || self.viewport.gpu_fault().is_some() {
            return;
        }
        {
            let voxel_actions = egui.data_mut(|data| data.remove_temp::<Vec<super::voxels::VoxelAction>>(
                egui::Id::new("voxel-actions"))).unwrap_or_default();
            for action in voxel_actions { self.voxels.handle(action); }
            let mut mesh_changed = self.voxels.advance(objects, scene_versions[0], egui);
            let actions = egui.data_mut(|data| data.remove_temp::<Vec<super::poisson_mesh::PoissonMeshAction>>(
                egui::Id::new("poisson-mesh-actions"))).unwrap_or_default();
            for action in actions {
                mesh_changed |= self.poisson_mesh.handle(action);
            }
            mesh_changed |= self.poisson_mesh.reconcile_representation(objects);
            mesh_changed |= self.poisson_mesh.invalidate_for_revision(mesh_source_revision);
            mesh_changed |= self.poisson_mesh.restore_saved(objects, mesh_source_revision);
            mesh_changed |= self.poisson_mesh.poll(mesh_source_revision);
            self.poisson_mesh.queue_missing_direct_meshes(objects, mesh_source_revision);
            if mesh_changed {
                // Scene upload rebuilds the buffer once, with the final owner
                // ordering and poses. Rebuilding here too duplicates all vertices.
                self.rebuild_mesh_color_atlas();
                self.invalidate_scene();
            }
            if let Some(requested) = self.poisson_mesh.next_request() {
                if let Some(object) = objects.iter().find(|object| object.uuid == requested &&
                    matches!(object.render_representation, crate::model::GroupRenderRepresentation::PoissonMesh | crate::model::GroupRenderRepresentation::DirectMesh)) {
                    let component = super::poisson_mesh::geometry::containing_root(objects, requested);
                    self.poisson_mesh.start(requested, component, mesh_source_revision, objects.to_vec(),
                        object.gaussian_splats.resolution, object.poisson_mesh.resolution,
                        if object.render_representation == crate::model::GroupRenderRepresentation::DirectMesh { super::poisson_mesh::MeshAlgorithm::Direct } else { super::poisson_mesh::MeshAlgorithm::Poisson }, None,
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
            self.voxels.handle(super::voxels::VoxelAction::Cancel(id));
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
        if let Some((root, status)) = self.voxels.computation_status() {
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

    pub fn has_pending_computations(&self, context: &egui::Context) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let capture_pending = self.group_capture_bake.is_some();
        #[cfg(target_arch = "wasm32")]
        let capture_pending = false;
        self.poisson_mesh.is_busy() || self.voxels.is_busy() || self.neural_jobs.is_busy()
            || capture_pending || !self.group_compute_requests.is_empty()
            || context.data(|data| {
                data.get_temp::<Vec<voxels::VoxelAction>>(egui::Id::new("voxel-actions"))
                    .is_some_and(|actions| !actions.is_empty())
                    || data.get_temp::<Vec<poisson_mesh::PoissonMeshAction>>(egui::Id::new("poisson-mesh-actions"))
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn pending_submission_defers_mesh_buffer_rebuild_and_scene_upload() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 32, height: 32, ..Default::default()
        })).unwrap();
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
        object.imported_mesh = Some(std::sync::Arc::new(poisson_mesh::geometry::Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![[0,1,2]], vec![object.uuid])));
        let source = vec![object];
        let context = egui::Context::default();
        let camera = Camera::new();
        let old_vertices = renderer.mesh_vertex_count;
        let old_objects = renderer.uploaded_object_count;
        let complete = renderer.viewport.hold_submission_for_test();
        for _ in 0..100 {
            renderer.advance_computations(&camera, &source, &[], [1,1], 1,
                World::default(), &context, true);
        }
        assert_eq!(renderer.mesh_vertex_count, old_vertices);
        assert_eq!(renderer.uploaded_object_count, old_objects);
        assert!(!renderer.poisson_mesh.shown_components().contains(&source[0].uuid));
        complete();
        renderer.advance_computations(&camera, &source, &[], [1,1], 1,
            World::default(), &context, true);
        assert!(renderer.poisson_mesh.shown_components().contains(&source[0].uuid));
        assert_eq!(renderer.mesh_vertex_count, 3);
        let uploaded = renderer.mesh_buffer.clone();
        for _ in 0..10 {
            renderer.poisson_mesh.rebuild_buffer(&renderer.device, &renderer.queue,
                &source, &mut renderer.mesh_buffer, &mut renderer.mesh_vertex_count, &renderer.voxels);
        }
        assert_eq!(renderer.mesh_buffer, uploaded, "same-size rebuilds reuse the GPU allocation");
        renderer.advance_computations(&camera, &[], &[], [2,2], 2,
            World::default(), &context, true);
        assert_eq!(renderer.mesh_vertex_count, 0);
        assert!(renderer.mesh_buffer.size() <= 48, "empty scenes release retained vertex capacity");
    }
}
