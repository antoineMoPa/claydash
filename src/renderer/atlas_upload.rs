use super::group_capture::PreparedGroupScene;
use super::*;
use std::collections::HashMap;

type DepthMetadata = (u32, u32, u32, Vec3, Vec3, i32);
type SplatSample = (uuid::Uuid, Vec3, f32, [f32; 3], Vec3, u32);

pub(super) struct UploadedAtlases {
    pub box_depth_metadata: HashMap<uuid::Uuid, DepthMetadata>,
    pub splat_bvh_metadata: HashMap<uuid::Uuid, (u32, u32)>,
    pub splat_samples: Vec<SplatSample>,
    pub materials: material_gpu::PackedMaterials,
    pub stencil_layers: HashMap<uuid::Uuid, u32>,
}

impl Renderer {
    pub(super) fn upload_scene_atlases(
        &self,
        objects: &[&SdfObject],
        source_objects: &[SdfObject],
        prepared_scene: Option<&PreparedGroupScene>,
    ) -> UploadedAtlases {
        let mut box_depth_texels = Vec::new();
        let mut box_depth_metadata = std::collections::HashMap::new();
        let mut splat_bounds = std::collections::HashMap::new();
        let mut splat_bvh_metadata = std::collections::HashMap::new();
        let mut splat_samples = Vec::new();
        let mut materials = material_gpu::PackedMaterials::default();
        let source_lookup: std::collections::HashMap<_, _> = source_objects
            .iter()
            .map(|object| (object.uuid, object))
            .collect();
        if let Some(prepared) = &prepared_scene {
            for object in objects {
                if let Some(field) = prepared.neural_fields.get(&object.uuid) {
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
                            let support = 0.6
                                * ((maximum[u_axis] - minimum[u_axis]) / width as f32)
                                    .max((maximum[v_axis] - minimum[v_axis]) / height as f32);
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
                            box_depth_texels.push([material_index as f32, 0.0, 0.0, 0.0]);
                        }
                    }
                    box_depth_metadata
                        .insert(object.uuid, (offset, width, height, minimum, maximum, kind));
                }
            }
        }
        for (id, mut bounds) in splat_bounds {
            if let Some(range) = splat_bvh::append_splat_bvh(&mut box_depth_texels, &mut bounds) {
                splat_bvh_metadata.insert(id, range);
            }
        }
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
            splat_samples,
            materials,
            stencil_layers,
        }
    }
}
