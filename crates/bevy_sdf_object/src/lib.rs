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

const MAX_SDFS_PER_ENTITY: i32 = 256;

#[derive(Clone)]
pub struct SDFObject {
    pub uuid: uuid::Uuid,
    pub position: Vec3,
    pub quaternion: Quat,
    pub scale: Vec3,
    pub color: Vec4,
    pub object_type: i32,
}

impl SDFObject {
    /// Create a new individually-addressable object (with different uuid)
    pub fn duplicate(&self) -> Self {
        let mut clone = self.clone();
        clone.uuid = uuid::Uuid::new_v4();
        return clone;
    }

    pub fn inverse_transform_matrix(&self) -> Mat4 {
        return Transform::from_rotation(self.quaternion)
            .compute_matrix()
            .inverse();
    }
}

impl Default for SDFObject {
    fn default() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            position: Vec3::default(),
            quaternion: Quat::from_rotation_x(2.0),
            scale: Vec3::ONE,
            color: Vec4::default(),
            object_type: TYPE_END,
        }
    }
}

/// SDFObjectMaterial
/// This material uses our raymarching shader to display SDF objects.
// TODO: move to strorage buffers once chrome supports it.
#[derive(Asset, TypeUuid, TypePath, AsBindGroup, Clone)]
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
    pub sdf_scales: [Vec4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(4)]
    pub sdf_colors: [Vec4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(5)]
    pub sdf_inverse_transforms: [Mat4; MAX_SDFS_PER_ENTITY as usize],
}

fn sphere_sdf(p: Vec3, r: f32) -> f32 {
    return p.length() - r;
}

fn box_sdf(p: Vec3, box_parameters: Vec3) -> f32 {
    let box_q = p.abs() - box_parameters;
    let max_box_q = Vec3::new(
        box_q.x.max(0.0),
        box_q.y.max(0.0),
        box_q.z.max(0.0)
    );
    return (max_box_q + box_q.x.max(box_q.y.max(box_q.z)).min(0.0)).length();
}

/// Compute the union of 2 distance fields.
fn sdf_union(d1: f32, d2: f32) -> f32 {
    return d1.min(d2);
}

fn object_distance(p: Vec3, object: &SDFObject) -> f32 {
    let sphere_r = 0.2;
    let box_parameters = Vec3::new(0.3, 0.3, 0.3);

    let scaled_position = (p - object.position) / object.scale;
    let d_current_object = match object.object_type {
        TYPE_SPHERE => {
            sphere_sdf(scaled_position, sphere_r)
        },
        TYPE_BOX => {
            box_sdf(scaled_position, box_parameters)
        },
        _ => { panic!("Not implemented!") }
    };

    // Correct the returned distance to account for the scale
    return d_current_object * object.scale.length() / Vec3::ONE.length();
}

const RUST_RAYMARCH_ITERATIONS: i32 = 64;

/// Raymarch/Raycast, e.g.: To find which object was clicked
/// This is not meant to be used in real time rendering.
/// For real time rendering, use shaders.
/// Returns uuid of first found object
pub fn raymarch(start_position: Vec3, ray: Vec3, objects: Vec<SDFObject>) -> Option<uuid::Uuid> {
    let mut position = start_position - ray.normalize();
    let direction = ray.normalize();
    // TODO un-hardcode
    let mut d = 10000.0;
    let selection_distance_threshold = 0.01;

    for _i in 1..RUST_RAYMARCH_ITERATIONS {
        for obj in objects.iter() {
            let d_current_object = object_distance(position, obj);

            d = sdf_union(d_current_object, d);

            if d < selection_distance_threshold {
                return Some(obj.uuid);
            }
        }

        position += direction * d;
    }

    return None
}

impl Default for SDFObjectMaterial {
    fn default() -> Self {
        Self {
            camera: Vec4::ZERO,
            sdf_meta: [IVec4 { w: TYPE_END, x: 0, y: 0, z: 0 }; MAX_SDFS_PER_ENTITY as usize],
            sdf_positions: [Vec4::ZERO; MAX_SDFS_PER_ENTITY as usize],
            sdf_scales: [Vec4::ONE; MAX_SDFS_PER_ENTITY as usize],
            sdf_colors: [Vec4::ZERO; MAX_SDFS_PER_ENTITY as usize],
            sdf_inverse_transforms: [Mat4::IDENTITY; MAX_SDFS_PER_ENTITY as usize],
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
        defs.push(ShaderDefVal::Int("TYPE_BOX".into(), TYPE_BOX));

        Ok(())
    }
}
