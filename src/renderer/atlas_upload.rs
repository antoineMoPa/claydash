use super::group_capture::PreparedGroupScene;
use super::*;
use std::collections::HashMap;

type DepthMetadata = (u32, u32, u32, Vec3, Vec3, i32);
type SplatSample = (uuid::Uuid, Vec3, f32, [f32; 3], Vec3, u32);

/// Estimate local footprint from adjacent rays in the same face and depth layer.
/// Never enlarge beyond the grid footprint, keeping proxy/BVH bounds conservative.
struct SplatFootprint<'a> {
    texels: &'a [[f32; 4]],
    owners: &'a [Option<uuid::Uuid>],
    normals: &'a [Vec3],
    width: u32,
    height: u32,
    axis: usize,
    u_axis: usize,
    v_axis: usize,
    sign: f32,
    spacing: glam::Vec2,
}
impl SplatFootprint<'_> {
    fn sigma(&self, index: usize) -> f32 {
        let face_size = (self.width * self.height) as usize;
        let pixel = index % face_size;
        let x = (pixel % self.width as usize) as i32;
        let y = (pixel / self.width as usize) as i32;
        let base = 0.6 * self.spacing.max_element();
        let mut scale = 1.0_f32;
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = x + dx;
            let ny = y + dy;
            // A capture face border is not itself a surface edge.
            if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
                continue;
            }
            let neighbor = index - pixel + (ny as u32 * self.width + nx as u32) as usize;
            if self.owners[neighbor].is_none() || self.owners[neighbor] != self.owners[index] {
                scale = scale.min(0.5);
                continue;
            }
            let mut delta = Vec3::ZERO;
            delta[self.axis] = -self.sign * (self.texels[neighbor][0] - self.texels[index][0]);
            delta[self.u_axis] = dx as f32 * self.spacing.x;
            delta[self.v_axis] = dy as f32 * self.spacing.y;
            let normal = self.normals[index];
            let agreement = normal.dot(self.normals[neighbor]).clamp(0.0, 1.0);
            let plane_error = delta.dot(normal).abs() / self.spacing.max_element();
            // Curvature and depth discontinuities tighten the Gaussian, while
            // sloped planes retain their footprint (zero tangent-plane error).
            scale = scale.min((1.0 / (1.0 + 2.0 * (1.0 - agreement) + plane_error)).max(0.35));
        }
        base * scale
    }
}

pub(super) struct UploadedAtlases {
    pub box_depth_metadata: HashMap<uuid::Uuid, DepthMetadata>,
    pub depth_accelerator_transforms: HashMap<uuid::Uuid, u32>,
    pub splat_bvh_metadata: HashMap<uuid::Uuid, (u32, u32)>,
    pub splat_samples: Vec<SplatSample>,
    pub materials: material_gpu::PackedMaterials,
    pub stencil_layers: HashMap<uuid::Uuid, u32>,
}

