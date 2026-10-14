//! Explicit shader variants keep non-emissive scenes on one color attachment.
pub(crate) fn scene_source(source: &str, entry: &str) -> String {
    // Keep the large SDF shader in the entry point. Moving it into a helper
    // produced corrupted Metal fragments for the complex Astra CSG scene.
    let marker = format!("@fragment fn {entry}(");
    let start = source.find(&marker).expect("bloom fragment entry");
    let body_start = start + source[start..].find('{').expect("fragment body");
    let mut depth = 1;
    let mut body_end = body_start + 1;
    for (offset, ch) in source[body_start + 1..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            body_end = body_start + 1 + offset;
            break;
        }
    }
    let signature = source[start..body_start]
        .replace("-> @location(0) vec4<f32>", "-> BloomOutput")
        .replace("->@location(0) vec4<f32>", "-> BloomOutput");
    let body = &source[body_start + 1..body_end];
    let mut output = String::from("\n bloom_emission = vec3(0.0);\n");
    let mut remaining = body;
    while let Some((prefix, after_return)) = remaining.split_once("return ") {
        let (expression, suffix) = after_return.split_once(';').expect("fragment return");
        output.push_str(prefix);
        output.push_str(&format!("return bloom_color({expression});"));
        remaining = suffix;
    }
    output.push_str(remaining);
    format!("{}{}{{{output}}}{}\nstruct BloomOutput {{ @location(0) color: vec4<f32>, @location(1) emission: vec4<f32> }}\nfn bloom_color(color: vec4<f32>) -> BloomOutput {{ return BloomOutput(color, vec4(bloom_emission, 1.0)); }}", &source[..start], signature, &source[body_end + 1..])
}

pub(crate) fn scene_targets(
    format: wgpu::TextureFormat,
    bloom: bool,
) -> Vec<Option<wgpu::ColorTargetState>> {
    let mut targets = vec![Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    if bloom {
        targets.push(Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba16Float,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selective_bloom_shader_variants_validate() {
        let scene = crate::renderer::material_gpu::shader_source();
        for source in [
            scene_source(&scene, "fs_main"),
            geometry_source(&scene),
            scene_source(
                &crate::renderer::poisson_material::source(&scene, 2),
                "fs_poisson",
            ),
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

/// Negative optics.w carries the emitting material index. Non-emissive fabric
/// retains its existing positive sheen payload; emitting fabric reads sheen
/// from the material slot. This stays within WebGPU's 32-byte attachment budget.
pub(super) fn geometry_source(source: &str) -> String {
    source.replace("    output.depth = clamp(clip.z / clip.w, 0.0, 1.0);", "    let emission_index = material_capture(point, -ray, object).index;\n    let emitted = material_params[material_headers[emission_index].offset + 1u].yzw;\n    if any(emitted > vec3(0.0)) { output.optics.w = -f32(emission_index + 1u); }\n    output.depth = clamp(clip.z / clip.w, 0.0, 1.0);")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests {
    use crate::{
        model::{BackgroundMode, GroupRenderRepresentation, PrimitiveKind, SdfObject},
        renderer::HeadlessOptions,
        Camera, Renderer, World,
    };
    use std::time::{Duration, Instant};
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn neon_raster_mesh_bloom_extends_silhouette() {
        let mut renderer = pollster::block_on(Renderer::new_headless(HeadlessOptions {
            width: 160,
            height: 160,
            ..Default::default()
        }))
        .unwrap();
        let mut camera = Camera::new();
        camera.position = glam::Vec3::new(0.0, 0.0, 3.0);
        camera.target = glam::Vec3::ZERO;
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.render_representation = GroupRenderRepresentation::DirectMesh;
        object.poisson_mesh.resolution = 16;
        object.material.emission_color = glam::Vec3::new(0.01, 0.4, 1.0);
        object.material.emission_intensity = 4.0;
        let world = World {
            background: BackgroundMode::Transparent,
            ..Default::default()
        };
        let source = vec![object.clone()];
        let context = egui::Context::default();
        renderer
            .poisson_mesh
            .handle(crate::renderer::poisson_mesh::PoissonMeshAction::Build(
                object.uuid,
            ));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            renderer.advance_computations(&camera, &source, &[], [1, 1], 0, world, &context, true);
            if renderer.mesh_vertex_count > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "direct neon mesh build timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let off = renderer
            .render_offscreen(
                &camera,
                &source,
                World {
                    bloom_enabled: false,
                    ..world
                },
                &[],
                Duration::from_secs(60),
            )
            .unwrap();
        let on = renderer
            .render_offscreen(&camera, &source, world, &[], Duration::from_secs(60))
            .unwrap();
        assert!(renderer.mesh_vertex_count > 0);
        let halo = on
            .rgba
            .chunks_exact(4)
            .zip(off.rgba.chunks_exact(4))
            .filter(|(a, b)| b[3] == 0 && a[3] > 0 && a[2] > 0)
            .count();
        assert!(halo > 100, "rasterized neon must produce a halo: {halo}");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "bloom/performance.rs"]
mod performance;
