use super::*;
use crate::model::{PrimitiveKind, SheetBend, SheetCut, SheetEmboss, SheetPanelParams, SheetStage};
use wgpu::util::DeviceExt;

fn source() -> String {
    format!("struct Object {{ params: vec4<f32> }}\n\
@group(0) @binding(0) var<storage, read> objects: array<Object>;\n\
@group(0) @binding(1) var<storage, read> polygon_points: array<vec2<f32>>;\n\
@group(0) @binding(2) var<storage, read> samples: array<vec4<f32>>;\n\
@group(0) @binding(3) var<storage, read_write> result: array<f32>;\n\
{}\n\
@compute @workgroup_size(64) fn check(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
 if id.x < arrayLength(&samples) {{ result[id.x] = sheet_distance(samples[id.x].xyz, objects[0]); }}\n\
}}", include_str!("../../assets/shaders/sheet.wgsl"))
}

#[test]
fn directional_sheet_shader_is_portable_and_upload_preserves_lines() {
    let module = wgpu::naga::front::wgsl::parse_str(&source()).unwrap();
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
    let mut panel = SheetPanelParams::default();
    panel.bends.push(SheetBend {
        start: 0.1,
        direction: 0.7,
        radius: 0.2,
        angle: -1.2,
    });
    panel.embosses.push(SheetEmboss::Circle {
        center: Vec2::ZERO,
        radius: 0.1,
        depth: 0.025,
        transition: 0.1,
    });
    for stage in [SheetStage::Blank, SheetStage::Cut, SheetStage::Formed] {
        panel.stage = stage;
        object.params = SdfParams::SheetPanelParams(panel.clone());
        let mut points = Vec::new();
        let packed = pack_primitive(&object, &[], &[], Vec3::ONE, 1.0, &mut points, &mut 0);
        assert_eq!(
            packed.params[2],
            if stage == SheetStage::Formed {
                1.0
            } else {
                0.0
            }
        );
        if stage == SheetStage::Formed {
            assert_eq!(points[4].position, [0.1, 0.7]);
            assert_eq!(points[5].position, [0.2, -1.2]);
            assert_eq!(points[2].position[0], 1.0);
            assert_eq!(points[7].position, [0.0, 0.0]);
            assert_eq!(points[8].position, [0.1, 0.025]);
            assert_eq!(points[9].position, [0.1, 0.0]);
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn directional_sheet_cpu_gpu_fields_agree_for_cuts_stages_and_four_bends() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .expect("GPU adapter");
        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .expect("GPU device");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("directional sheet CPU/GPU agreement"),
            source: wgpu::ShaderSource::Wgsl(source().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("check"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut panel = SheetPanelParams::default();
        panel.bends = vec![
            SheetBend {
                start: -0.75,
                direction: 0.0,
                radius: 0.2,
                angle: 2.3,
            },
            SheetBend {
                start: -0.1,
                direction: 1.5707963,
                radius: 0.13,
                angle: -1.2,
            },
            SheetBend {
                start: -0.2,
                direction: -0.7,
                radius: 3.0,
                angle: 0.8,
            },
            SheetBend {
                start: 0.15,
                direction: 0.9,
                radius: 5.0,
                angle: -0.6,
            },
        ];
        panel.cuts = vec![
            SheetCut::Circle {
                center: Vec2::new(-0.85, -0.3),
                radius: 0.1,
            },
            SheetCut::Straight {
                from: Vec2::new(0.9, 0.0),
                to: Vec2::new(0.9, 1.0),
            },
        ];
        let mut flat_emboss = SheetPanelParams::default();
        flat_emboss.embosses = vec![
            SheetEmboss::Circle {
                center: Vec2::new(-0.3, -0.35),
                radius: 0.2,
                depth: 0.04,
                transition: 0.1,
            },
            SheetEmboss::Circle {
                center: Vec2::new(0.3, 0.15),
                radius: 0.15,
                depth: -0.03,
                transition: 0.12,
            },
        ];
        flat_emboss.cuts.push(SheetCut::Circle {
            center: Vec2::new(-0.3, -0.35),
            radius: 0.2,
        });
        let mut legacy_emboss = flat_emboss.clone();
        legacy_emboss.bends.push(SheetBend {
            start: -0.7,
            direction: 0.0,
            radius: 0.35,
            angle: 1.4,
        });
        let mut directional_emboss = panel.clone();
        directional_emboss.embosses = flat_emboss.embosses.clone();
        let mut compound = flat_emboss.clone();
        compound.surface = Some(crate::model::SheetSurface { heights: [0.0,0.04,0.04,0.0,0.04,0.12,0.12,0.04,0.04,0.12,0.12,0.04,0.0,0.04,0.04,0.0] });
        compound.path_embosses = vec![crate::model::SheetPathEmboss {
            stations: vec![
                crate::model::SheetPathStation { point: Vec2::new(-0.7,0.0),width:0.06,depth:0.02,profile:crate::model::SheetPathProfile::Rounded },
                crate::model::SheetPathStation { point: Vec2::new(0.0,0.2),width:0.12,depth:0.035,profile:crate::model::SheetPathProfile::Crease },
                crate::model::SheetPathStation { point: Vec2::new(0.7,0.0),width:0.06,depth:0.02,profile:crate::model::SheetPathProfile::Rounded },
            ], taper_start:0.2,taper_end:0.2,
        }];
        let mut legacy_compound = compound.clone();
        legacy_compound.bends.push(SheetBend { start: -0.7,direction:0.0,radius:0.5,angle:1.2 });
        let mut directional_compound = compound.clone();
        directional_compound.bends = panel.bends.clone();
        directional_compound.bends[0].radius=0.5;
        directional_compound.bends[1].radius=0.5;
        for recipe in [panel, flat_emboss, legacy_emboss, directional_emboss, compound, legacy_compound, directional_compound] {
            let mut panel = recipe;
            assert!(panel.validate().is_ok());
            for stage in [SheetStage::Blank, SheetStage::Cut, SheetStage::Formed] {
                panel.stage = stage;
                let mut object = SdfObject::create_kind(PrimitiveKind::SheetPanel);
                object.params = SdfParams::SheetPanelParams(panel.clone());
                let mut points = Vec::new();
                let packed = pack_primitive(&object, &[], &[], Vec3::ONE, 1.0, &mut points, &mut 0);
                let mut samples: Vec<[f32; 4]> = Vec::new();
                for i in 0..11 {
                    for j in 0..9 {
                        for k in 0..7 {
                            samples.push([
                                -1.0 + i as f32 * 0.2,
                                -0.6 + j as f32 * 0.15,
                                -0.4 + k as f32 * 0.15,
                                0.0,
                            ]);
                        }
                    }
                }
                for i in 0..40 {
                    let p = panel.material_point(Vec2::new(-0.95 + i as f32 * 0.05, 0.2));
                    samples.push([p.x, p.y, p.z, 0.0]);
                }
                for center in [Vec2::new(-0.3, -0.35), Vec2::new(0.3, 0.15)] {
                    for radius in [0.15, 0.2, 0.25, 0.3] {
                        for i in 0..12 {
                            let angle = i as f32 * std::f32::consts::TAU / 12.0;
                            let uv = center + Vec2::new(angle.cos(), angle.sin()) * radius;
                            let p = panel.material_point(uv);
                            for z in [-0.05, 0.0, 0.05] {
                                samples.push([p.x, p.y, p.z + z, 0.0]);
                            }
                        }
                    }
                }
                let storage = |bytes: &[u8]| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytes,
                        usage: wgpu::BufferUsages::STORAGE,
                    })
                };
                let objects = storage(bytemuck::cast_slice(&[packed.params]));
                let points = storage(bytemuck::cast_slice(&points));
                let inputs = storage(bytemuck::cast_slice(&samples));
                let bytes = samples.len() as u64 * 4;
                let result = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: bytes,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: bytes,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: objects.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: points.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: inputs.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: result.as_entire_binding(),
                        },
                    ],
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups((samples.len() as u32).div_ceil(64), 1, 1);
                }
                encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, bytes);
                queue.submit([encoder.finish()]);
                let (sender, receiver) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |status| {
                        sender.send(status).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(30)),
                    })
                    .unwrap();
                receiver.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                let values: &[f32] = bytemuck::cast_slice(&mapped);
                for (sample, &gpu) in samples.iter().zip(values) {
                    let cpu = panel.distance(Vec3::new(sample[0], sample[1], sample[2]));
                    assert!(
                        (cpu - gpu).abs() < 2e-5,
                        "{stage:?}: {sample:?}, CPU={cpu}, GPU={gpu}"
                    );
                }
            }
        }
    });
}