impl Renderer {
    pub(super) fn upload_scene_atlases(
        &mut self,
        objects: &[&SdfObject],
        source_objects: &[SdfObject],
        prepared_scene: Option<&PreparedGroupScene>,
    ) -> UploadedAtlases {
        let mut box_depth_texels = Vec::new();
        let mut box_depth_metadata = std::collections::HashMap::new();
        let mut depth_accelerator_transforms = HashMap::new();
        let mut splat_bounds = std::collections::HashMap::new();
        let mut splat_bvh_metadata = std::collections::HashMap::new();
        let mut splat_samples = Vec::new();
        let mut materials = material_gpu::PackedMaterials::default();
        let source_indices: HashMap<_, _> = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.uuid, index as u32))
            .collect();
        let source_lookup: std::collections::HashMap<_, _> = source_objects
            .iter()
            .map(|object| (object.uuid, object))
            .collect();
        if let Some(prepared) = &prepared_scene {
            for object in objects {
                if let Some(field) = prepared.neural_fields.get(&object.uuid) {
                    if field.raymarch_last_segment {
                        if let Some(mut records) =
                            depth_accelerator_capture_records(source_objects, object.uuid)
                        {
                            let transform = box_depth_texels.len() as u32;
                            records[3][1] = 12.0;
                            box_depth_texels.extend_from_slice(&records);
                            depth_accelerator_transforms.insert(object.uuid, transform + 1);
                        }
                    }
                    let offset = box_depth_texels.len() as u32;
                    box_depth_texels.extend_from_slice(
                        &field
                            .network
                            .gpu_records(object.neural_sdf.hit_distance_cells),
                    );
                    let fallback = source_lookup.get(&object.uuid).copied().unwrap_or(object);
                    for owner in &field.owners {
                        let source = source_lookup.get(owner).copied().unwrap_or(fallback);
                        let custom_index = if source.material.kind == MaterialKind::Custom {
                            source
                                .material_id
                                .and_then(|id| {
                                    self.custom_material_sources
                                        .iter()
                                        .position(|(asset_id, _)| *asset_id == id)
                                })
                                .map_or(0, |i| i as u32 + 1)
                        } else {
                            0
                        };
                        let material = materials.insert_custom(source.material, custom_index);
                        box_depth_texels.push([
                            material as f32,
                            source.color.x,
                            source.color.y,
                            source.color.z,
                        ]);
                        if field.raymarch_last_segment {
                            box_depth_texels.push([
                                source_indices
                                    .get(owner)
                                    .copied()
                                    .map_or(0, |index| index + 1)
                                    as f32,
                                0.0,
                                0.0,
                                0.0,
                            ]);
                        }
                    }
                    let extent = Vec3::splat(field.half_extent);
                    box_depth_metadata.insert(object.uuid, (offset, 32, 32, -extent, extent, 12));
                    continue;
                }
                let capture = if let Some(atlas) = prepared.box_depth_atlases.get(&object.uuid) {
                    Some((
                        &atlas.texels,
                        &atlas.owners,
                        atlas.normals.as_slice(),
                        atlas.resolution,
                        atlas.resolution,
                        atlas.local_min,
                        atlas.local_max,
                        if prepared.gaussian_splats.contains(&object.uuid) {
                            10
                        } else {
                            8
                        },
                    ))
                } else {
                    prepared
                        .sphere_depth_atlases
                        .get(&object.uuid)
                        .map(|atlas| {
                            (
                                &atlas.texels,
                                &atlas.owners,
                                &[] as &[Vec3],
                                atlas.width,
                                atlas.height,
                                Vec3::splat(-atlas.radius),
                                Vec3::splat(atlas.radius),
                                9,
                            )
                        })
                };
                if let Some((texels, owners, normals, width, height, minimum, maximum, kind)) =
                    capture
                {
                    if object.render_representation.is_depth_accelerator() {
                        if let Some(records) =
                            depth_accelerator_capture_records(source_objects, object.uuid)
                        {
                            let transform = box_depth_texels.len() as u32;
                            let mut records = records;
                            records[3][1] = kind as f32;
                            box_depth_texels.extend_from_slice(&records);
                            depth_accelerator_transforms.insert(object.uuid, transform + 1);
                        }
                    }
                    let offset = box_depth_texels.len() as u32;
                    for (sample_index, (texel, owner)) in texels.iter().zip(owners).enumerate() {
                        let material_index = owner
                            .and_then(|id| source_lookup.get(&id).copied())
                            .map_or(0, |source| {
                                let custom_index = if source.material.kind == MaterialKind::Custom {
                                    source
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
                                materials.insert_custom(source.material, custom_index)
                            });
                        if kind == 10 {
                            let face_size = (width * height) as usize;
                            let face = (sample_index / face_size) % 6;
                            let pixel = sample_index % face_size;
                            let x = pixel as u32 % width;
                            let y = pixel as u32 / width;
                            let (axis, u_axis, v_axis, sign) = match face {
                                0 => (0, 1, 2, 1.0),
                                1 => (0, 1, 2, -1.0),
                                2 => (1, 0, 2, 1.0),
                                3 => (1, 0, 2, -1.0),
                                4 => (2, 0, 1, 1.0),
                                _ => (2, 0, 1, -1.0),
                            };
                            let mut center = Vec3::ZERO;
                            center[axis] = if sign > 0.0 {
                                maximum[axis]
                            } else {
                                minimum[axis]
                            } - sign * texel[0];
                            center[u_axis] = minimum[u_axis]
                                + (x as f32 + 0.5) / width as f32
                                    * (maximum[u_axis] - minimum[u_axis]);
                            center[v_axis] = minimum[v_axis]
                                + (y as f32 + 0.5) / height as f32
                                    * (maximum[v_axis] - minimum[v_axis]);
                            let support = SplatFootprint {
                                texels,
                                owners,
                                normals,
                                width,
                                height,
                                axis,
                                u_axis,
                                v_axis,
                                sign,
                                spacing: glam::Vec2::new(
                                    (maximum[u_axis] - minimum[u_axis]) / width as f32,
                                    (maximum[v_axis] - minimum[v_axis]) / height as f32,
                                ),
                            }
                            .sigma(sample_index);
                            if owner.is_some() {
                                splat_bounds
                                    .entry(object.uuid)
                                    .or_insert_with(Vec::new)
                                    .push(splat_bvh::SplatBound {
                                        center,
                                        radius: 2.5 * support,
                                        texel_offset: box_depth_texels.len() as u32,
                                    });
                                splat_samples.push((
                                    object.uuid,
                                    center,
                                    support,
                                    [texel[1], texel[2], texel[3]],
                                    normals[sample_index],
                                    material_index,
                                ));
                            }
                            box_depth_texels.push(if owner.is_some() {
                                center.extend(support).to_array()
                            } else {
                                [0.0, 0.0, 0.0, -1.0]
                            });
                            box_depth_texels.push([
                                material_index as f32,
                                texel[1],
                                texel[2],
                                texel[3],
                            ]);
                            box_depth_texels.push(normals[sample_index].extend(0.0).to_array());
                        } else {
                            box_depth_texels.push(*texel);
                            // Preserve depth-map ownership separately from its
                            // material ID. Zero means an empty/unavailable owner.
                            let source_owner =
                                if object.render_representation.is_depth_accelerator() {
                                    owner
                                        .and_then(|id| source_indices.get(&id).copied())
                                        .map_or(0, |index| index + 1)
                                } else {
                                    0
                                };
                            box_depth_texels.push([
                                material_index as f32,
                                source_owner as f32,
                                0.0,
                                0.0,
                            ]);
                        }
                    }
                    box_depth_metadata
                        .insert(object.uuid, (offset, width, height, minimum, maximum, kind));
                }
            }
        }
        for (id, mut bounds) in splat_bounds {
            if let Some(range) = splat_bvh::append_splat_bvh_with_budget(
                &mut box_depth_texels,
                &mut bounds,
                self.capture_record_budget(),
            ) {
                splat_bvh_metadata.insert(id, range);
            }
        }
        self.resize_capture_buffer_for(box_depth_texels.len());
        if !box_depth_texels.is_empty() {
            self.queue.write_buffer(
                &self.box_depth_buffer,
                0,
                bytemuck::cast_slice(&box_depth_texels),
            );
        }
        let mut stencil_layers = std::collections::HashMap::new();
        for object in objects
            .iter()
            .filter(|object| object.image_stencil.is_some())
            .take(32)
        {
            let stencil = object.image_stencil.as_ref().unwrap();
            if stencil.image.len() > 16 * 1024 * 1024 {
                continue;
            }
            let Ok(image) = image::load_from_memory(&stencil.image) else {
                continue;
            };
            let layer = stencil_layers.len() as u32;
            let rgba = image::imageops::resize(
                &image.to_rgba8(),
                512,
                512,
                image::imageops::FilterType::Triangle,
            );
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.image_atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                rgba.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512 * 4),
                    rows_per_image: Some(512),
                },
                wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
            );
            stencil_layers.insert(object.uuid, layer);
        }
        UploadedAtlases {
            box_depth_metadata,
            splat_bvh_metadata,
            depth_accelerator_transforms,
            splat_samples,
            materials,
            stencil_layers,
        }
    }
}

