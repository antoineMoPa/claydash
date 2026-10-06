use super::*;
use wgpu::util::DeviceExt;

pub(crate) struct TextureReadback { receiver: std::sync::mpsc::Receiver<Result<Vec<u8>, String>> }
impl TextureReadback {
    pub fn poll(&self) -> Option<Result<Vec<u8>, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Texture readback stopped".into())),
        }
    }
}

fn bake_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bind: &wgpu::BindGroup,
    pipeline: &wgpu::RenderPipeline,
    vertices: &[[[f32; 4]; 7]],
    size: u32,
) -> TextureReadback {
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bake triangles"),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("GLB baked texture"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GLB texture readback"),
        size: u64::from(size) * u64::from(size) * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Bake material colours"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..vertices.len() as u32);
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (send, receiver) = std::sync::mpsc::channel();
    let mapped = readback.clone();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let result = result.map_err(|e| e.to_string()).and_then(|_| {
                let data = mapped
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                let pixels = data.to_vec();
                drop(data);
                Ok(pixels)
            });
            mapped.unmap();
            std::thread::spawn(move || {
                let png = result.and_then(|pixels| {
                    let mut out = Vec::new();
                    {
                        let mut encoder = png::Encoder::new(&mut out, size, size);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
                        writer.write_image_data(&pixels).map_err(|e| e.to_string())?;
                    }
                    Ok(out)
                });
                let _ = send.send(png);
            });
        });

    TextureReadback { receiver }
}


