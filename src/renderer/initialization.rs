use super::*;

fn create_lattice_atlas(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lattice inverse atlas"),
        size: wgpu::Extent3d {
            width: LATTICE_ATLAS_WIDTH,
            height: LATTICE_ATLAS_TILE_PITCH * rows,
            depth_or_array_layers: modifier_gpu::INVERSE_GRID_RESOLUTION as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_scene_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: [&wgpu::Buffer; 8],
    atlas: &wgpu::Texture,
    sampler: &wgpu::Sampler,
    image_atlas: &wgpu::Texture,
    image_sampler: &wgpu::Sampler,
    box_depth_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view = atlas.create_view(&Default::default());
    let image_view = image_atlas.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let mut entries: Vec<_> = buffers
        .into_iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: if binding == 7 { 8 } else { binding as u32 },
            resource: buffer.as_entire_binding(),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: 9,
        resource: wgpu::BindingResource::TextureView(&view),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 10,
        resource: wgpu::BindingResource::Sampler(sampler),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 11,
        resource: wgpu::BindingResource::TextureView(&image_view),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 12,
        resource: wgpu::BindingResource::Sampler(image_sampler),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 13,
        resource: box_depth_buffer.as_entire_binding(),
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene bind group"),
        layout,
        entries: &entries,
    })
}

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_pixels(&self) -> u32 {
        self.viewport.preview_pixels()
    }

    pub async fn new(window: Arc<Window>, use_bvh: bool, uncapped: bool) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("find GPU adapter");
        if uncapped {
            eprintln!("GPU: {:?}", adapter.get_info());
        }
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_storage_buffer_binding_size: adapter
                        .limits()
                        .max_storage_buffer_binding_size,
                    max_buffer_size: adapter.limits().max_buffer_size,
                    ..Default::default()
                },
                required_features: if timestamps {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            })
            .await
            .expect("create GPU device");
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        // Browser canvases generally expose an unorm surface even though the
        // page is displayed in sRGB. Render through the compatible sRGB view
        // so WebGPU applies the same linear-to-sRGB conversion as native.
        let render_format = render_format_for_surface(format);
        let present_mode = if uncapped {
            capabilities
                .present_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::PresentMode::Immediate)
                .unwrap_or(wgpu::PresentMode::AutoNoVsync)
        } else {
            wgpu::PresentMode::Fifo
        };
        let surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        #[cfg(not(target_arch = "wasm32"))]
        let surface_usage = surface_usage | wgpu::TextureUsages::COPY_SRC;
        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: (render_format != format)
                .then_some(render_format)
                .into_iter()
                .collect(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<GpuCamera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let objects_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("objects"),
            size: (MAX_OBJECTS * std::mem::size_of::<GpuObject>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bvh_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene bvh"),
            size: (MAX_BVH_NODES * std::mem::size_of::<GpuBvhNode>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let material_headers_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material headers"),
            size: (MAX_OBJECTS * std::mem::size_of::<material_gpu::GpuMaterialHeader>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let material_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material parameters"),
            size: (MAX_OBJECTS * material_gpu::MAX_PARAM_SLOTS * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let polygon_points_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("polygon points"),
            size: (MAX_POLYGON_POINTS * std::mem::size_of::<GpuPolygonPoint>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let box_depth_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("box depth capture texels"),
            size: (MAX_BOX_DEPTH_TEXELS * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lattice_points_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lattice control offsets"),
            size: (MAX_LATTICE_POINTS * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lattice_atlas = create_lattice_atlas(&device, 1);
        let lattice_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let image_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image stencil atlas"),
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let modifier_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("modifier parameters"),
            size: (MAX_OBJECTS * modifier_gpu::MAX_PARAM_SLOTS * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene layout"),
            entries: &(0..7)
                .chain(std::iter::once(8))
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: if binding == 0 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage { read_only: true }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .chain([
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D3,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 10,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 11,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 12,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 13,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ])
                .collect::<Vec<_>>(),
        });
        let bind_group = create_scene_bind_group(
            &device,
            &layout,
            [
                &camera_buffer,
                &objects_buffer,
                &bvh_buffer,
                &material_headers_buffer,
                &material_params_buffer,
                &polygon_points_buffer,
                &lattice_points_buffer,
                &modifier_params_buffer,
            ],
            &lattice_atlas,
            &lattice_sampler,
            &image_atlas,
            &image_sampler,
            &box_depth_buffer,
        );
        let shader_source = super::material_gpu::shader_source();
        #[cfg(not(target_arch = "wasm32"))]
        let shader_source = if uncapped {
            std::env::args()
                .find_map(|arg| arg.strip_prefix("--benchmark-shader=").map(str::to_owned))
                .map(|path| std::fs::read_to_string(path).expect("read benchmark reference shader"))
                .unwrap_or(shader_source)
        } else {
            shader_source
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader_features = SceneShaderFeatures::for_scene(&[], &[]);
        let pipeline = create_scene_pipeline_for_materials(
            &device,
            &shader_source,
            &pipeline_layout,
            render_format,
            use_bvh,
            1,
            false,
            false,
            shader_features,
            false,
        );
        let fast_pipeline = create_scene_pipeline_for_materials(
            &device,
            &shader_source,
            &pipeline_layout,
            render_format,
            use_bvh,
            1,
            false,
            true,
            shader_features,
            false,
        );
        let (splat_pipeline, splat_camera_buffer, splat_camera_bind_group, splat_buffer) =
            hybrid_splats::create_splat_resources(&device, render_format);
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            render_format,
            egui_wgpu::RendererOptions::default(),
        );
        let viewport = crate::viewport::Viewport::new(&device, render_format, timestamps, &layout);
        let post_processing = post_processing::PostProcessor::new(&device, render_format);
        let renderer = Self {
            viewport,
            post_processing,
            initial_pixel_budget: 48 * 1024,
            surface,
            device,
            queue,
            config,
            render_format,
            pipeline,
            fast_pipeline,
            hybrid_pipeline: None,
            hybrid_fast_pipeline: None,
            hybrid_depth_pipeline: None,
            deferred_geometry_pipeline: None,
            deferred_supported: true,
            splat_pipeline,
            splat_camera_buffer,
            splat_camera_bind_group,
            splat_instances: Vec::new(),
            splat_buffer,
            splat_buffer_capacity: 1,
            hybrid_enabled: false,
            boolean_pipeline: None,
            fast_boolean_pipeline: None,
            shader_source,
            custom_material_sources: Vec::new(),
            pipeline_layout,
            use_bvh,
            node_count: 0,
            uploaded_object_count: 0,
            has_booleans: false,
            scene_shader_features: shader_features,
            scene_pipelines_dirty: false,
            bind_group,
            bind_group_layout: layout,
            camera_buffer,
            objects_buffer,
            bvh_buffer,
            material_headers_buffer,
            material_params_buffer,
            polygon_points_buffer,
            box_depth_buffer,
            lattice_points_buffer,
            lattice_atlas,
            lattice_atlas_rows: 1,
            lattice_sampler,
            image_atlas,
            image_sampler,
            modifier_params_buffer,
            uploaded_scene_versions: [i32::MIN; 2],
            neural_jobs: Default::default(),
            group_compute_requests: Default::default(),
            group_capture_cache: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            group_capture_bake: None,
            egui_renderer,
            material_previews: Vec::new(),
            material_preview_pipelines: Vec::new(),
            material_preview_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            capture_result: Arc::new(std::sync::Mutex::new(None)),
            capture_pending: false,
            outline_capture: None,
        };
        renderer
    }

    pub(super) fn resize_lattice_atlas_for(&mut self, tile_count: usize) {
        let rows = (tile_count as u32)
            .div_ceil(LATTICE_ATLAS_TILES_PER_ROW)
            .max(1)
            .next_power_of_two();
        if rows == self.lattice_atlas_rows {
            return;
        }
        self.lattice_atlas = create_lattice_atlas(&self.device, rows);
        self.lattice_atlas_rows = rows;
        self.rebuild_scene_bind_group();
    }

    fn rebuild_scene_bind_group(&mut self) {
        self.bind_group = create_scene_bind_group(
            &self.device,
            &self.bind_group_layout,
            [
                &self.camera_buffer,
                &self.objects_buffer,
                &self.bvh_buffer,
                &self.material_headers_buffer,
                &self.material_params_buffer,
                &self.polygon_points_buffer,
                &self.lattice_points_buffer,
                &self.modifier_params_buffer,
            ],
            &self.lattice_atlas,
            &self.lattice_sampler,
            &self.image_atlas,
            &self.image_sampler,
            &self.box_depth_buffer,
        );
    }

    pub(super) fn capture_record_budget(&self) -> usize {
        let limits = self.device.limits();
        // BVH offsets are f32; keep record addresses exactly representable.
        (u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size) / 16)
            .min(1 << 24) as usize
    }

    pub(super) fn resize_capture_buffer_for(&mut self, records: usize) {
        let bytes = (records * 16) as u64;
        if bytes <= self.box_depth_buffer.size() {
            return;
        }
        self.box_depth_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resized depth and splat capture buffer"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.rebuild_scene_bind_group();
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.config.width as f32, self.config.height as f32)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn invalidate_scene(&mut self) {
        self.uploaded_scene_versions = [i32::MIN; 2];
        self.viewport.invalidate();
    }
}
