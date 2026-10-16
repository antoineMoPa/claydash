//! Contact probes and material transport use separate kernels. In particular,
//! sheet queries must not inherit the live reflection/transmission state.
use super::*;

pub(super) fn transport_source(source: &str) -> String {
    let source = source.replace("fn ambient_occlusion(", "fn evaluate_contact_occlusion(")
        .replace("@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {",
            "@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {\n contact_pixel = vec2<i32>(input.position.xy);");
    format!("{source}\n@group(1) @binding(0) var contact_image: texture_2d<f32>;\nvar<private> contact_pixel: vec2<i32>;\nfn ambient_occlusion(point: vec3<f32>, normal: vec3<f32>) -> f32 {{ return textureLoad(contact_image, contact_pixel, 0).x; }}\n")
}

pub(super) fn probe_source(source: &str) -> String {
    format!(
        "{}\n{}",
        source.replace("@fragment fn fs_main(", "@fragment fn fs_transport_unused("),
        include_str!("../../assets/shaders/contact.wgsl")
    )
}

impl Renderer {
    pub(super) fn contact_transport_layout(&self) -> wgpu::PipelineLayout {
        self.device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("contact and material transport layout"),
                bind_group_layouts: &[
                    Some(&self.bind_group_layout),
                    Some(self.viewport.contact_layout()),
                ],
                immediate_size: 0,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_and_transport_entries_validate_independently() {
        let source = super::super::material_gpu::shader_source();
        for source in [probe_source(&source), transport_source(&source)] {
            let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap();
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn separate_contact_preserves_native_crowned_sheet_shading() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 24,
            height: 16,
            ..Default::default()
        }))
        .unwrap();
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::SheetPanel);
        if let crate::model::SdfParams::SheetPanelParams(panel) = &mut object.params {
            panel.surface = Some(crate::model::SheetSurface {
                heights: [
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.2, 0.2, 0.0, 0.0, 0.2, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0,
                ],
            });
        }
        let scene = vec![object];
        let world = World::default();
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.4, 0.3, 3.0);
        camera.target = Vec3::ZERO;
        camera.viewport = glam::Vec2::new(24.0, 16.0);
        let split = renderer
            .render_offscreen(
                &camera,
                &scene,
                world,
                &[],
                std::time::Duration::from_secs(10),
            )
            .unwrap();
        let features = renderer.scene_pipeline_cache.back().unwrap().0.features;
        renderer.pipeline = create_scene_pipeline_for_materials(
            &renderer.device,
            &renderer.shader_source,
            &renderer.pipeline_layout,
            renderer.render_format,
            true,
            1,
            false,
            false,
            features,
            false,
        );
        renderer.viewport.set_contact_pipeline(None);
        let direct = renderer.direct_reference_pixels(false);
        let mut split_ppm = b"P6\n24 16\n255\n".to_vec();
        for pixel in split.rgba.chunks_exact(4) {
            split_ppm.extend_from_slice(&pixel[..3]);
        }
        assert_eq!(
            split_ppm, direct,
            "separating the probes must preserve source shading"
        );
    }
}
