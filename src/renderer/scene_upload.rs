pub(super) use super::group_capture::*;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScenePipelinePreparation {
    Viewport,
    MaterialPreview,
    NeuralTraining,
}

mod camera;
mod objects;
mod training;
pub(super) use training::PackedTrainingScene;

impl Renderer {
    pub(super) fn upload_scene_with_world(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        world: World,
    ) {
        self.upload_scene_with_world_exposure(
            camera,
            objects,
            selected,
            scene_versions,
            1.0,
            world,
            ScenePipelinePreparation::Viewport,
        );
    }

    pub(super) fn upload_scene_with_exposure(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        exposure: f32,
    ) {
        self.upload_scene_with_world_exposure(
            camera,
            objects,
            selected,
            scene_versions,
            exposure,
            World::default(),
            ScenePipelinePreparation::MaterialPreview,
        );
    }

    pub(super) fn upload_scene_with_world_exposure(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
        exposure: f32,
        world: World,
        pipeline_preparation: ScenePipelinePreparation,
    ) -> Option<PackedTrainingScene> {
        let source_objects = &objects[..objects.len().min(MAX_OBJECTS)];
        let training_only = pipeline_preparation == ScenePipelinePreparation::NeuralTraining;
        let manage_scene_jobs = pipeline_preparation == ScenePipelinePreparation::Viewport;
        #[cfg(not(target_arch = "wasm32"))]
        if manage_scene_jobs {
            let stale_bake = self.group_capture_bake.as_mut().is_some_and(|job| {
                if job.source_revision == scene_versions[0] {
                    return false;
                }
                let stale = source_objects
                    .iter()
                    .find(|object| object.uuid == job.root)
                    .is_none_or(|object| {
                        super::group_capture::capture_key(source_objects, object) != job.key
                    });
                if !stale {
                    job.source_revision = scene_versions[0];
                }
                stale
            });
            if stale_bake {
                if let Some(job) = self.group_capture_bake.take() {
                    if source_objects.iter().any(|object| {
                        object.uuid == job.root
                            && object.render_representation
                                == crate::model::GroupRenderRepresentation::GaussianSplats
                    }) {
                        self.group_compute_requests.insert(job.root);
                    }
                }
            }
            let completed =
                self.group_capture_bake
                    .as_ref()
                    .and_then(|job| match job.receiver.try_recv() {
                        Ok(capture) => Some((job.root, job.key, job.source_revision, capture)),
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            Some((job.root, job.key, job.source_revision, None))
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    });
            if let Some((root, key, source_revision, capture)) = completed {
                self.group_capture_bake = None;
                self.group_compute_requests.remove(&root);
                if capture.is_some() || source_revision != scene_versions[0] {
                    self.uploaded_scene_versions = [i32::MIN; 2];
                }
                if capture.is_some() {
                    if let (Some(capture), Some(object)) = (
                        capture,
                        source_objects.iter().find(|object| object.uuid == root),
                    ) {
                        if super::group_capture::capture_key(source_objects, object) == key {
                            self.group_capture_cache.insert(
                                root,
                                super::group_capture::CachedGroupCapture { key, capture },
                            );
                        } else if object.render_representation
                            == crate::model::GroupRenderRepresentation::GaussianSplats
                        {
                            self.group_compute_requests.insert(root);
                        }
                    }
                }
            }
        }
        let document_changed = training_only || self.uploaded_scene_versions != scene_versions;
        if document_changed && manage_scene_jobs {
            self.neural_jobs.reconcile(source_objects);
        }
        let neural_changed = if !manage_scene_jobs {
            false
        } else {
            let mut jobs = std::mem::take(&mut self.neural_jobs);
            let changed = jobs.poll(|source, root| {
                let packed = self.pack_neural_training_scene(camera, &source, root)?;
                super::neural_sdf::GpuTrainingJob::new(
                    self.device.clone(),
                    self.queue.clone(),
                    source,
                    root,
                    packed,
                )
            });
            self.neural_jobs = jobs;
            changed
        };
        if neural_changed {
            self.viewport.invalidate();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if manage_scene_jobs
            && (document_changed || neural_changed)
            && self.group_capture_bake.is_none()
        {
            for root in source_objects.iter().filter(|object| {
                matches!(
                    object.render_representation,
                    crate::model::GroupRenderRepresentation::BoxDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereAccelerator
                        | crate::model::GroupRenderRepresentation::BoxAccelerator
                        | crate::model::GroupRenderRepresentation::GaussianSplats
                )
            }) {
                let requested = self.group_compute_requests.contains(&root.uuid);
                let gaussian = root.render_representation
                    == crate::model::GroupRenderRepresentation::GaussianSplats;
                if gaussian && !requested {
                    continue;
                }
                let key = super::group_capture::capture_key(source_objects, root);
                if !requested && self.cancelled_capture_keys.get(&root.uuid) == Some(&key) {
                    continue;
                }
                if !requested
                    && self
                        .group_capture_cache
                        .get(&root.uuid)
                        .is_some_and(|entry| entry.key == key)
                {
                    continue;
                }
                if !requested {
                    if let Some(capture) =
                        super::group_capture::restored_capture(source_objects, root, key)
                    {
                        self.group_capture_cache.insert(
                            root.uuid,
                            super::group_capture::CachedGroupCapture { key, capture },
                        );
                        continue;
                    }
                }
                self.group_capture_bake = super::group_capture::start_capture_bake(
                    source_objects,
                    root.uuid,
                    key,
                    scene_versions[0],
                );
                if self.group_capture_bake.is_some() {
                    self.group_compute_requests.remove(&root.uuid);
                    break;
                }
            }
        }
        let scene_changed = document_changed || neural_changed;
        let capture_budget = self.capture_record_budget();
        #[cfg(not(target_arch = "wasm32"))]
        let pending_bakes: std::collections::HashSet<_> = source_objects
            .iter()
            .filter(|object| {
                matches!(
                    object.render_representation,
                    crate::model::GroupRenderRepresentation::BoxDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereAccelerator
                        | crate::model::GroupRenderRepresentation::BoxAccelerator
                        | crate::model::GroupRenderRepresentation::GaussianSplats
                )
            })
            .filter(|object| {
                let requested = self.group_compute_requests.contains(&object.uuid);
                (requested
                    || object.render_representation
                        != crate::model::GroupRenderRepresentation::GaussianSplats)
                    && !self
                        .group_capture_cache
                        .get(&object.uuid)
                        .is_some_and(|entry| {
                            entry.key == super::group_capture::capture_key(source_objects, object)
                                && !requested
                        })
            })
            .map(|object| object.uuid)
            .collect();
        #[cfg(target_arch = "wasm32")]
        let pending_bakes = std::collections::HashSet::new();
        let prepared_scene = (scene_changed
            && source_objects.iter().any(|object| {
                matches!(
                    object.render_representation,
                    crate::model::GroupRenderRepresentation::BoxDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereDepthAtlas
                        | crate::model::GroupRenderRepresentation::SphereAccelerator
                        | crate::model::GroupRenderRepresentation::BoxAccelerator
                        | crate::model::GroupRenderRepresentation::GaussianSplats
                        | crate::model::GroupRenderRepresentation::NeuralSdf
                )
            }))
        .then(|| {
            prepare_group_scene_with_pending(
                source_objects,
                &mut self.group_capture_cache,
                &self.neural_jobs.ready(),
                &self.group_compute_requests,
                capture_budget,
                &pending_bakes,
            )
        });
        #[cfg(not(target_arch = "wasm32"))]
        if manage_scene_jobs {
            self.group_compute_requests.retain(|id| {
                source_objects.iter().any(|object| {
                    object.uuid == *id
                        && object.render_representation
                            == crate::model::GroupRenderRepresentation::GaussianSplats
                })
            });
        }
        #[cfg(target_arch = "wasm32")]
        if manage_scene_jobs {
            self.group_compute_requests.clear();
        }
        if let Some(prepared) = &prepared_scene {
            for id in self.neural_jobs.ready().keys() {
                if prepared.objects.iter().any(|o| o.uuid == *id)
                    && !prepared.neural_fields.contains_key(id)
                {
                    self.neural_jobs.reject_payload(*id);
                }
            }
        }
        if scene_changed && prepared_scene.is_none() && manage_scene_jobs {
            self.group_capture_cache.clear();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let mesh_components = if manage_scene_jobs { self.poisson_mesh.shown_components() }
            else { std::collections::HashSet::new() };
        let mut scene_objects_storage: Vec<SdfObject> = prepared_scene.as_ref()
            .map_or(source_objects, |prepared| prepared.objects.as_slice())
            .to_vec();
        // Mesh owners still need their original GPU material, transform and
        // stencil records. Exclude their components from the BVH below, not
        // from the object and material tables.
        #[cfg(not(target_arch = "wasm32"))]
        for object in source_objects.iter().filter(|object| {
            super::poisson_mesh::geometry::belongs_to_component(source_objects, object.uuid, &mesh_components)
        }) {
            let mut source_material_object = object.clone();
            source_material_object.render_representation =
                crate::model::GroupRenderRepresentation::ExactSdf;
            if let Some(entry) = scene_objects_storage.iter_mut().find(|entry| entry.uuid == object.uuid) {
                *entry = source_material_object;
            } else {
                scene_objects_storage.push(source_material_object);
            }
        }
        let scene_objects = scene_objects_storage.as_slice();
        let object_count = if scene_changed {
            scene_objects.iter().filter(|object| {
                #[cfg(not(target_arch = "wasm32"))]
                { !super::poisson_mesh::geometry::belongs_to_component(source_objects, object.uuid, &mesh_components) }
                #[cfg(target_arch = "wasm32")]
                { let _ = object; true }
            }).count() as u32
        } else {
            self.uploaded_object_count
        };
        let mut gpu_camera =
            camera::packed_camera(camera, exposure, object_count, self.node_count, world);
        if !training_only {
            self.queue
                .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        }
        if !scene_changed {
            return None;
        }

        // Children precede parents, so the fragment shader can evaluate the
        // boolean tree in one forward pass while preserving sibling order.
        let ordered = boolean_postorder(scene_objects);
        let objects = &ordered;
        #[cfg(not(target_arch = "wasm32"))]
        if manage_scene_jobs && self.mesh_vertex_count > 0 {
            self.poisson_mesh.rebuild_buffer(&self.device, scene_objects,
                &mut self.mesh_buffer, &mut self.mesh_vertex_count);
        }
        let mut atlases = self.upload_scene_atlases(objects, source_objects, prepared_scene.as_ref());
        let objects::PackedObjects {
            mut gpu_objects, mut bounds, distance_bound_factors, polygon_points,
            host_capacity, lattice_points, lattice_atlas_uploads, modifiers,
        } = self.pack_scene_objects(objects, scene_objects, source_objects, selected,
            prepared_scene.as_ref(), &mut atlases, training_only);
        let super::atlas_upload::UploadedAtlases { materials, splat_samples, .. } = atlases;
        let primitive_bounds = bounds.clone();
        expand_soft_bounds(&gpu_objects, &mut bounds);
        let mut component_bounds = boolean_component_bounds(&gpu_objects, &bounds);
        mark_depth_accelerator_subtrees(&mut gpu_objects);
        if !training_only {
            let source_by_id: std::collections::HashMap<_, _> = source_objects.iter()
                .map(|object| (object.uuid, object)).collect();
            let mut eligible_roots = std::collections::HashSet::new();
            let mut first = 0;
            for (root, object) in objects.iter().enumerate().take(gpu_objects.len()) {
                if gpu_objects[root].meta[3] >= 0 { continue; }
                let source_supports_split = objects[first..=root].iter().all(|part| {
                    source_by_id.get(&part.uuid).is_some_and(|source| {
                        union_split_supports_source_mode(source.render_representation)
                    })
                });
                let used_as_inlay_host = source_objects.iter().any(|part| {
                    part.surface_inlay.is_some_and(|inlay| inlay.host == object.uuid)
                });
                #[cfg(not(target_arch = "wasm32"))]
                let contains_mesh = objects[first..=root].iter()
                    .any(|part| mesh_components.contains(&part.uuid));
                #[cfg(target_arch = "wasm32")]
                let contains_mesh = false;
                if object.boolean_parent.is_none() && source_supports_split && !contains_mesh
                    && !used_as_inlay_host && object.mirror.is_none()
                    && !object.repetition.enabled && object.lattice.is_none()
                    && object.path_extrusion.is_none()
                {
                    eligible_roots.insert(root);
                }
                first = root + 1;
            }
            if pipeline_preparation == ScenePipelinePreparation::Viewport {
                split_top_level_hard_unions(&mut gpu_objects, &eligible_roots,
                    &primitive_bounds, &mut component_bounds);
            }
            flatten_nested_hard_unions(&mut gpu_objects);
        }
        // Detached union children are now independent render components. Their
        // group mirrors and repetitions must enlarge their own BVH bounds.
        for (index, object) in objects.iter().enumerate().take(gpu_objects.len()) {
            if gpu_objects[index].meta[3] >= 0 || !object.repetition.enabled {
                continue;
            }
            if !scene_objects.iter().any(|child| child.boolean_parent == Some(object.uuid)) {
                continue;
            }
            let Some(bound) = component_bounds[index].as_mut() else { continue };
            let group = crate::model::group_world_matrix(scene_objects, object.uuid);
            let local_offset = Vec3::from_array(std::array::from_fn(|axis| {
                if object.repetition.count[axis] > 1 {
                    object.repetition.count[axis].saturating_sub(1) as f32
                        * object.repetition.spacing[axis].max(0.001) * 0.5
                } else { 0.0 }
            }));
            bound.half_extent += group.x_axis.truncate().abs() * local_offset.x
                + group.y_axis.truncate().abs() * local_offset.y
                + group.z_axis.truncate().abs() * local_offset.z;
            bound.radius = bound.half_extent.length();
        }
        for (index, object) in objects.iter().enumerate().take(gpu_objects.len()) {
            if gpu_objects[index].meta[3] >= 0 { continue; }
            let Some(mirror) = object.mirror else { continue };
            let Some(bound) = component_bounds[index].as_mut() else { continue };
            *bound = mirrored_bound(*bound,
                crate::model::group_world_matrix(scene_objects, object.uuid), mirror);
        }
        let mut group_bounds = Vec::new();
        let mut group_start = 0;
        let mut capacity = host_capacity;
        let mut starts = vec![0u32; objects.len()];
        for index in 0..gpu_objects.len() {
            if gpu_objects[index].meta[3] < 0 {
                #[cfg(not(target_arch = "wasm32"))]
                let meshed = mesh_components.contains(&objects[index].uuid);
                #[cfg(target_arch = "wasm32")]
                let meshed = false;
                if let Some(bound) = component_bounds[index].filter(|_| !meshed) {
                    group_bounds.push(bound);
                }
                let march_factor = gpu_objects[group_start..=index]
                    .iter()
                    .map(|object| f32::from_bits(object.modifier[1]))
                    .fold(1.0_f32, f32::min);
                let component_capacity =
                    mark_component_evaluation(&mut gpu_objects, group_start, index);
                gpu_objects[index].modifier[1] = march_factor.to_bits();
                for object in &mut gpu_objects[group_start..=index] {
                    object.component[0] = group_start as u32;
                    object.component[1] = index as u32;
                }
                starts[index] = group_start as u32;
                capacity = capacity.max(component_capacity);
                group_start = index + 1;
            }
        }
        for bound in &group_bounds {
            let root = bound.object_index as usize;
            let start = starts[root] as usize;
            for object in &mut gpu_objects[start..=root] {
                object.component[0] = start as u32;
                object.component[1] = root as u32;
            }
        }
        let mut bvh = build_bvh(&mut group_bounds);
        let mut component_factors = vec![0.0_f32; gpu_objects.len()];
        let mut component_blend_allowance = vec![0.0_f32; gpu_objects.len()];
        for bound in &group_bounds {
            let root = bound.object_index as usize;
            let start = starts[root] as usize;
            if objects[start..=root].iter().all(|object| {
                object.mirror.is_none()
                    && object.path_extrusion.is_none()
                    && object.lattice.is_none()
                    && !object.repetition.enabled
            }) {
                component_factors[root] = distance_bound_factors[start..=root]
                    .iter()
                    .copied()
                    .fold(1.0_f32, f32::min);
                // A smooth union can lower the result by at most one quarter
                // of its parent's blend width per operand combination.
                let maximum_blend = objects[start..=root]
                    .iter()
                    .map(|object| object.softness.max(0.0))
                    .fold(0.0_f32, f32::max);
                component_blend_allowance[root] = maximum_blend * (root - start) as f32 * 0.25;
            }
        }
        for index in (0..bvh.len()).rev() {
            let (factor, blend_allowance) = if bvh[index].metadata[0] != BVH_LEAF {
                let root = bvh[index].metadata[0] as usize;
                (component_factors[root], component_blend_allowance[root])
            } else {
                let left = index + 1;
                let right = bvh[left].metadata[1] as usize;
                (
                    bvh[left].aabb_min[3].min(bvh[right].aabb_min[3]),
                    bvh[left].aabb_max[3].max(bvh[right].aabb_max[3]),
                )
            };
            bvh[index].aabb_min[3] = factor;
            bvh[index].aabb_max[3] = blend_allowance;
        }
        for node in &mut bvh {
            if node.metadata[0] != BVH_LEAF {
                node.metadata[2] = starts[node.metadata[0] as usize];
            }
        }
        let scene_node_count = bvh.len() as u32;
        append_operand_bvhs(&mut bvh, &mut gpu_objects, &starts);
        if training_only {
            return Some(PackedTrainingScene {
                objects: gpu_objects,
                bvh,
                polygon_points,
                material_headers: materials.headers,
                material_params: materials.params,
                lattice_points,
                modifier_params: modifiers.params,
                lattice_tiles: lattice_atlas_uploads,
                ids: objects.iter().map(|object| object.uuid).collect(),
                start: 0,
                root: 0,
                capacity,
            });
        }
        self.node_count = scene_node_count;
        debug_assert!(bvh.len() <= MAX_BVH_NODES);
        gpu_camera.count[1] = self.node_count;
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        // Size fragment scratch for the largest nested component. Direct
        // components stream through one accumulator.
        self.has_booleans = capacity > 1;
        let transmitted_instances: u64 = gpu_objects
            .iter()
            .filter(|object| {
                let material = materials.materials[object.component[3] as usize];
                material.opacity < 0.999 && material.metallic < 0.999
            })
            .map(|object| {
                object.repeat_count[..3]
                    .iter()
                    .map(|&count| count.max(1) as u64)
                    .fold(1u64, u64::saturating_mul)
            })
            .fold(0u64, u64::saturating_add);
        // The mesh path has one depth value for the opaque SDF surface. Keep
        // the ray compositor for glass, reflected splats, custom shaders and
        // group modifiers whose copies cannot be represented by one instance.
        let supported_materials = source_objects.iter().all(|object| {
            object.material.kind == MaterialKind::Solid
                && object.material.opacity >= 0.999
                && object.image_stencil.is_none()
                && object.surface_inlay.is_none()
        });
        let supported_roots = prepared_scene.as_ref().is_some_and(|prepared| {
            prepared.gaussian_splats.iter().all(|id| {
                source_objects
                    .iter()
                    .find(|object| object.uuid == *id)
                    .is_some_and(|root| {
                        root.boolean_parent.is_none()
                            && root.operation == crate::model::BooleanOperation::Union
                            && !root.repetition.enabled
                            && root.mirror.is_none()
                    })
            })
        });
        let has_splat_samples = !splat_samples.is_empty();
        self.hybrid_enabled = self.use_bvh
            && self.shader_source.contains("fn fs_depth")
            && supported_materials
            && supported_roots
            && (has_splat_samples || self.mesh_is_visible());
        self.splat_instances.clear();
        if self.hybrid_enabled {
            let matrices: std::collections::HashMap<_, _> = splat_samples
                .iter()
                .map(|sample| sample.0)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .map(|id| (id, crate::model::object_world_matrix(scene_objects, id)))
                .collect();
            for (id, center, support, color, normal, material_index) in splat_samples {
                #[cfg(not(target_arch = "wasm32"))]
                if self.poisson_mesh.shown_root() == Some(id) { continue; }
                let matrix = matrices[&id];
                let scale = matrix
                    .x_axis
                    .truncate()
                    .length()
                    .max(matrix.y_axis.truncate().length())
                    .max(matrix.z_axis.truncate().length());
                let world_normal = matrix
                    .inverse()
                    .transpose()
                    .transform_vector3(normal)
                    .normalize_or_zero();
                self.splat_instances.push(GpuSplat {
                    center_radius: matrix
                        .transform_point3(center)
                        .extend(support * scale)
                        .to_array(),
                    color_opacity: [
                        color[0],
                        color[1],
                        color[2],
                        materials.materials[material_index as usize].opacity,
                    ],
                    normal: world_normal.extend(0.0).to_array(),
                });
            }
        }
        self.initial_pixel_budget =
            if (capacity > 1 && gpu_objects.len() > 256) || transmitted_instances > 1024 {
                1024
            } else if (capacity > 1 && gpu_objects.len() > 64) || transmitted_instances > 256 {
                4096
            } else if transmitted_instances > 64 {
                16 * 1024
            } else {
                48 * 1024
            };
        self.viewport.set_initial_budget(self.initial_pixel_budget);
        self.resize_lattice_atlas_for(lattice_atlas_uploads.len());
        let mut shader_features =
            SceneShaderFeatures::for_scene(&materials.materials, &gpu_objects);
        shader_features.neural_sdf |= prepared_scene
            .as_ref()
            .is_some_and(|prepared| !prepared.neural_fields.is_empty());
        shader_features.neural_width = prepared_scene.as_ref().map_or(4, |prepared| {
            prepared
                .neural_fields
                .values()
                .map(|field| field.network.width())
                .max()
                .unwrap_or(4)
        });
        self.deferred_supported = self.use_bvh
            && self.shader_source.contains("fn fs_gbuffer")
            && (!has_splat_samples || self.hybrid_enabled)
            && crate::model::deferred_fallback_reason(source_objects).is_none();
        // Thumbnail draws use their dedicated sphere pipeline. Uploading their
        // temporary objects must not compile viewport variants for every preset.
        if pipeline_preparation == ScenePipelinePreparation::Viewport {
            self.prepare_scene_pipelines(world, capacity, shader_features);
        }
        if !gpu_objects.is_empty() {
            self.queue.write_buffer(
                &self.material_headers_buffer,
                0,
                bytemuck::cast_slice(&materials.headers),
            );
            self.queue.write_buffer(
                &self.material_params_buffer,
                0,
                bytemuck::cast_slice(&materials.params),
            );
            if !polygon_points.is_empty() {
                self.queue.write_buffer(
                    &self.polygon_points_buffer,
                    0,
                    bytemuck::cast_slice(&polygon_points),
                );
            }
            if !lattice_points.is_empty() {
                self.queue.write_buffer(
                    &self.lattice_points_buffer,
                    0,
                    bytemuck::cast_slice(&lattice_points),
                );
            }
            for (tile, resolution, offsets) in lattice_atlas_uploads {
                let texels: Vec<u16> = offsets
                    .iter()
                    .flat_map(|offset| {
                        [offset.x, offset.y, offset.z, 0.0]
                            .map(|value| half::f16::from_f32(value).to_bits())
                    })
                    .collect();
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.lattice_atlas,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: tile % LATTICE_ATLAS_TILES_PER_ROW * LATTICE_ATLAS_TILE_PITCH + 1,
                            y: tile / LATTICE_ATLAS_TILES_PER_ROW * LATTICE_ATLAS_TILE_PITCH + 1,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&texels),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(resolution * 8),
                        rows_per_image: Some(resolution),
                    },
                    wgpu::Extent3d {
                        width: resolution,
                        height: resolution,
                        depth_or_array_layers: resolution,
                    },
                );
            }
            if !modifiers.headers.is_empty() {
                self.queue.write_buffer(
                    &self.modifier_params_buffer,
                    0,
                    bytemuck::cast_slice(&modifiers.params),
                );
            }
            self.queue
                .write_buffer(&self.objects_buffer, 0, bytemuck::cast_slice(&gpu_objects));
            self.queue
                .write_buffer(&self.bvh_buffer, 0, bytemuck::cast_slice(&bvh));
        }
        self.uploaded_scene_versions = scene_versions;
        self.uploaded_object_count = object_count;
        None
    }
}
