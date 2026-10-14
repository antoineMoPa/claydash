use super::*;

const LEVELS: u32 = 4;

pub(super) struct Bloom {
    pub enabled: bool,
    pub emission: bool,
    pub uniform: wgpu::Buffer,
    pub empty: wgpu::TextureView,
    pipelines: Option<[wgpu::RenderPipeline; 3]>,
    layout: wgpu::BindGroupLayout,
    radius: wgpu::Buffer,
    sampler: wgpu::Sampler,
}

pub(super) struct BloomTargets {
    pub preview: wgpu::TextureView,
    pub refined: wgpu::TextureView,
    pub result: wgpu::TextureView,
    extract: wgpu::BindGroup,
    levels: Vec<wgpu::TextureView>,
    combined: Vec<wgpu::TextureView>,
    down_groups: Vec<wgpu::BindGroup>,
    up_groups: Vec<wgpu::BindGroup>,
}

fn texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("neon emission / bloom"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Bloom {
    pub fn new(device: &wgpu::Device) -> Self {
        let entries = [0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom blur"),
            entries: &[
                entries[0],
                entries[1],
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            enabled: false,
            emission: false,
            uniform: buffer("bloom composite settings"),
            radius: buffer("bloom blur radius"),
            empty: texture(device, [1, 1]),
            pipelines: None,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    fn ensure_pipelines(&mut self, device: &wgpu::Device, extract_layout: &wgpu::BindGroupLayout) {
        if self.pipelines.is_some() {
            return;
        }
        let layout = &self.layout;
        let pipeline = |source: &str, layout: &wgpu::BindGroupLayout, entry| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bloom"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bloom"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bloom"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        self.pipelines = Some([
            pipeline(&extract_source(), extract_layout, "fs_main"),
            pipeline(
                include_str!("../../assets/shaders/bloom.wgsl"),
                layout,
                "fs_down",
            ),
            pipeline(
                include_str!("../../assets/shaders/bloom.wgsl"),
                layout,
                "fs_up",
            ),
        ]);
    }

    pub fn settings(&self, queue: &wgpu::Queue, world: crate::model::World) {
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&[
                if self.enabled {
                    world.bloom_strength.clamp(0.0, 2.0)
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ]),
        );
        // Four scales give a wide halo with five texture reads per blur pixel.
        queue.write_buffer(
            &self.radius,
            0,
            bytemuck::cast_slice(&[(world.bloom_radius / 24.0).clamp(0.17, 4.0), 0.0, 0.0, 0.0]),
        );
    }

    pub fn targets(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        progress: &wgpu::Buffer,
        size: [u32; 2],
        preview_size: [u32; 2],
    ) -> BloomTargets {
        self.ensure_pipelines(device, layout);
        let preview = texture(device, preview_size);
        let refined = texture(device, size);
        let extract = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("progressive emission resolve"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&preview),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&refined),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: progress.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.empty),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let levels: Vec<_> = (1..=LEVELS)
            .map(|level| {
                texture(
                    device,
                    [
                        size[0].div_ceil(1 << level).max(1),
                        size[1].div_ceil(1 << level).max(1),
                    ],
                )
            })
            .collect();
        let combined: Vec<_> = (1..LEVELS)
            .map(|level| {
                texture(
                    device,
                    [
                        size[0].div_ceil(1 << level).max(1),
                        size[1].div_ceil(1 << level).max(1),
                    ],
                )
            })
            .collect();
        let group = |image: &wgpu::TextureView, detail: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom blur inputs"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(image),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(detail),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.radius.as_entire_binding(),
                    },
                ],
            })
        };
        let down_groups = (1..LEVELS as usize)
            .map(|i| group(&levels[i - 1], &self.empty))
            .collect();
        let up_groups = (0..LEVELS as usize - 1)
            .map(|i| {
                group(
                    if i + 1 == LEVELS as usize - 1 {
                        &levels[i + 1]
                    } else {
                        &combined[i + 1]
                    },
                    &levels[i],
                )
            })
            .collect();
        let result = combined[0].clone();
        BloomTargets {
            preview,
            refined,
            result,
            extract,
            levels,
            combined,
            down_groups,
            up_groups,
        }
    }

    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &BloomTargets,
        query_set: Option<&wgpu::QuerySet>,
    ) {
        let [extract, down, up] = self.pipelines.as_ref().expect("bloom pipelines");
        let mut draw = |view: &wgpu::TextureView,
                        pipeline: &wgpu::RenderPipeline,
                        group: &wgpu::BindGroup,
                        last: bool| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cached neon bloom"),
                timestamp_writes: query_set.filter(|_| last).map(|query_set| {
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: None,
                        end_of_pass_write_index: Some(1),
                    }
                }),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        draw(&targets.levels[0], extract, &targets.extract, false);
        for i in 1..LEVELS as usize {
            draw(&targets.levels[i], down, &targets.down_groups[i - 1], false);
        }
        for i in (0..LEVELS as usize - 1).rev() {
            draw(&targets.combined[i], up, &targets.up_groups[i], i == 0);
        }
    }
}

