use crate::model::Material;

// Six metal slots follow the two common slots. Tint stays on the object color.
pub(super) fn pack_metal(material: Material, params: &mut Vec<[f32; 4]>) {
    let m = material.metal;
    params.extend_from_slice(&[
        [
            m.finish.gpu_code(),
            m.tangent.gpu_code(),
            m.anisotropy,
            m.brush_angle,
        ],
        [m.texture_scale, m.relief_strength, m.scratches, m.oxidation],
        [m.paint_coverage, m.paint_roughness, m.image_relief, 0.0],
        [m.paint_color.x, m.paint_color.y, m.paint_color.z, 0.0],
        [m.species.f0().x, m.species.f0().y, m.species.f0().z, 0.0],
        [
            m.species.oxide().x,
            m.species.oxide().y,
            m.species.oxide().z,
            0.0,
        ],
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn height_image_is_neutral_when_absent_and_preserves_other_stencils() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .unwrap();
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let mut packed = super::super::PackedMaterials::default();
            let plain = Material::metal_preset(crate::model::MetalSpecies::Copper);
            let mut height = plain;
            height.metal.image_relief = 1.0;
            packed.insert(plain);
            packed.insert(height);
            packed.insert(Material::default());
            let common = include_str!("../../../assets/shaders/material_common.wgsl");
            let surface_start = common.find("struct Surface {").unwrap();
            let surface_end = surface_start + common[surface_start..].find("\n}").unwrap() + 2;
            let stencil_helpers = &common[common.find("fn metal_image_detail_enabled").unwrap()..];
            let source = format!(
                r#"
const HAS_METAL_MATERIAL: bool = true;
const MATERIAL_METAL: u32 = 8u;
struct Camera {{ position: vec4<f32> }}
const camera = Camera(vec4(0.0, 0.0, 3.0, 1.0));
struct Object {{ inverse_rows: array<vec4<f32>, 3>, scale: vec4<f32>, component: vec4<u32>, stencil_meta: vec4<f32> }}
struct MaterialHeader {{ kind: u32, offset: u32, length: u32, reserved: u32 }}
@group(0) @binding(0) var<storage, read> material_headers: array<MaterialHeader>;
@group(0) @binding(1) var<storage, read> material_params: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;
{surface}
fn stencil_color(point: vec3<f32>, normal: vec3<f32>, object: Object) -> vec4<f32> {{
    if object.stencil_meta.x < 0.5 {{ return vec4(0.0); }}
    // Controlled linear grayscale ramp stands in for decoded atlas texels.
    return vec4(vec3(0.5 + point.x), 1.0);
}}
{metal}
{helpers}
@compute @workgroup_size(1) fn run() {{
    var object = Object(array<vec4<f32>, 3>(vec4(1.0, 0.0, 0.0, 0.0), vec4(0.0, 1.0, 0.0, 0.0), vec4(0.0, 0.0, 1.0, 0.0)), vec4(1.0), vec4<u32>(0u), vec4(0.0));
    let point = vec3(0.21, 0.12, 0.0);
    let normal = vec3(0.0, 0.0, 1.0);
    let base = Surface(vec3(1.0), normal, 0.12, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, vec3(0.0, 1.0, 0.0), 0.0, vec3(1.0, 0.0, 0.0), 0.0, false);
    let plain = evaluate_metal(point, normal, object, base, material_headers[0].offset);
    let absent = evaluate_metal(point, normal, object, base, material_headers[1].offset);
    output[0] = vec4(plain.normal, plain.roughness);
    output[1] = vec4(absent.normal, absent.roughness);
    object.stencil_meta.x = 1.0;
    output[2] = material_stencil_color(point, normal, object);
    object.component.w = 1u;
    output[3] = material_stencil_color(point, normal, object);
    let image = evaluate_metal(point, normal, object, base, material_headers[1].offset);
    output[4] = vec4(image.normal, image.roughness);
    object.component.w = 2u;
    output[5] = material_stencil_color(point, normal, object);
}}
"#,
                surface = &common[surface_start..surface_end],
                metal = include_str!("../../../assets/shaders/material_metal.wgsl"),
                helpers = stencil_helpers
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("metal image semantics"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("metal image semantics"),
                layout: None,
                module: &shader,
                entry_point: Some("run"),
                compilation_options: Default::default(),
                cache: None,
            });
            let headers = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&packed.headers),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&packed.params),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let output = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 96,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 96,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: headers.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: output.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 96);
            queue.submit([encoder.finish()]);
            let (sender, receiver) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    sender.send(result).unwrap()
                });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .unwrap();
            receiver.recv().unwrap().unwrap();
            let data = readback.slice(..).get_mapped_range().unwrap();
            let pixels: &[[f32; 4]] = bytemuck::cast_slice(&data);
            assert_eq!(
                pixels[0], pixels[1],
                "no image must not perturb metal normals or roughness"
            );
            assert_eq!(
                pixels[2], pixels[5],
                "Solid and Metal with zero image relief retain identical decal samples"
            );
            assert!(pixels[2][3] > 0.99);
            assert_eq!(
                pixels[3], [0.0; 4],
                "height interpretation suppresses the color decal"
            );
            assert!(
                (pixels[4][0] - pixels[1][0]).abs() > 0.001,
                "uploaded grayscale height changes the normal"
            );
        });
    }
}
