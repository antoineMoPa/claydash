use bevy_reflect::{TypePath,TypeUuid};
use bevy::{
    prelude::*,
    pbr::{
        MaterialPipeline,
        MaterialPipelineKey,
    },
    render::{
        mesh::MeshVertexBufferLayout,
        render_resource::{
            AsBindGroup, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError, ShaderDefVal,
        },
    },
};
use sdf_consts::*;

pub struct BevySDFObjectPlugin;

impl Plugin for BevySDFObjectPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SDFObjectMaterial>::default());
    }
}

const MAX_SDFS_PER_ENTITY: i32 = 512;

#[derive(Clone)]
pub struct SDFObject {
    pub uuid: uuid::Uuid,
    pub position: Vec3,
    pub color: Vec4,
    pub object_type: i32,
}

impl Default for SDFObject {
    fn default() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            position: Vec3::default(),
            color: Vec4::default(),
            object_type: TYPE_END,
        }
    }
}

/// SDFObjectMaterial
/// This material uses our raymarching shader to display SDF objects.
// TODO: move to strorage buffers once chrome supports it.
#[derive(TypeUuid, TypePath, AsBindGroup, Clone)]
#[uuid = "84F24BEA-CC34-4A35-B223-C5C148A14722"]
#[repr(C,align(16))]
pub struct SDFObjectMaterial {
    #[uniform(0)]
    pub camera: Vec4,
    // w: object type
    // x: 0: not-selected. 1: selected
    #[uniform(1)]
    pub sdf_meta: [IVec4; MAX_SDFS_PER_ENTITY as usize], // using vec4 instead of i32 solves webgpu align issues
    #[uniform(2)]
    pub sdf_positions: [Vec4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(3)]
    pub sdf_colors: [Vec4; MAX_SDFS_PER_ENTITY as usize],
}

fn sdf_union(d1: f32, d2: f32) -> f32 {
    return d1.min(d2);
}

/// Raymarch/Raycast, e.g.: To find which object was clicked
/// This is not meant to be used in real time rendering.
/// For real time rendering, use shaders.
/// Returns uuid of first found object
pub fn raymarch(start_position: Vec3, ray: Vec3, objects: Vec<SDFObject>) -> Option<uuid::Uuid> {
    let mut position = start_position - ray.normalize();
    let direction = ray.normalize();
    // TODO un-hardcode
    let sphere_r = 0.2;
    let box_parameters = Vec3::new(0.3, 0.3, 0.3);
    let mut d = 10000.0;
    let mut d_current_object: f32 = 0.0;
    let selection_distance_threshold = 0.01;

    for _i in 1..64 {
        for obj in objects.iter() {
            let t = obj.object_type;

            if t == TYPE_SPHERE {
                d_current_object = (position - obj.position).length() - sphere_r;
            }
            else if t == TYPE_CUBE {
                let box_q = (position - obj.position).abs() - box_parameters;
                let max_box_q = Vec3::new(
                    box_q.x.max(0.0),
                    box_q.y.max(0.0),
                    box_q.z.max(0.0)
                );
                d_current_object = (max_box_q + box_q.x.max(box_q.y.max(box_q.z)).min(0.0)).length();
            }

            d = sdf_union(d_current_object, d);

            if d < selection_distance_threshold {
                return Some(obj.uuid);
            }
        }

        position += direction * d;
    }

    println!("D {}", d);

    return None
}

impl Default for SDFObjectMaterial {
    fn default() -> Self {
        Self {
            camera: Vec4::new(0.0, 0.0, 0.0, 0.0),
            sdf_meta: [IVec4 { w: TYPE_END, x: 0, y: 0, z: 0 }; MAX_SDFS_PER_ENTITY as usize],
            sdf_positions: [Vec4::new(0.0, 0.0, 0.0, 0.0); MAX_SDFS_PER_ENTITY as usize],
            sdf_colors: [Vec4::new(0.0, 0.0, 0.0, 0.0); MAX_SDFS_PER_ENTITY as usize],
        }
    }
}

impl Material for SDFObjectMaterial {
    fn fragment_shader() -> ShaderRef {
        return "shaders/all.wgsl".into();
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayout,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let fragment = descriptor.fragment.as_mut().unwrap();
        ShaderDefVal::Int("MAX_SDFS_PER_ENTITY".into(), MAX_SDFS_PER_ENTITY);

        let defs = &mut fragment.shader_defs;

        defs.push(ShaderDefVal::Int(
            "MAX_SDFS_PER_ENTITY".into(),
            MAX_SDFS_PER_ENTITY)
        );

        defs.push(ShaderDefVal::Int("TYPE_END".into(), TYPE_END));
        defs.push(ShaderDefVal::Int("TYPE_SPHERE".into(), TYPE_SPHERE));
        defs.push(ShaderDefVal::Int("TYPE_CUBE".into(), TYPE_CUBE));

        Ok(())
    }
}
