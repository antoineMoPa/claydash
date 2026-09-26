use super::*;

impl Renderer {
    pub(super) fn upload_scene(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        selected: &[uuid::Uuid],
        scene_versions: [i32; 2],
    ) {
        self.upload_scene_with_world(camera, objects, selected, scene_versions, World::default());
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
    ) {
        let scene_changed = self.uploaded_scene_versions != scene_versions;
        let mut gpu_camera = GpuCamera {
            inverse_view_projection: (camera.projection() * camera.view())
                .inverse()
                .to_cols_array_2d(),
            position: camera.position.extend(exposure).to_array(),
            count: [
                objects.len().min(MAX_OBJECTS) as u32,
                self.node_count,
                camera.viewport.y.round().max(1.0) as u32,
                u32::from(camera.projection_mode == ProjectionMode::Orthographic),
            ],
            world_mode: [world.background.shader_id(), 0, 0, 0],
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
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&gpu_camera));
        if !scene_changed {
            return;
        }

        // Children precede parents, so the fragment shader can evaluate the
        // boolean tree in one forward pass while preserving sibling order.
        let selected_ids: std::collections::HashSet<_> = selected.iter().copied().collect();
        let scene_objects = &objects[..objects.len().min(MAX_OBJECTS)];
        let scene_lookup: std::collections::HashMap<_, _> = scene_objects
            .iter()
            .map(|object| (object.uuid, object))
            .collect();
        let ordered = boolean_postorder(scene_objects);
        let objects = &ordered;
        let object_indices: std::collections::HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as i32))
            .collect();

        let mut bounds = Vec::with_capacity(objects.len().min(MAX_OBJECTS));
        let mut materials = material_gpu::PackedMaterials::default();
        let mut polygon_points = Vec::new();
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
                let path_profile = object
                    .path_extrusion
                    .and_then(|modifier| modifier.profile_curve)
                    .and_then(|id| crate::model::profile_curve_vertices(scene_objects, id));
                let path_profile_extent = path_profile.as_ref().map_or(1.0_f32, |vertices| {
                    vertices
                        .iter()
                        .map(|point| point.length())
                        .fold(1.0_f32, f32::max)
                });
                let path_radius = object
                    .path_extrusion
                    .map_or(0.0, |modifier| modifier.radius);
                let path_profile_count = if object
                    .path_extrusion
                    .and_then(|modifier| modifier.profile_curve)
                    .is_some()
                {
                    path_profile
                        .as_ref()
                        .map_or(2.0, |vertices| vertices.len().min(65) as f32)
                } else if object
                    .path_extrusion
                    .is_some_and(|modifier| modifier.profile == crate::model::BezierProfile::Square)
                {
                    1.0
                } else {
                    0.0
                };
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
                    SdfParams::PolygonPrismParams(ref polygon) => {
                        let offset = polygon_points.len() as u32;
                        polygon_points.extend(
                            polygon
                                .vertices
                                .iter()
                                .take(crate::model::MAX_POLYGON_PRISM_VERTICES)
                                .map(|point| GpuPolygonPoint {
                                    position: point.to_array(),
                                }),
                        );
                        let count = polygon_points.len() as u32 - offset;
                        let planar_radius = polygon
                            .vertices
                            .iter()
                            .map(|point| point.length())
                            .fold(0.0_f32, f32::max);
                        (
                            [
                                polygon.half_depth,
                                f32::from_bits(offset),
                                f32::from_bits(count),
                                distance_scale,
                            ],
                            Vec2::new(planar_radius, polygon.half_depth).length()
                                * abs_scale.max_element(),
                        )
                    }
                    SdfParams::LoftParams(ref loft) => {
                        let offset = polygon_points.len() as u32;
                        let profile_count = loft.profile_count();
                        for section in loft
                            .sections
                            .iter()
                            .take(crate::model::LoftParams::MAX_SECTIONS)
                        {
                            polygon_points.push(GpuPolygonPoint {
                                position: [section.x, section.center_y],
                            });
                            polygon_points.push(GpuPolygonPoint {
                                position: [section.center_z, section.half_height],
                            });
                            polygon_points.push(GpuPolygonPoint {
                                position: [section.half_width, 0.0],
                            });
                            for index in 0..profile_count {
                                polygon_points.push(GpuPolygonPoint {
                                    position: crate::model::LoftParams::profile_point(
                                        section,
                                        index,
                                        profile_count,
                                    )
                                    .to_array(),
                                });
                            }
                        }
                        let count = loft
                            .sections
                            .len()
                            .min(crate::model::LoftParams::MAX_SECTIONS)
                            as u32;
                        (
                            [
                                f32::from_bits(offset),
                                f32::from_bits(count),
                                f32::from_bits(profile_count as u32),
                                distance_scale,
                            ],
                            loft.local_extent().length() * abs_scale.max_element(),
                        )
                    }
                    SdfParams::BezierCurveParams(ref curve) => {
                        let offset = polygon_points.len() as u32;
                        for &point in curve.points.iter().take(25) {
                            polygon_points.push(GpuPolygonPoint {
                                position: [point.x, point.y],
                            });
                            polygon_points.push(GpuPolygonPoint {
                                position: [point.z, 0.0],
                            });
                        }
                        let profile_offset = polygon_points.len() as u32;
                        if let Some(vertices) = &path_profile {
                            polygon_points.extend(vertices.iter().take(65).map(|point| {
                                GpuPolygonPoint {
                                    position: point.to_array(),
                                }
                            }));
                        }
                        (
                            [
                                path_radius,
                                f32::from_bits(offset),
                                f32::from_bits(profile_offset),
                                distance_scale,
                            ],
                            curve
                                .local_extent(path_radius * path_profile_extent)
                                .length()
                                * abs_scale.max_element(),
                        )
                    }
                };
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
                bounds.push(ObjectBound {
                    half_extent,
                    center: matrix.transform_point3(Vec3::ZERO),
                    radius: repeated_radius + cage_extent.length(),
                    object_index: index as u32,
                });
                let material_index = materials.insert(object.material);
                GpuObject {
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
                        object.object_type,
                        object.operation.gpu_code(),
                        object
                            .boolean_parent
                            .and_then(|parent| object_indices.get(&parent).copied())
                            .unwrap_or(-1),
                    ],
                    color: object.color.to_array(),
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
                            march_factor
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
                gpu_objects[index].modifier[1] = march_factor.to_bits();
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
                    self.render_format,
                    self.use_bvh,
                    capacity,
                    false,
                ),
            ));
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
    }
}