impl Viewport {
    pub fn set_bloom(&mut self, enabled: bool, emission: bool) {
        if enabled != self.bloom.enabled || emission != self.bloom.emission {
            self.bloom.enabled = enabled;
            self.bloom.emission = emission;
            self.invalidate();
        }
    }
    pub fn bloom_settings(&self, queue: &wgpu::Queue, world: crate::model::World) {
        self.bloom.settings(queue, world);
    }
}

pub(super) fn composite_source() -> String {
    let source = include_str!("../../assets/shaders/viewport.wgsl").replace(
        "@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32>",
        "fn scene_color(input: VertexOutput) -> vec4<f32>",
    );
    format!("{source}\n@group(0) @binding(4) var halo: texture_2d<f32>;\n@group(0) @binding(5) var<uniform> bloom_settings: vec4<f32>;\n@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {{\n let color = scene_color(input);\n if bloom_settings.x <= 0.0 {{ return color; }}\n let glow = textureSampleLevel(halo, filtering, input.uv, 0.0).rgb * bloom_settings.x;\n let alpha = color.a + (1.0 - color.a) * clamp(max(glow.r, max(glow.g, glow.b)), 0.0, 1.0);\n if alpha < 0.5 / 255.0 {{ return vec4(0.0); }}\n return vec4((color.rgb * color.a + glow) / max(alpha, 0.00001), alpha);\n}}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bloom_blur_and_composite_validate() {
        for source in [
            include_str!("../../assets/shaders/bloom.wgsl").to_owned(),
            composite_source(),
            extract_source(),
            deferred_emission_source(false),
            deferred_emission_source(true),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}

fn deferred_emission_source(bloom: bool) -> String {
    let source = deferred_shader_source()
        .replace(
            "    let optical = textureLoad(optics, coordinates, 0);",
            r#"    let optical = textureLoad(optics, coordinates, 0);
    var fabric_sheen = optical.w;
    var emitted = vec3(0.0);
    if optical.w < 0.0 {
        let header = material_headers[u32(-optical.w) - 1u];
        emitted = material_params[header.offset + 1u].yzw * clamp(optical.x, 0.0, 1.0);
        fabric_sheen = 0.0;
        if header.kind == 7u { fabric_sheen = material_params[header.offset + 5u].x; }
    }"#,
        )
        .replace("color.xyz * optical.w", "color.xyz * fabric_sheen")
        .replace(
            "    return vec4(shaded, alpha);",
            "    bloom_emission = emitted;\n    return vec4(shaded + emitted, alpha);",
        );
    let source = format!("{source}\nstruct MaterialHeader {{ kind: u32, offset: u32, length: u32, reserved: u32 }}\n@group(0) @binding(3) var<storage, read> material_headers: array<MaterialHeader>;\n@group(0) @binding(4) var<storage, read> material_params: array<vec4<f32>>;\nvar<private> bloom_emission: vec3<f32>;");
    if bloom {
        crate::renderer::bloom::scene_source(&source, "fs_main")
    } else {
        source
    }
}

pub(super) fn deferred_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    bloom: bool,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("emissive deferred lighting"),
        source: wgpu::ShaderSource::Wgsl(deferred_emission_source(bloom).into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("emissive deferred lighting"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &crate::renderer::bloom::scene_targets(format, bloom),
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

// Resolve each of the four native samples before downsampling. A one-pixel neon
// strip must contribute regardless of which pixel in the 2x2 block it occupies.
fn extract_source() -> String {
    let source = include_str!("../../assets/shaders/viewport.wgsl").replace(
        "@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32>",
        "fn emission_color(input: VertexOutput) -> vec4<f32>",
    );
    format!("{source}\n@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {{\n let step = vec2(0.5) / vec2<f32>(textureDimensions(refined));\n return (emission_color(VertexOutput(input.position, input.uv + step))\n + emission_color(VertexOutput(input.position, input.uv - step))\n + emission_color(VertexOutput(input.position, input.uv + step * vec2(1.0, -1.0)))\n + emission_color(VertexOutput(input.position, input.uv + step * vec2(-1.0, 1.0)))) * 0.25;\n}}")
}
