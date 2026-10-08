use crate::model::PostProcessPass;

const PREFIX: &str = r#"
struct Frame { frame_size: vec2<f32>, origin: vec2<f32>, resolution: vec2<f32>, time: f32, pad: f32 }
@group(0) @binding(0) var scene_color: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var<uniform> frame: Frame;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    out.position = vec4(positions[index], 0.0, 1.0);
    out.uv = positions[index] * vec2(0.5, -0.5) + vec2(0.5);
    return out;
}
// UV is local to the scene viewport. Sampling clamps to that rectangle.
fn sample_scene(uv: vec2<f32>) -> vec4<f32> {
    let pixel = frame.origin + clamp(uv * frame.resolution, vec2(0.5), frame.resolution - vec2(0.5));
    return textureSampleLevel(scene_color, scene_sampler, pixel / frame.frame_size, 0.0);
}
fn effect(uv: vec2<f32>, color: vec4<f32>, resolution: vec2<f32>, time: f32) -> vec4<f32> {
"#;
const SUFFIX: &str = r#"
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.position.xy;
    let color = textureSampleLevel(scene_color, scene_sampler, pixel / frame.frame_size, 0.0);
    let uv = (pixel - frame.origin) / frame.resolution;
    if (any(uv < vec2(0.0)) || any(uv >= vec2(1.0))) { return color; }
    return effect(uv, color, frame.resolution, frame.time);
}
"#;

fn source(body: &str) -> String {
    format!("{PREFIX}{body}{SUFFIX}")
}