impl Renderer {
    pub(crate) fn bake_poisson_page(&mut self, source: &[SdfObject], assets: &[MaterialAsset],
        camera: &Camera, world: World,
        page: &poisson_mesh::textured_glb::Page) -> Result<TextureReadback, String> {
        if source.len() > MAX_OBJECTS { return Err("Scene exceeds the renderer object limit".into()); }
        self.sync_custom_materials(assets)?;
        let mut exact = source.to_vec();
        for object in &mut exact {
            object.render_representation = crate::model::GroupRenderRepresentation::ExactSdf;
        }
        self.invalidate_scene();
        self.upload_scene_with_world_exposure(camera, &exact, &[], [i32::MIN + 1; 2],
            1.0, world, super::scene_upload::ScenePipelinePreparation::MaterialPreview);
        let order = boolean_postorder(&exact);
        let owners: std::collections::HashMap<_, _> = order.iter().enumerate()
            .map(|(index, object)| (object.uuid, index as u32)).collect();
        let columns = page.size / poisson_mesh::textured_glb::TILE;
        let mut vertices = Vec::<[[f32; 4]; 7]>::with_capacity(page.count);
        for i in 0..page.count {
            let triangle = page.mesh.triangle(page.first + i);
            let owner = *owners.get(&page.mesh.owners[page.first + i])
                .ok_or("Mesh material owner is missing")?;
            let normals = page.mesh.normals[page.first + i];
            vertices.push([
                triangle[0].extend(owner as f32).to_array(),
                triangle[1].extend(0.0).to_array(),
                triangle[2].extend(0.0).to_array(),
                [((i as u32 % columns) * 16) as f32,
                    ((i as u32 / columns) * 16) as f32, page.size as f32, 0.0],
                normals[0].extend(0.0).to_array(),
                normals[1].extend(0.0).to_array(),
                normals[2].extend(0.0).to_array(),
            ]);
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.shader_source.hash(&mut hasher);
        let capacity = mesh_component_capacity(&exact);
        capacity.hash(&mut hasher);
        let shader_key = hasher.finish();
        if self.poisson_bake_pipeline.as_ref().is_none_or(|(key, _)| *key != shader_key) {
            self.poisson_bake_pipeline = Some((shader_key, texture_pipeline(&self.device,
                Some(&self.pipeline_layout), &self.shader_source, capacity)));
        }
        let pipeline = &self.poisson_bake_pipeline.as_ref().unwrap().1;
        let readback = bake_texture(&self.device, &self.queue, &self.bind_group,
            pipeline, &vertices, page.size);
        self.invalidate_scene();
        Ok(readback)
    }

    pub(crate) fn poll_poisson_bake(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }
}

fn mesh_component_capacity(source: &[SdfObject]) -> u32 {
    let ordered = boolean_postorder(source);
    let mut start = 0;
    let mut capacity = 1usize;
    for (index, object) in ordered.iter().enumerate() {
        if object.boolean_parent.is_some() { continue; }
        let group = &ordered[start..=index];
        let direct = group[..group.len() - 1].iter()
            .all(|child| child.boolean_parent == Some(object.uuid));
        capacity = capacity.max(if direct { group.len().min(2) }
            else { group.len().next_power_of_two() });
        start = index + 1;
    }
    capacity as u32
}

fn texture_pipeline(device: &wgpu::Device, layout: Option<&wgpu::PipelineLayout>,
    scene: &str, _capacity: u32) -> wgpu::RenderPipeline {
    let source = super::poisson_material::source(scene, _capacity);
    let source = format!("{source}\n{}", include_str!("../../assets/shaders/poisson_bake.wgsl"));
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Poisson source material texture bake"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Poisson source material texture bake"), layout,
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_poisson_bake"),
            buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 112,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4,
                    2 => Float32x4, 3 => Float32x4, 4 => Float32x4,
                    5 => Float32x4, 6 => Float32x4] })], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_poisson_bake"),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: None, write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
        multiview_mask: None, cache: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn source_material_texture_pipeline_compiles_on_gpu() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default().request_adapter(&Default::default())
                .await.expect("GPU adapter");
            let (device, _) = adapter.request_device(&Default::default()).await.expect("GPU device");
            let source = super::super::material_gpu::shader_source();
            let _pipeline = super::texture_pipeline(&device, None, &source, 1);
        });
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_poisson_texture_bakes_material_without_directional_lighting() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&Default::default())
            .await
            .expect("GPU adapter");
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let mut asset = MaterialAsset::custom("Red shader".into());
        asset.wgsl=Some("var s = base; s.color = vec3(1.0, 0.0, 0.0); s.reflectivity = 0.0; s.opacity = 1.0; return s;".into());
        let source = super::material_gpu::shader_source_for_assets(&[asset.clone()]);
        let pipeline = texture_pipeline(&device, None, &source, 1);
        let full = format!("{}\n{}", super::poisson_material::source(&source, 8), include_str!("../../assets/shaders/poisson_bake.wgsl"));
        let module = wgpu::naga::front::wgsl::parse_str(&full).unwrap();
        let info = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        let entry = module
            .entry_points
            .iter()
            .position(|e| e.name == "fs_poisson_bake")
            .unwrap();
        let used = info.get_entry_point(entry);
        let bindings: Vec<_> = module
            .global_variables
            .iter()
            .filter_map(|(h, v)| {
                v.binding
                    .as_ref()
                    .filter(|b| b.group == 0 && !used[h].is_empty())
                    .map(|b| b.binding)
            })
            .collect();
        let mut camera = GpuCamera::zeroed();
        camera.position = [0.0, 0.0, 5.0, 1.0];
        camera.count = [2, 0, 64, 0];
        camera.lighting_params = [1.0, 0.0, 0.0, 0.0];
        let mut child = GpuObject::zeroed();
        child.meta = [0, sdf_consts::TYPE_BOX, 0, 1];
        child.color = [0.0, 1.0, 0.0, 1.0];
        child.inverse_rows = inverse_affine_rows(
            glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 0.05)).inverse());
        child.group_inverse_rows = child.inverse_rows;
        child.params = [0.17, 2.0, 1.0, 1.0];
        child.scale = [1.0, 1.0, 1.0, 0.0];
        child.repeat_count = [1, 1, 1, 0];
        child.component = [0, 1, 0, 0];
        child.modifier[1] = 0.8f32.to_bits();
        let mut root = child;
        root.meta = [0, sdf_consts::TYPE_BOX, 0, FLAT_UNION_ROOT];
        root.inverse_rows = inverse_affine_rows(glam::Mat4::IDENTITY);
        root.group_inverse_rows = root.inverse_rows;
        root.params = [2.0, 2.0, 1.0, 1.0];
        root.component[3] = 1;
        let mut materials = super::material_gpu::PackedMaterials::default();
        materials.insert_custom(asset.material, 1);
        materials.insert(Material::default());
        let objects = [child, root];
        let mut buffers = std::collections::HashMap::new();
        for &binding in &bindings {
            if matches!(binding, 9 | 10 | 11 | 12) {
                continue;
            }
            let bytes: &[u8] = match binding {
                0 => bytemuck::bytes_of(&camera),
                1 => bytemuck::cast_slice(&objects),
                3 => bytemuck::cast_slice(&materials.headers),
                4 => bytemuck::cast_slice(&materials.params),
                _ => &[0; 1024],
            };
            buffers.insert(
                binding,
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytes,
                    usage: if binding == 0 {
                        wgpu::BufferUsages::UNIFORM
                    } else {
                        wgpu::BufferUsages::STORAGE
                    },
                }),
            );
        }
        let tex = |dimension| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let lattice = tex(wgpu::TextureDimension::D3);
        let images = tex(wgpu::TextureDimension::D2);
        let lattice_view = lattice.create_view(&Default::default());
        let image_view = images.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&Default::default());
        let entries: Vec<_> = bindings
            .into_iter()
            .map(|binding| wgpu::BindGroupEntry {
                binding,
                resource: match binding {
                    9 => wgpu::BindingResource::TextureView(&lattice_view),
                    11 => wgpu::BindingResource::TextureView(&image_view),
                    10 | 12 => wgpu::BindingResource::Sampler(&sampler),
                    _ => buffers[&binding].as_entire_binding(),
                },
            })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let vertices = [[
            [0.0, 0.0, 1.0, 1.0],
            [0.3, 0.0, 1.0, 0.0],
            [0.0, 0.3, 1.0, 0.0],
            [0.0, 0.0, 64.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]];
        let readback = bake_texture(&device, &queue, &bind, &pipeline, &vertices, 64);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let png = loop {
            if let Some(result) = readback.poll() { break result.unwrap(); }
            assert!(std::time::Instant::now() < deadline, "PNG encoding timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        let pixel = image.get_pixel(4, 4).0;
        assert!(
            pixel[0] == 255,
            "base colour must not contain lighting: {pixel:?}"
        );
        assert!(
            pixel[1] < 16 && pixel[2] < 16 && pixel[3] == 255,
            "custom shader replaces green base: {pixel:?}"
        );
        let toward_second_corner = image.get_pixel(8, 3).0;
        assert!(pixel == toward_second_corner,
            "surface normals must not introduce baked directional shading: {pixel:?} vs {toward_second_corner:?}");
        let outside_band = image.get_pixel(12, 3).0;
        assert!(outside_band[1] > outside_band[0] * 2 && outside_band[1] > 30,
            "original source owner should switch across one triangle tile: {pixel:?} vs {outside_band:?}");
    });
}
