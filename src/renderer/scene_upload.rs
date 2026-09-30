pub(super) use super::group_capture::*;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScenePipelinePreparation {
    Viewport,
    MaterialPreview,
    NeuralTraining,
}

pub(super) struct PackedTrainingScene {
    pub objects: Vec<GpuObject>,
    pub bvh: Vec<GpuBvhNode>,
    pub polygon_points: Vec<GpuPolygonPoint>,
    pub material_headers: Vec<material_gpu::GpuMaterialHeader>,
    pub material_params: Vec<[f32; 4]>,
    pub lattice_points: Vec<[f32; 4]>,
    pub modifier_params: Vec<[f32; 4]>,
    pub lattice_tiles: Vec<(u32, u32, Vec<Vec3>)>,
    pub ids: Vec<uuid::Uuid>,
    pub start: u32,
    pub root: u32,
    pub capacity: u32,
}

impl Renderer {
    pub(super) fn pack_neural_training_scene(
        &mut self,
        camera: &Camera,
        source: &[SdfObject],
        root: uuid::Uuid,
    ) -> Result<PackedTrainingScene, &'static str> {
        let mut exact = source.to_vec();
        for object in &mut exact {
            object.render_representation = crate::model::GroupRenderRepresentation::ExactSdf;
        }
        let mut packed = self
            .upload_scene_with_world_exposure(
                camera,
                &exact,
                &[],
                [i32::MIN; 2],
                1.0,
                World::default(),
                ScenePipelinePreparation::NeuralTraining,
            )
            .ok_or("Could not prepare GPU training scene")?;
        let root_index = packed
            .ids
            .iter()
            .position(|id| *id == root)
            .ok_or("Missing training root")?;
        let descendants: std::collections::HashSet<_> = source
            .iter()
            .filter(|object| {
                let mut id = Some(object.uuid);
                for _ in 0..source.len() {
                    if id == Some(root) {
                        return true;
                    }
                    id = id
                        .and_then(|id| source.iter().find(|object| object.uuid == id))
                        .and_then(|object| object.boolean_parent);
                }
                false
            })
            .map(|object| object.uuid)
            .collect();
        let start = packed
            .ids
            .iter()
            .position(|id| descendants.contains(id))
            .ok_or("Missing training subtree")?;
        let capacity = mark_component_evaluation(&mut packed.objects, start, root_index);
        packed.start = start as u32;
        packed.root = root_index as u32;
        packed.capacity = packed.capacity.max(capacity);
        Ok(packed)
    }

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

    fn upload_scene_with_world_exposure(
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
        let document_changed = training_only || self.uploaded_scene_versions != scene_versions;
        if document_changed && !training_only {
            self.neural_jobs.reconcile(source_objects);
        }
        let neural_changed = if training_only {
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
        let scene_changed = document_changed || neural_changed;
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
            prepare_group_scene_with_neural(
                source_objects,
                &mut self.group_capture_cache,
                &self.neural_jobs.ready(),
                &self.group_compute_requests,
            )
        });
        if !training_only {
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
        if scene_changed && prepared_scene.is_none() && !training_only {
            self.group_capture_cache.clear();
        }
        let scene_objects = prepared_scene
            .as_ref()
            .map_or(source_objects, |prepared| prepared.objects.as_slice());
        let object_count = if scene_changed {
            scene_objects.len() as u32
        } else {
            self.uploaded_object_count
        };
        let mut gpu_camera = GpuCamera {
            inverse_view_projection: (camera.projection() * camera.view())
                .inverse()
                .to_cols_array_2d(),
            view_projection: (camera.projection() * camera.view()).to_cols_array_2d(),
            position: camera.position.extend(exposure).to_array(),
            count: [
                object_count,
                self.node_count,
                camera.viewport.y.round().max(1.0) as u32,
                u32::from(camera.projection_mode == ProjectionMode::Orthographic),
            ],
            world_mode: [
                world.background.shader_id(),
                u32::from(world.screen_space_ambient_occlusion),
                u32::from(world.screen_space_reflections),
                0,
            ],
            world_color: [
                world.flat_color[0],
                world.flat_color[1],
                world.flat_color[2],
                1.0,
            ],
            sun_direction: {
                let direction = world.sun_direction();
                [
                    direction[0],
                    direction[1],
                    direction[2],
                    world.sun_intensity,
                ]
            },
            sky_params: [world.turbidity, world.sun_temperature, 0.0, 0.0],
        };
        if !training_only {
            self.queue
                .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        }
        if !scene_changed {
            return None;
        }

        // Children precede parents, so the fragment shader can evaluate the
        // boolean tree in one forward pass while preserving sibling order.
        let selected_ids = visible_selection(source_objects, scene_objects, selected);
        let scene_lookup: std::collections::HashMap<_, _> = scene_objects
            .iter()
            .map(|object| (object.uuid, object))
            .collect();
        let ordered = boolean_postorder(scene_objects);
        let objects = &ordered;
        let super::atlas_upload::UploadedAtlases {
            box_depth_metadata,
            splat_bvh_metadata,
            depth_accelerator_transforms,
            splat_samples,
            mut materials,
            stencil_layers,
        } = self.upload_scene_atlases(objects, source_objects, prepared_scene.as_ref());
        let object_indices: std::collections::HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as i32))
            .collect();

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut polygon_points = Vec::new();
        let mut text_points_used = 0usize;
        let mut lattice_points: Vec<[f32; 4]> = Vec::new();
        let mut lattice_atlas_uploads = Vec::new();
        let mut modifiers = modifier_gpu::PackedModifiers::default();
        let mut lattice_slots = std::collections::HashMap::new();
        let mut gpu_objects: Vec<GpuObject> = objects
            .iter()
            .take(MAX_OBJECTS)
            .enumerate()
            .map(|(index, object)| {
                let mut ancestor = Some(object.uuid);
                let mut cage = None;
                for _ in 0..scene_objects.len() {
                    let Some(id) = ancestor else {
                        break;
                    };
                    let Some(candidate) = scene_lookup.get(&id).copied() else {
                        break;
                    };
                    if let Some(lattice) = &candidate.lattice {
                        let n = lattice.resolution as usize;
                        if (2..=9).contains(&n) && lattice.offsets.len() == n * n * n {
                            cage = Some((id, lattice));
                            break;
                        }
                    }
                    ancestor = candidate.boolean_parent;
                }
                let (modifier_index, march_factor, cage_extent) =
                    if let Some((root, lattice)) = cage {
                        *lattice_slots.entry(root).or_insert_with(|| {
                            if lattice.offsets.iter().all(|offset| *offset == Vec3::ZERO) {
                                return (0, 0.8, Vec3::ZERO);
                            }
                            let effective_offsets = lattice.effective_offsets();
                            let control_offset = lattice_points.len() as u32;
                            lattice_points.extend(
                                effective_offsets
                                    .iter()
                                    .map(|point| point.extend(0.0).to_array()),
                            );
                            let (tile, inverse_min, inverse_max) = if lattice.resolution <= 3 {
                                let tile = lattice_atlas_uploads.len() as u32;
                                let (inverse_points, inverse_min, inverse_max) =
                                    modifier_gpu::inverse_lattice_grid(lattice, &effective_offsets);
                                lattice_atlas_uploads.push((
                                    tile,
                                    modifier_gpu::INVERSE_GRID_RESOLUTION as u32,
                                    inverse_points,
                                ));
                                (tile, inverse_min, inverse_max)
                            } else {
                                let tile = lattice_atlas_uploads.len() as u32;
                                lattice_atlas_uploads.push((
                                    tile,
                                    lattice.resolution as u32,
                                    effective_offsets.clone(),
                                ));
                                (tile, lattice.min, lattice.max)
                            };
                            let forward = crate::model::lattice_world_matrix(scene_objects, root);
                            let march_factor = modifier_gpu::lattice_march_factor(
                                lattice,
                                &effective_offsets,
                                forward,
                            );
                            let maximum = lattice
                                .offsets
                                .iter()
                                .fold(Vec3::ZERO, |maximum, offset| maximum.max(offset.abs()));
                            let world_extent = forward.x_axis.truncate().abs() * maximum.x
                                + forward.y_axis.truncate().abs() * maximum.y
                                + forward.z_axis.truncate().abs() * maximum.z;
                            let index = modifiers.insert_lattice(
                                inverse_affine_rows(forward.inverse()),
                                inverse_affine_rows(forward),
                                inverse_min,
                                inverse_max,
                                tile,
                                if lattice.resolution <= 3 {
                                    modifier_gpu::INVERSE_GRID_RESOLUTION as u32
                                } else {
                                    lattice.resolution as u32
                                },
                                control_offset,
                                lattice.resolution as u32,
                                lattice.min,
                                lattice.max,
                            );
                            (index, march_factor, world_extent)
                        })
                    } else {
                        (0, 0.8, Vec3::ZERO)
                    };
                let matrix = crate::model::object_world_matrix(scene_objects, object.uuid);
                let group_matrix = crate::model::group_world_matrix(scene_objects, object.uuid);
                let repeats_group = object.repetition.enabled
                    && scene_objects
                        .iter()
                        .any(|child| child.boolean_parent == Some(object.uuid));
                let abs_scale = Vec3::new(
                    matrix.x_axis.truncate().length(),
                    matrix.y_axis.truncate().length(),
                    matrix.z_axis.truncate().length(),
                );
                let distance_scale = abs_scale.min_element().max(0.000_001);
                let super::primitive_upload::PackedPrimitive {
                    params,
                    radius,
                    path_profile_extent,
                    path_radius,
                    path_profile_count,
                    text_geometry,
                } = super::primitive_upload::pack_primitive(
                    object,
                    scene_objects,
                    source_objects,
                    abs_scale,
                    distance_scale,
                    &mut polygon_points,
                    &mut text_points_used,
                );
                let inlay_host = object.surface_inlay.and_then(|inlay| {
                    object_indices
                        .get(&inlay.host)
                        .copied()
                        .map(|host| (host, inlay))
                });
                let inlay_parameters = inlay_host.map(|(_, inlay)| {
                    let offset = polygon_points.len() as u32;
                    polygon_points.push(GpuPolygonPoint {
                        position: [inlay.offset, inlay.thickness],
                    });
                    offset
                });
                let repeated_radius = if object.repetition.enabled && !repeats_group {
                    let extent = Vec3::from_array([
                        if object.repetition.count[0] > 1 {
                            (object.repetition.count[0].saturating_sub(1)) as f32
                                * object.repetition.spacing.x
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.count[1] > 1 {
                            (object.repetition.count[1].saturating_sub(1)) as f32
                                * object.repetition.spacing.y
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.count[2] > 1 {
                            (object.repetition.count[2].saturating_sub(1)) as f32
                                * object.repetition.spacing.z
                                * 0.5
                        } else {
                            0.0
                        },
                    ]);
                    radius + extent.length() * abs_scale.max_element()
                } else {
                    radius
                };
                let local_extent = match object.params {
                    SdfParams::SphereParams(ref p) => Vec3::splat(p.radius),
                    SdfParams::BoxParams(ref p) => p.box_q,
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => Vec3::new(radius, half_height, radius),
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => Vec3::new(
                        major_radius + minor_radius,
                        minor_radius,
                        major_radius + minor_radius,
                    ),
                    SdfParams::PolygonPrismParams(ref polygon) => {
                        let planar = polygon
                            .vertices
                            .iter()
                            .fold(Vec2::ZERO, |extent, point| extent.max(point.abs()));
                        Vec3::new(planar.x, planar.y, polygon.half_depth)
                    }
                    SdfParams::LoftParams(ref loft) => loft.local_extent(),
                    SdfParams::BezierCurveParams(ref curve) => {
                        curve.local_extent(path_radius * path_profile_extent)
                    }
                    SdfParams::TextParams(_) => text_geometry
                        .as_ref()
                        .map_or(Vec3::ZERO, |text| text.local_extent()),
                };
                let mut repeated_extent = local_extent;
                if object.repetition.enabled && !repeats_group {
                    for axis in 0..3 {
                        if object.repetition.count[axis] > 1 {
                            repeated_extent[axis] += object.repetition.count[axis].saturating_sub(1)
                                as f32
                                * object.repetition.spacing[axis].max(0.001)
                                * 0.5;
                        }
                    }
                }
                let half_extent = matrix.x_axis.truncate().abs() * repeated_extent.x
                    + matrix.y_axis.truncate().abs() * repeated_extent.y
                    + matrix.z_axis.truncate().abs() * repeated_extent.z
                    + cage_extent;
                let bound = ObjectBound {
                    half_extent,
                    center: matrix.transform_point3(Vec3::ZERO),
                    radius: repeated_radius + cage_extent.length(),
                    object_index: index as u32,
                };
                bounds.push(bound);
                let uniform_scale = (abs_scale.max_element() - abs_scale.min_element())
                    <= abs_scale.max_element() * 0.00001;
                let box_depth = box_depth_metadata.get(&object.uuid).copied();
                let neural_accelerator_offset = if !training_only
                    && depth_accelerator_transforms.contains_key(&object.uuid)
                {
                    prepared_scene
                        .as_ref()
                        .and_then(|prepared| prepared.neural_fields.get(&object.uuid))
                        .filter(|field| field.raymarch_last_segment)
                        .map(|field| field.distance_offset)
                } else {
                    None
                };
                let splat_bvh = splat_bvh_metadata.get(&object.uuid).copied();
                let safe_distance_bound = box_depth.is_none()
                    && uniform_scale
                    && matches!(
                        &object.params,
                        SdfParams::SphereParams(_)
                            | SdfParams::BoxParams(_)
                            | SdfParams::CylinderParams { .. }
                            | SdfParams::TorusParams { .. }
                            | SdfParams::PolygonPrismParams(_)
                    );
                let custom_index = if object.material.kind == MaterialKind::Custom {
                    object
                        .material_id
                        .and_then(|id| {
                            self.custom_material_sources
                                .iter()
                                .position(|(asset_id, _)| *asset_id == id)
                        })
                        .map_or(0, |index| index as u32 + 1)
                } else {
                    0
                };
                let material_index = materials.insert_custom(object.material, custom_index);
                GpuObject {
                    distance_bound: bound
                        .center
                        .extend(if safe_distance_bound {
                            repeated_radius + 0.01
                        } else {
                            -1.0
                        })
                        .to_array(),
                    operand_tree: [
                        0,
                        0,
                        0,
                        if let Some(offset) = neural_accelerator_offset {
                            offset.max(0.1).to_bits()
                        } else if !training_only
                            && depth_accelerator_transforms.contains_key(&object.uuid)
                        {
                            object
                                .depth_accelerator_settings()
                                .filter(|settings| settings.is_valid())
                                .map_or(0, |settings| settings.configurable_epsilon.to_bits())
                        } else {
                            0
                        },
                    ],
                    box_depth_meta: box_depth.map_or([0; 4], |(offset, width, height, _, _, _)| {
                        [
                            offset,
                            width,
                            height,
                            depth_accelerator_transforms
                                .get(&object.uuid)
                                .copied()
                                .unwrap_or_else(|| splat_bvh.map_or(0, |(start, _)| start)),
                        ]
                    }),
                    box_depth_min: box_depth.map_or([0.0; 4], |(_, _, _, minimum, _, _)| {
                        minimum
                            .extend(splat_bvh.map_or(0.0, |(_, end)| end as f32))
                            .to_array()
                    }),
                    box_depth_max: box_depth.map_or([0.0; 4], |(_, _, _, _, maximum, _)| {
                        maximum.extend(0.0).to_array()
                    }),
                    // Spare component lanes carry blend width and material index.
                    component: [0, 0, object.softness.to_bits(), material_index],
                    scale: abs_scale
                        .extend(match &object.params {
                            SdfParams::BoxParams(box_params) => box_params.corner_radius,
                            _ => path_profile_count,
                        })
                        .to_array(),
                    meta: [
                        i32::from(selected_ids.contains(&object.uuid)),
                        if object.render_representation.is_depth_accelerator()
                            || neural_accelerator_offset.is_some()
                        {
                            object.object_type
                        } else {
                            box_depth.map_or(object.object_type, |(_, _, _, _, _, kind)| kind)
                        },
                        object.operation.gpu_code(),
                        object
                            .boolean_parent
                            .and_then(|parent| object_indices.get(&parent).copied())
                            .unwrap_or(-1),
                    ],
                    color: object.color.to_array(),
                    stencil_placement: object.image_stencil.as_ref().map_or([0.0; 4], |stencil| {
                        [
                            stencil.size.x,
                            stencil.size.y,
                            stencil.offset.x,
                            stencil.offset.y,
                        ]
                    }),
                    stencil_meta: object.image_stencil.as_ref().map_or([0.0; 4], |stencil| {
                        [
                            stencil_layers
                                .get(&object.uuid)
                                .map_or(0.0, |layer| *layer as f32 + 1.0),
                            stencil.rotation.to_radians(),
                            f32::from(stencil.both_sides),
                            f32::from(stencil.image_plane),
                        ]
                    }),
                    inverse_rows: inverse_affine_rows(matrix.inverse()),
                    group_inverse_rows: inverse_affine_rows(group_matrix.inverse()),
                    params,
                    repeat_spacing: object
                        .repetition
                        .spacing
                        .extend(inlay_parameters.map_or(0.0, f32::from_bits))
                        .to_array(),
                    repeat_count: if object.repetition.enabled {
                        [
                            object.repetition.count[0] as i32,
                            object.repetition.count[1] as i32,
                            object.repetition.count[2] as i32,
                            1,
                        ]
                    } else {
                        [1, 1, 1, 0]
                    },
                    modifier: [
                        modifier_index,
                        (if matches!(object.params, SdfParams::BezierCurveParams(_))
                            && path_profile_count == 0.0
                            && modifier_index == 0
                        {
                            1.0
                        } else if matches!(object.params, SdfParams::BezierCurveParams(_))
                            && path_profile_count != 0.0
                        {
                            march_factor.min(0.5)
                        } else if let SdfParams::LoftParams(loft) = &object.params {
                            march_factor.min(loft.march_factor())
                        } else {
                            if box_depth.is_some() {
                                march_factor.min(0.35)
                            } else {
                                march_factor
                            }
                        })
                        .to_bits(),
                        if modifier_index == 0 {
                            0
                        } else {
                            modifiers.parameter_offset(modifier_index)
                        },
                        if modifier_index == 0 {
                            0
                        } else {
                            modifier_gpu::MODIFIER_LATTICE
                        },
                    ],
                    mirror_axes: {
                        let mut axes = object.mirror.map_or([0; 4], |mirror| {
                            [
                                u32::from(mirror.axes[0]),
                                u32::from(mirror.axes[1]),
                                u32::from(mirror.axes[2]),
                                0,
                            ]
                        });
                        if let SdfParams::BezierCurveParams(curve) = &object.params {
                            axes[3] = curve
                                .segment_count()
                                .min(crate::model::BezierCurveParams::MAX_SEGMENTS)
                                as u32
                                | if curve.closed { 0x8000_0000 } else { 0 };
                        }
                        if !matches!(object.params, SdfParams::BezierCurveParams(_)) {
                            axes[3] = inlay_host.map_or(
                                if object.surface_inlay.is_some() {
                                    u32::MAX
                                } else {
                                    0
                                },
                                |(host, _)| host as u32 + 1,
                            );
                        }
                        axes
                    },
                }
            })
            .collect();
        expand_soft_bounds(&gpu_objects, &mut bounds);
        let mut component_bounds = boolean_component_bounds(&gpu_objects, &bounds);
        for (index, object) in objects.iter().enumerate() {
            if object.boolean_parent.is_some() || !object.repetition.enabled {
                continue;
            }
            if !scene_objects
                .iter()
                .any(|child| child.boolean_parent == Some(object.uuid))
            {
                continue;
            }
            let Some(bound) = component_bounds[index].as_mut() else {
                continue;
            };
            let group = crate::model::group_world_matrix(scene_objects, object.uuid);
            let local_offset = Vec3::from_array(std::array::from_fn(|axis| {
                if object.repetition.count[axis] > 1 {
                    object.repetition.count[axis].saturating_sub(1) as f32
                        * object.repetition.spacing[axis].max(0.001)
                        * 0.5
                } else {
                    0.0
                }
            }));
            bound.half_extent += group.x_axis.truncate().abs() * local_offset.x
                + group.y_axis.truncate().abs() * local_offset.y
                + group.z_axis.truncate().abs() * local_offset.z;
            bound.radius = bound.half_extent.length();
        }
        for (index, object) in objects.iter().enumerate() {
            if object.boolean_parent.is_some() {
                continue;
            }
            let Some(mirror) = object.mirror else {
                continue;
            };
            let Some(bound) = component_bounds[index].as_mut() else {
                continue;
            };
            *bound = mirrored_bound(
                *bound,
                crate::model::group_world_matrix(scene_objects, object.uuid),
                mirror,
            );
        }
        mark_depth_accelerator_subtrees(&mut gpu_objects);
        if !training_only {
            flatten_nested_hard_unions(&mut gpu_objects);
        }
        let mut group_bounds = Vec::new();
        let mut group_start = 0;
        let mut capacity = 1;
        let mut starts = vec![0u32; objects.len()];
        for index in 0..gpu_objects.len() {
            if gpu_objects[index].meta[3] < 0 {
                if let Some(bound) = component_bounds[index] {
                    group_bounds.push(bound);
                }
                let march_factor = gpu_objects[group_start..=index]
                    .iter()
                    .map(|object| f32::from_bits(object.modifier[1]))
                    .fold(1.0_f32, f32::min);
                let component_capacity =
                    mark_component_evaluation(&mut gpu_objects, group_start, index);
                gpu_objects[index].modifier[1] = march_factor.to_bits();
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
        let was_boolean = self.has_booleans;
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
            && has_splat_samples;
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
            self.prepare_scene_pipelines(world, capacity, was_boolean, shader_features);
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