/// Map the capture's centered local frame to packed world-to-capture rows.
pub(super) fn depth_accelerator_capture_records(
    source: &[SdfObject],
    root: uuid::Uuid,
) -> Option<[[f32; 4]; 4]> {
    let (minimum, maximum) = crate::model::lattice_bounds(source, root)?;
    let world = crate::model::lattice_world_matrix(source, root);
    let capture = world * glam::Mat4::from_translation((minimum + maximum) * 0.5);
    let inverse = capture.inverse();
    let distance_scale = Vec3::new(
        world.x_axis.truncate().length(),
        world.y_axis.truncate().length(),
        world.z_axis.truncate().length(),
    )
    .min_element();
    if !inverse.is_finite() || !distance_scale.is_finite() || distance_scale <= 0.0 {
        return None;
    }
    let rows = inverse_affine_rows(inverse);
    Some([rows[0], rows[1], rows[2], [distance_scale, 0.0, 0.0, 0.0]])
}

#[cfg(test)]
mod sphere_accelerator_tests {
    use super::*;

    #[test]
    fn sphere_capture_rows_use_the_group_center_and_world_distance_scale() {
        let mut root = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        root.group_transform.translation = Vec3::new(3.0, -2.0, 1.0);
        root.group_transform.rotation = glam::Quat::from_rotation_y(0.7);
        root.group_transform.scale = Vec3::splat(1.7);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(root.uuid);
        child.transform.translation = Vec3::new(2.0, 0.0, 0.0);
        let id = root.uuid;
        let scene = [root, child];
        let (minimum, maximum) = crate::model::lattice_bounds(&scene, id).unwrap();
        let world = crate::model::lattice_world_matrix(&scene, id);
        let center = (minimum + maximum) * 0.5;
        let rows = depth_accelerator_capture_records(&scene, id).unwrap();
        for local in [Vec3::ZERO, Vec3::new(0.2, -0.1, 0.3)] {
            let point = world.transform_point3(center + local).extend(1.0);
            let captured = Vec3::new(
                glam::Vec4::from_array(rows[0]).dot(point),
                glam::Vec4::from_array(rows[1]).dot(point),
                glam::Vec4::from_array(rows[2]).dot(point),
            );
            assert!((captured - local).length() < 0.00001);
        }
        assert!((rows[3][0] - 1.7).abs() < 0.00001);
    }
}

