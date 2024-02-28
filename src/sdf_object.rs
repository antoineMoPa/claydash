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
const MAX_CONTROL_POINTS: i32 = 16;

#[derive(PartialEq,Copy,Clone,Serialize,Deserialize)]
pub enum ControlPointType {
    SphereRadius,
    BoxX,
    BoxY,
    BoxZ,
    None,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct ControlPoint {
    pub position: Vec3,
    pub control_point_type: ControlPointType,
    pub object_uuid: uuid::Uuid,
    pub label: String,
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

    fn sdf(&self, p: Vec3) -> f32 {
        let box_q = p.abs() - self.box_q;
        let max_box_q = Vec3::new(
            box_q.x.max(0.0),
            box_q.y.max(0.0),
            box_q.z.max(0.0)
        );
        return (max_box_q + box_q.x.max(box_q.y.max(box_q.z)).min(0.0)).length();
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

    fn sdf(&self, p: Vec3) -> f32 {
        return p.length() - self.radius;
    }
}

impl SDFObjectParams {
    pub fn update_material(&self, index: usize, material: &mut SDFObjectMaterial) {
        match self {
            SDFObjectParams::BoxParams(box_params) => box_params.update_material(index, material),
            SDFObjectParams::SphereParams(sphere_params) => sphere_params.update_material(index, material),
        }
    }

    pub fn sdf(&self, p: Vec3) -> f32 {
        match self {
            SDFObjectParams::BoxParams(box_params) => box_params.sdf(p),
            SDFObjectParams::SphereParams(sphere_params) => sphere_params.sdf(p),
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
    /// Index of the sdf object in the sdf_params uniform array
    #[serde(skip)]
    pub index: u32,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub enum SdfOperation {
    #[default]
    Union,
    Intersection,
    Exclusion,
    UseLhsAsIs,
    UseRhsAsIs,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub enum SDFTreeNode {
    SDFObjectTree(Box<SDFObjectTree>),
    SDFObject(SDFObject),
    #[default]
    None,
}

pub type SDFOperationList = Vec<SDFOperationListEntry>;


pub fn sdf_operation_list_to_ivec4_list(list: SDFOperationList) -> Vec<IVec4> {
    let mut vec: Vec<IVec4> = Vec::new();

    for entry in list.iter() {
        let entry = entry.clone();
        vec.push(IVec4 {
            w: entry.operation as i32,
            x: entry.lhs.index as i32,
            y: entry.rhs.index as i32,
            z: 0, // unused
        });
    }

    return vec;
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SDFOperationListEntry {
    lhs: SDFObject,
    rhs: SDFObject,
    operation: SdfOperation,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SDFObjectTree {
    lhs: SDFTreeNode,
    rhs: SDFTreeNode,
    operation: SdfOperation,
}


impl SDFObjectTree {
    pub fn get_vec_sdf_object(&self) -> Vec<SDFObject> {
        let mut lhs: Vec<SDFObject> = match &self.lhs {
            SDFTreeNode::SDFObject(object) => {
                vec!(object.clone())
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_object()
            },
            _ => {
                vec!()
            }
        };

        let mut rhs: Vec<SDFObject> = match &self.rhs {
            SDFTreeNode::SDFObject(object) => {
                let object = object.clone();
                vec!(object)
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_object()
            },
            _ => {
                vec!()
            }
        };

        lhs.append(&mut rhs);

        // TODO: Add mix operation

        return lhs;
    }

    pub fn get_vec_sdf_object_mut(&mut self) -> Vec<&mut SDFObject> {
        let mut lhs: Vec<&mut SDFObject> = match &mut self.lhs {
            SDFTreeNode::SDFObject(object) => {
                vec!(object)
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_object_mut()
            },
            _ => {
                vec!()
            }
        };

        let mut rhs: Vec<&mut SDFObject> = match &mut self.rhs {
            SDFTreeNode::SDFObject(object) => {
                vec!(object)
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_object_mut()
            },
            _ => {
                vec!()
            }
        };

        lhs.append(&mut rhs);

        return lhs;
    }

    pub fn remove_object_with_uuid(&mut self, uuid: uuid::Uuid) {
        match &mut self.lhs {
            SDFTreeNode::SDFObject(object) => {
                if object.uuid == uuid {
                    self.lhs = SDFTreeNode::None;
                }
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.remove_object_with_uuid(uuid);
            },
            _ => {}
        }

        match &mut self.rhs {
            SDFTreeNode::SDFObject(object) => {
                if object.uuid == uuid {
                    self.rhs = SDFTreeNode::None;
                }
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.remove_object_with_uuid(uuid);
            },
            _ => {}
        }
    }

    /// Explore the SDF Object Tree and create an array of objects.
    ///
    /// The right hand node will define the mix operation (how to mix the object with
    /// previous object).
    ///
    /// Here is an example tree:
    ///
    ///
    ///                 union
    ///                /      \
    ///               /        \
    ///              /          \
    ///          intersection  object 3
    ///            /   \
    ///           /     \
    ///      object 1   object 2
    ///
    /// Here is the result we expect:
    ///
    /// | object   | mix operations                                 |
    /// |          |                                                |
    /// | object 1 | none (first object no mix operation necessary) |
    /// | object 2 | intersection                                   |
    /// | object 3 | union                                          |
    ///
    ///
    pub fn get_vec_sdf_operation(&self) -> Vec<SDFOperationListEntry> {
        let mut lhs: Vec<SDFOperationListEntry> = match &self.lhs {
            SDFTreeNode::SDFObject(object) => {
                vec!(SDFOperationListEntry{
                    lhs: object.clone(),
                    rhs: SDFObject::default(),
                    operation: SdfOperation::UseLhsAsIs,
                })
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_operation()
            },
            _ => {
                panic!("uninitialized tree node");
            }
        };

        let mut rhs: Vec<SDFOperationListEntry> = match &self.rhs {
            SDFTreeNode::SDFObject(object) => {
                vec!(SDFOperationListEntry{
                    lhs: object.clone(),
                    rhs: SDFObject::default(),
                    operation: SdfOperation::UseRhsAsIs,
                })
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.get_vec_sdf_operation()
            },
            _ => {
                panic!("uninitialized tree node");
            }
        };

        rhs[0].operation = self.operation.clone();

        lhs.append(&mut rhs);

        return lhs;
    }

    /// Add an object to the tree
    pub fn add_object(&mut self, object: SDFObject) {
        match &mut self.lhs {
            SDFTreeNode::None => {
                self.lhs = SDFTreeNode::SDFObject(object);
            },
            SDFTreeNode::SDFObjectTree(tree) => {
                tree.add_object(object);
            },
            _ => {
                match &mut self.rhs {
                    SDFTreeNode::None => {
                        self.rhs = SDFTreeNode::SDFObject(object);
                    },
                    SDFTreeNode::SDFObjectTree(tree) => {
                        tree.add_object(object);
                    },
                    _ => {
                        // Convert rhs to a tree and add the object to the tree
                        let tree = SDFObjectTree {
                            lhs: self.rhs.clone(),
                            rhs: SDFTreeNode::SDFObject(object),
                            operation: SdfOperation::Union,
                        };
                        self.rhs = SDFTreeNode::SDFObjectTree(Box::new(tree));
                    }
                }
            }
        }
    }

}

/// Create iterator that allows mutating the sdf objects in the tree
impl Iterator for SDFObjectTree {
    type Item = SDFObject;

    fn next(&mut self) -> Option<SDFObject> {
        //let mut vec = self.get_vec_sdf_object();
        return None;
    }
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
                let r: f32 = match &self.params {
                    SDFObjectParams::SphereParams(params) => { params.radius },
                    _ => { panic!("No sphere params.") }
                };

                let s = self.transform.scale;

                let radius_control_point = ControlPoint {
                    position: self.transform.translation + Vec3::new(r, 0.0, 0.0) * s,
                    control_point_type: ControlPointType::SphereRadius,
                    object_uuid: self.uuid,
                    label: "radius".to_owned(),
                };
                vec!(radius_control_point)
            },
            TYPE_BOX => {
                let box_q: Vec3 = match &self.params {
                    SDFObjectParams::BoxParams(params) => { params.box_q },
                    _ => { panic!("No sphere params.") }
                };

                let s = self.transform.scale;
                let r = self.transform.rotation;
                let x_control_point = ControlPoint {
                    position: self.transform.translation + r * Vec3::new(box_q.x, 0.0, 0.0) * s,
                    control_point_type: ControlPointType::BoxX,
                    object_uuid: self.uuid,
                    label: "x size".to_owned(),
                };

                let y_control_point = ControlPoint {
                    position: self.transform.translation + r * Vec3::new(0.0, box_q.y, 0.0) * s,
                    control_point_type: ControlPointType::BoxY,
                    object_uuid: self.uuid,
                    label: "y size".to_owned(),
                };

                let z_control_point = ControlPoint {
                    position: self.transform.translation + r * Vec3::new(0.0, 0.0, box_q.z) * s,
                    control_point_type: ControlPointType::BoxZ,
                    object_uuid: self.uuid,
                    label: "z size".to_owned(),
                };


                vec!(x_control_point, y_control_point, z_control_point)
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
            index: 0,
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
    /// w: operation type
    /// x: lhs index
    /// y: rhs index
    /// z: unused
    #[uniform(7)]
    pub sdf_operations: [IVec4; MAX_SDFS_PER_ENTITY as usize],
    #[uniform(8)]
    pub control_point_positions: [Vec4; MAX_CONTROL_POINTS as usize],
    #[uniform(9)]
    pub num_control_points: IVec4, // Padded to respect alignment constraints. Only first value is used.
}

/// Compute the union of 2 distance fields.
fn sdf_union(d1: f32, d2: f32) -> f32 {
    return d1.min(d2);
}

fn object_distance(p: Vec3, object: &SDFObject) -> f32 {
    let transformed_position = (object.inverse_transform_matrix() * Vec4::from((p, 1.0))).xyz();

    let d_current_object = object.params.sdf(transformed_position);

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
            sdf_operations: [IVec4::ZERO; MAX_SDFS_PER_ENTITY as usize],
            control_point_positions: [Vec4::ZERO; MAX_CONTROL_POINTS as usize],
            num_control_points: IVec4::ZERO,
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