pub fn validate_passes(passes: &[PostProcessPass]) -> Result<(), String> {
    if passes.len() > 16 {
        return Err("at most 16 post-processing passes are supported".into());
    }
    let mut ids = std::collections::HashSet::new();
    for pass in passes {
        if !ids.insert(pass.uuid) {
            return Err(format!("duplicate post-processing pass {}", pass.uuid));
        }
        if pass.name.trim().is_empty() || pass.name.len() > 128 {
            return Err("post-processing pass name must be 1–128 bytes".into());
        }
        if pass.wgsl.trim().is_empty() || pass.wgsl.len() > 8192 {
            return Err(format!("{}: WGSL body must be 1–8192 bytes", pass.name));
        }
        let source = source(&pass.wgsl);
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .map_err(|error| format!("{}: {}", pass.name, error.emit_to_string(&source)))?;
        if module.functions.len() != 2
            || module.global_variables.len() != 3
            || module.entry_points.len() != 2
            || !module.overrides.is_empty()
        {
            return Err(format!(
                "{}: WGSL must contain only the body of effect",
                pass.name
            ));
        }
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|error| format!("{}: WGSL validation: {error}", pass.name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_contract_validates_and_rejects_bad_source() {
        let pass = PostProcessPass::new("Tint".into(), PostProcessPass::EXAMPLE.into());
        validate_passes(&[pass.clone()]).unwrap();
        let blur = PostProcessPass::new(
            "Blur".into(),
            "let step = vec2<f32>(1.0 / resolution.x, 0.0); return (sample_scene(uv - step) + color + sample_scene(uv + step)) / 3.0;".into(),
        );
        validate_passes(&[blur]).unwrap();
        let mut invalid = pass.clone();
        invalid.wgsl = "return vec4<f32>(missing_name);".into();
        assert!(validate_passes(&[invalid]).is_err());
        let mut injected = pass.clone();
        injected.wgsl = "return color; } @group(1) @binding(0) var<uniform> extra_binding: vec4<f32>; fn extra() -> vec4<f32> { return extra_binding;".into();
        assert!(wgpu::naga::front::wgsl::parse_str(&source(&injected.wgsl)).is_ok());
        assert!(validate_passes(&[injected]).is_err());
        assert!(validate_passes(&[pass.clone(), pass]).is_err());
    }

    #[test]
    fn pass_round_trips_in_scene() {
        let pass = PostProcessPass::new("Tint".into(), PostProcessPass::EXAMPLE.into());
        let bytes = serde_json::to_vec(&vec![pass.clone()]).unwrap();
        let restored: Vec<PostProcessPass> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, vec![pass]);
    }
}

struct Targets {
    _textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    size: [u32; 2],
}

pub(super) struct PostProcessor {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    frame_buffer: wgpu::Buffer,
    blit: wgpu::RenderPipeline,
    pipelines: Vec<(uuid::Uuid, String, wgpu::RenderPipeline)>,
    validation: Option<(Vec<PostProcessPass>, bool)>,
    targets: Option<Targets>,
    format: wgpu::TextureFormat,
    start: web_time::Instant,
}

impl PostProcessor {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-processing inputs"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post-processing layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post-processing frame"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blit = Self::create_pipeline(device, &pipeline_layout, format, "return color;");
        Self {
            layout,
            pipeline_layout,
            sampler,
            frame_buffer,
            blit,
            pipelines: Vec::new(),
            validation: None,
            targets: None,
            format,
            start: web_time::Instant::now(),
        }
    }

    pub fn is_valid(&mut self, passes: &[PostProcessPass]) -> bool {
        if let Some((checked, result)) = &self.validation {
            if checked == passes {
                return *result;
            }
        }
        let result = validate_passes(passes).is_ok();
        self.validation = Some((passes.to_vec(), result));
        result
    }

    fn create_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
        body: &str,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post-processing WGSL"),
            source: wgpu::ShaderSource::Wgsl(source(body).into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post-processing pass"),
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
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
    }

    pub fn prepare(&mut self, device: &wgpu::Device, passes: &[PostProcessPass], size: [u32; 2]) {
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| targets.size != size)
        {
            let make_texture = || {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("post-processing ping-pong"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            let textures = [make_texture(), make_texture()];
            let views = [
                textures[0].create_view(&Default::default()),
                textures[1].create_view(&Default::default()),
            ];
            self.targets = Some(Targets {
                _textures: textures,
                views,
                size,
            });
        }
        self.pipelines.retain(|(id, body, _)| {
            passes
                .iter()
                .any(|pass| pass.uuid == *id && pass.wgsl == *body)
        });
        for pass in passes.iter().filter(|pass| pass.enabled) {
            if !self
                .pipelines
                .iter()
                .any(|(id, body, _)| *id == pass.uuid && *body == pass.wgsl)
            {
                self.pipelines.push((
                    pass.uuid,
                    pass.wgsl.clone(),
                    Self::create_pipeline(device, &self.pipeline_layout, self.format, &pass.wgsl),
                ));
            }
        }
    }

    pub fn source_view(&self) -> &wgpu::TextureView {
        &self
            .targets
            .as_ref()
            .expect("prepared post-processing")
            .views[0]
    }

    fn bind_group(&self, device: &wgpu::Device, source: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-processing source"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.frame_buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        passes: &[PostProcessPass],
        viewport: [f32; 4],
        output: &wgpu::TextureView,
        display: Option<&wgpu::TextureView>,
    ) {
        let targets = self.targets.as_ref().expect("prepared post-processing");
        let uniforms = [
            targets.size[0] as f32,
            targets.size[1] as f32,
            viewport[0],
            viewport[1],
            viewport[2],
            viewport[3],
            self.start.elapsed().as_secs_f32(),
            0.0,
        ];
        queue.write_buffer(&self.frame_buffer, 0, bytemuck::cast_slice(&uniforms));
        let mut source_index = 0;
        for effect in passes.iter().filter(|pass| pass.enabled) {
            let target_index = 1 - source_index;
            let bind = self.bind_group(device, &targets.views[source_index]);
            let pipeline = &self
                .pipelines
                .iter()
                .find(|(id, body, _)| *id == effect.uuid && *body == effect.wgsl)
                .expect("validated post-processing pipeline")
                .2;
            self.draw(encoder, &targets.views[target_index], pipeline, &bind);
            source_index = target_index;
        }
        let bind = self.bind_group(device, &targets.views[source_index]);
        self.draw(encoder, output, &self.blit, &bind);
        if let Some(display) = display {
            self.draw(encoder, display, &self.blit, &bind);
        }
    }

    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post-processing draw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        pass.draw(0..3, 0..1);
    }
}
