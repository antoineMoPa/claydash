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
use serde::{Serialize, Deserialize};
use sdf_consts::*;

pub struct BevySDFObjectPlugin;

impl Plugin for BevySDFObjectPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SDFObjectMaterial>::default());
    }
}

const MAX_SDFS_PER_ENTITY: i32 = 256;
const MAX_CONTROL_POINTS: i32 = 32;

#[derive(PartialEq,Copy,Clone,Serialize,Deserialize)]
pub enum ControlPointType {
    SphereRadius,
    None,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct ControlPoint {
    pub position: Vec3,
    pub control_point_type: ControlPointType,
    pub object_uuid: uuid::Uuid,
}

impl ControlPoint {
    pub fn get_hit_distance(&self, camera_position: Vec3, ray: Vec3) -> f32 {
        let control_point_position = self.position;
        let camera_to_control_point_dist = control_point_position.distance(camera_position);
        let position_near_control_point = camera_position + ray * camera_to_control_point_dist;
        return (position_near_control_point - control_point_position).length();
    }
}

const CONTROL_POINT_CLICK_DISTANCE: f32 = 0.03;

/// Given a list of control points, find whether a ray starting at `position`
/// will hit any of the object's control points and returns the first hit control point.
pub fn control_points_hit(
    camera_position: Vec3,
    ray: Vec3,
    objects: &Vec<SDFObject>
) -> Option<ControlPoint> {

    for obj in objects.iter() {
        for control_point in obj.get_control_points().iter() {
            let hit_distance = control_point.get_hit_distance(camera_position, ray);
            if hit_distance < CONTROL_POINT_CLICK_DISTANCE {
                return Some(control_point.clone());
            }
        }
    }

    return None
}

#[derive(Clone,Serialize,Deserialize)]
pub struct BoxParams {
    pub box_q: Vec3,
}

impl Default for BoxParams {
    fn default() -> Self {
        Self {
            box_q: Vec3::new(0.3, 0.3, 0.3)
        }
    }
}

#[derive(Clone,Serialize,Deserialize)]
pub struct SphereParams {
    pub radius: f32,
}

impl Default for SphereParams {
    fn default() -> Self {
        Self { radius: 0.2 }
    }
}

#[derive(Clone,Serialize,Deserialize)]
pub enum SDFObjectParams {
    BoxParams(BoxParams),
    SphereParams(SphereParams)
}

impl BoxParams {
    pub fn update_material(&self, index: usize, material: &mut SDFObjectMaterial) {
        material.sdf_params[index] = Mat4::from_cols_array(&[
            self.box_q.x, self.box_q.y, self.box_q.z, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0
        ]);
    }
}

impl SphereParams {
    pub fn update_material(&self, index: usize, material: &mut SDFObjectMaterial) {
        material.sdf_params[index] = Mat4::from_cols_array(&[
            self.radius, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0
        ]);
    }
}

impl SDFObjectParams {
    pub fn update_material(&self, index: usize, material: &mut SDFObjectMaterial) {
        match self {
            SDFObjectParams::BoxParams(box_params) => box_params.update_material(index, material),
            SDFObjectParams::SphereParams(sphere_params) => sphere_params.update_material(index, material),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SDFObject {
    pub uuid: uuid::Uuid,
    pub transform: Transform,
    pub color: Vec4,
    pub object_type: i32,
    pub params: SDFObjectParams,
}

impl SDFObject {
    /// Create a new individually-addressable object (with different uuid)
    pub fn duplicate(&self) -> Self {
        let mut clone = self.clone();
        clone.uuid = uuid::Uuid::new_v4();
        return clone;
    }

    pub fn inverse_transform_matrix(&self) -> Mat4 {
        return self.transform.compute_matrix().inverse();
    }

    pub fn get_control_points(&self) -> Vec<ControlPoint> {
        match self.object_type {
            TYPE_SPHERE => {
                let radius_control_point = ControlPoint {
                    position: Vec3::new(0.4, 0.4, 0.4),
                    control_point_type: ControlPointType::SphereRadius,
                    object_uuid: self.uuid,
                };
                vec!(radius_control_point)
            },
            _ => vec!()
        }
    }

    pub fn create(object_type: i32) -> SDFObject {
        match object_type {
            sdf_consts::TYPE_SPHERE => SDFObject {
                object_type: sdf_consts::TYPE_SPHERE,
                params: SDFObjectParams::SphereParams(SphereParams::default()),
                ..SDFObject::default()
            },
            sdf_consts::TYPE_BOX => SDFObject {
                object_type: sdf_consts::TYPE_BOX,
                params: SDFObjectParams::BoxParams(BoxParams::default()),
                ..SDFObject::default()
            },
            _ => panic!("create() not implemented for {}", object_type)
        }
    }
}

impl Default for SDFObject {
    fn default() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            transform: Transform::IDENTITY,
            color: Vec4::default(),
            object_type: TYPE_END,
            params: SDFObjectParams::SphereParams(SphereParams::default()),
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
    #[uniform(1)]
    pub camera_right: Vec4,
    #[uniform(2)]
    pub camera_up: Vec4,
    // w: object type
    // x: 0: not-selected. 1: selected
    #[uniform(3)]
    pub sdf_meta: [IVec4; MAX_SDFS_PER_ENTITY as usize], // using vec4 instead of i32 solves webgpu align issues
    #[uniform(4)]
    pub sdf_colors: [Vec4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(5)]
    pub sdf_inverse_transforms: [Mat4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(6)]
    pub sdf_params: [Mat4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(7)]
    pub control_point_positions: [Vec4; MAX_CONTROL_POINTS as usize],
    #[uniform(8)]
    pub num_control_points: i32,
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
    let transformed_position = (object.inverse_transform_matrix() * Vec4::from((p, 1.0))).xyz();
    let d_current_object = match object.object_type {
        TYPE_SPHERE => {
            sphere_sdf(transformed_position, sphere_r)
        },
        TYPE_BOX => {
            box_sdf(transformed_position, box_parameters)
        },
        _ => { panic!("Not implemented!") }
    };

    // Correct the returned distance to account for the scale
    return d_current_object * object.transform.scale.length() / Vec3::ONE.length();
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

        position += direction * d * 0.3;
    }

    return None
}

impl Default for SDFObjectMaterial {
    fn default() -> Self {
        Self {
            camera: Vec4::ZERO,
            camera_up: Vec4::ZERO,
            camera_right: Vec4::ZERO,
            sdf_meta: [IVec4 { w: TYPE_END, x: 0, y: 0, z: 0 }; MAX_SDFS_PER_ENTITY as usize],
            sdf_colors: [Vec4::ZERO; MAX_SDFS_PER_ENTITY as usize],
            sdf_inverse_transforms: [Mat4::IDENTITY; MAX_SDFS_PER_ENTITY as usize],
            sdf_params: [Mat4::IDENTITY; MAX_SDFS_PER_ENTITY as usize],
            control_point_positions: [Vec4::ZERO; MAX_CONTROL_POINTS as usize],
            num_control_points: 0,
        }
    }
}

impl Material for SDFObjectMaterial {
    fn fragment_shader() -> ShaderRef {
        return "shaders/all.wgsl".into();
    }

    fn alpha_mode(&self) -> AlphaMode {
	AlphaMode::Blend
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
        defs.push(ShaderDefVal::Int(
            "MAX_CONTROL_POINTS".into(),
            MAX_CONTROL_POINTS)
        );

        defs.push(ShaderDefVal::Int("TYPE_END".into(), TYPE_END));
        defs.push(ShaderDefVal::Int("TYPE_SPHERE".into(), TYPE_SPHERE));
        defs.push(ShaderDefVal::Int("TYPE_BOX".into(), TYPE_BOX));

        Ok(())
    }
}
