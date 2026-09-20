use super::*;

impl Renderer {
    pub(super) fn upload_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
    ) {
        let scene_changed = self.uploaded_scene_versions != scene_versions;
        let mut gpu_camera = GpuCamera {
            inverse_view_projection: (camera.projection() * camera.view())
                .inverse()
                .to_cols_array_2d(),
            position: camera.position.extend(0.0).to_array(),
            count: [
                objects.len().min(MAX_OBJECTS) as u32,
                self.node_count,
                u32::from(
                    objects
                        .iter()
                        .any(|object| object.operation.gpu_code() != 0),
                ),
                u32::from(camera.projection_mode == ProjectionMode::Orthographic),
            ],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        if !scene_changed {
            return;
        }

        // Children precede parents, so the fragment shader can evaluate the
        // boolean tree in one forward pass while preserving sibling order.
        let selected_ids: std::collections::HashSet<_> = selected.iter().copied().collect();
        let scene_objects = &objects[..objects.len().min(MAX_OBJECTS)];
        let ordered = boolean_postorder(scene_objects);
        let objects = &ordered;
        let object_indices: std::collections::HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as i32))
            .collect();

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut gpu_objects: Vec<GpuObject> = objects
            .iter()
            .take(MAX_OBJECTS)
            .enumerate()
            .map(|(index, object)| {
                let matrix = crate::model::object_world_matrix(scene_objects, object.uuid);
                let abs_scale = Vec3::new(
                    matrix.x_axis.truncate().length(),
                    matrix.y_axis.truncate().length(),
                    matrix.z_axis.truncate().length(),
                );
                let distance_scale = abs_scale.min_element().max(0.000_001);
                let (params, radius) = match object.params {
                    SdfParams::SphereParams(ref params) => (
                        [params.radius, 0.0, 0.0, distance_scale],
                        params.radius * abs_scale.max_element(),
                    ),
                    SdfParams::BoxParams(ref params) => (
                        [
                            params.box_q.x,
                            params.box_q.y,
                            params.box_q.z,
                            distance_scale,
                        ],
                        (params.box_q * abs_scale).length(),
                    ),
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => (
                        [radius, half_height, 0.0, distance_scale],
                        Vec2::new(radius, half_height).length() * abs_scale.max_element(),
                    ),
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => (
                        [major_radius, minor_radius, 0.0, distance_scale],
                        (major_radius + minor_radius) * abs_scale.max_element(),
                    ),
                };
                let repeated_radius = if object.repetition.enabled {
                    let extent = Vec3::from_array([
                        if object.repetition.axes[0] {
                            (object.repetition.count[0].saturating_sub(1)) as f32
                                * object.repetition.spacing.x
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.axes[1] {
                            (object.repetition.count[1].saturating_sub(1)) as f32
                                * object.repetition.spacing.y
                                * 0.5
                        } else {
                            0.0
                        },
                        if object.repetition.axes[2] {
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
                };
                let mut repeated_extent = local_extent;
                if object.repetition.enabled {
                    for axis in 0..3 {
                        if object.repetition.axes[axis] {
                            repeated_extent[axis] += object.repetition.count[axis].saturating_sub(1)
                                as f32
                                * object.repetition.spacing[axis].max(0.001)
                                * 0.5;
                        }
                    }
                }
                let half_extent = matrix.x_axis.truncate().abs() * repeated_extent.x
                    + matrix.y_axis.truncate().abs() * repeated_extent.y
                    + matrix.z_axis.truncate().abs() * repeated_extent.z;
                bounds.push(ObjectBound {
                    half_extent,
                    center: matrix.transform_point3(Vec3::ZERO),
                    radius: repeated_radius,
                    object_index: index as u32,
                });
                GpuObject {
                    // Spare component lanes carry blend width and material kind.
                    component: [
                        0,
                        0,
                        object.softness.to_bits(),
                        object.material.kind.gpu_code(),
                    ],
                    meta: [
                        i32::from(selected_ids.contains(&object.uuid)),
                        object.object_type,
                        object.operation.gpu_code(),
                        object
                            .boolean_parent
                            .and_then(|parent| object_indices.get(&parent).copied())
                            .unwrap_or(-1),
                    ],
                    color: object.color.to_array(),
                    inverse_rows: inverse_affine_rows(matrix.inverse()),
                    params,
                    material: [
                        object.material.roughness,
                        object.material.metallic,
                        object.material.reflectivity,
                        object.material.opacity,
                    ],
                    repeat_spacing: object
                        .repetition
                        .spacing
                        .extend(object.material.refractive_index)
                        .to_array(),
                    repeat_count: if object.repetition.enabled {
                        [
                            if object.repetition.axes[0] {
                                object.repetition.count[0] as i32
                            } else {
                                1
                            },
                            if object.repetition.axes[1] {
                                object.repetition.count[1] as i32
                            } else {
                                1
                            },
                            if object.repetition.axes[2] {
                                object.repetition.count[2] as i32
                            } else {
                                1
                            },
                            1,
                        ]
                    } else {
                        [1, 1, 1, 0]
                    },
                }
            })
            .collect();
        expand_soft_bounds(&gpu_objects, &mut bounds);
        let component_bounds = boolean_component_bounds(&gpu_objects, &bounds);
        let mut group_bounds = Vec::new();
        let mut group_start = 0;
        let mut capacity = 1;
        let mut starts = vec![0u32; objects.len()];
        for (index, object) in gpu_objects.iter().enumerate() {
            if object.meta[3] < 0 {
                if let Some(bound) = component_bounds[index] {
                    group_bounds.push(bound);
                }
                starts[index] = group_start as u32;
                capacity = capacity.max((index - group_start + 1).next_power_of_two() as u32);
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
        self.node_count = bvh.len() as u32;
        gpu_camera.count[1] = self.node_count;
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        // Specialize scratch storage to the largest component, not scene size.
        self.has_booleans = capacity > 1;
        let transmitted_instances: u64 = gpu_objects
            .iter()
            .filter(|object| object.material[3] < 0.999 && object.material[1] < 0.999)
            .map(|object| {
                object.repeat_count[..3]
                    .iter()
                    .map(|&count| count.max(1) as u64)
                    .fold(1u64, u64::saturating_mul)
            })
            .fold(0u64, u64::saturating_add);
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
        if self.has_booleans
            && self
                .boolean_pipeline
                .as_ref()
                .is_none_or(|(size, _)| *size != capacity)
        {
            self.boolean_pipeline = Some((
                capacity,
                create_scene_pipeline(
                    &self.device,
                    &self.shader_source,
                    &self.pipeline_layout,
                    self.config.format,
                    self.use_bvh,
                    capacity,
                ),
            ));
        }
        if !gpu_objects.is_empty() {
            self.queue
                .write_buffer(&self.objects_buffer, 0, bytemuck::cast_slice(&gpu_objects));
            self.queue
                .write_buffer(&self.bvh_buffer, 0, bytemuck::cast_slice(&bvh));
        }
        self.uploaded_scene_versions = scene_versions;
    }
}