#[cfg(test)]
mod splat_footprint_tests {
    use super::*;

    #[test]
    fn footprints_tighten_at_curvature_gaps_and_depth_edges() {
        let id = uuid::Uuid::new_v4();
        let mut texels = vec![[1.0, 0.0, 0.0, 0.0]; 18];
        let mut owners = vec![Some(id); 18];
        let mut normals = vec![Vec3::X; 18];
        let sigma = |texels: &[[f32; 4]], owners: &[Option<uuid::Uuid>], normals: &[Vec3]| {
            SplatFootprint {
                texels,
                owners,
                normals,
                width: 3,
                height: 3,
                axis: 0,
                u_axis: 1,
                v_axis: 2,
                sign: 1.0,
                spacing: glam::Vec2::ONE,
            }
            .sigma(4)
        };
        let flat = sigma(&texels, &owners, &normals);
        assert!((flat - 0.6).abs() < 0.00001);
        // A different depth layer must not contaminate this footprint.
        texels[13][0] = 100.0;
        normals[13] = Vec3::Y;
        assert_eq!(sigma(&texels, &owners, &normals), flat);
        normals[5] = Vec3::Y;
        assert!(sigma(&texels, &owners, &normals) < flat);
        normals[5] = Vec3::X;
        texels[5][0] = 5.0;
        assert!(sigma(&texels, &owners, &normals) < flat);
        texels[5][0] = 1.0;
        owners[5] = None;
        assert!(sigma(&texels, &owners, &normals) < flat);
        owners[5] = Some(uuid::Uuid::new_v4());
        assert!(sigma(&texels, &owners, &normals) < flat);
    }

    #[test]
    fn tilted_planes_keep_their_footprint() {
        let texels: Vec<_> = (0..9)
            .map(|i| [1.0 + (i % 3) as f32, 0.0, 0.0, 0.0])
            .collect();
        let owners = vec![Some(uuid::Uuid::new_v4()); 9];
        let normals = vec![
            Vec3::new(
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0
            );
            9
        ];
        let footprint = SplatFootprint {
            texels: &texels,
            owners: &owners,
            normals: &normals,
            width: 3,
            height: 3,
            axis: 0,
            u_axis: 1,
            v_axis: 2,
            sign: 1.0,
            spacing: glam::Vec2::ONE,
        };
        assert!((footprint.sigma(4) - 0.6).abs() < 0.00001);
    }
}
