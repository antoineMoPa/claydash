use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use sdf_consts::*;

use observable_key_value_tree::{
    ObservableKVTree,
    CanBeNone,
    Update,
    Snapshot
};

use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;

use crate::sdf_object::{SDFObjectMaterial, SDFObject, ControlPointType, SDFObjectTree, SDFOperation};

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    GrabbingControlPoint,
    Scaling,
    Rotating,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    Uuid(uuid::Uuid),
    VecUuid(Vec<uuid::Uuid>),
    VecI32(Vec<i32>),
    I32(i32),
    F32(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    String(String),
    Transform(Transform),
    VecSDFObject(Vec<SDFObject>),
    SDFObjectTree(SDFObjectTree),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue>)),
    #[serde(skip)]
    VecUpdate(Vec<Update<ClaydashValue>>),
    #[serde(skip)]
    VecSnapshot(Vec<Snapshot<ClaydashValue>>),
    EditorState(EditorState),
    Bool(bool),
    #[serde(skip)]
    Snapshot(Snapshot<ClaydashValue>),
    ControlPointType(ControlPointType),
    None,
}

impl Default for ClaydashValue {
    fn default() -> Self {
        ClaydashValue::None
    }
}

impl CanBeNone<ClaydashValue> for ClaydashValue {
    fn none() -> Self {
        ClaydashValue::None
    }
}

macro_rules! define_unwrap_methods {
    ($unwrap_method_name:ident, $unwrap_or_default_method_name:ident, $unwrap_or_method_name:ident, $variant:ident, $type:ty, $default: expr) => {
        pub fn $unwrap_method_name(&self) -> $type {
            match &self {
                Self::$variant(value) => *value,
                _ => {
                    panic!("No {} value stored.", stringify!($type));
                }
            }
        }

        pub fn $unwrap_or_default_method_name(&self) -> $type {
            match &self {
                Self::$variant(value) => *value,
                _ => $default
            }
        }

        pub fn $unwrap_or_method_name(&self, default_value: $type) -> $type {
            match &self {
                Self::$variant(value) => *value,
                _ => default_value
            }
        }
    };
}

macro_rules! define_unwrap_methods_for_vec {
    ($unwrap_method_name:ident, $unwrap_or_method_name:ident, $variant:ident, $type:ty) => {
        pub fn $unwrap_method_name(&self) -> &$type {
            match &self {
                Self::$variant(value) => value,
                _ => {
                    panic!("No {} value stored.", stringify!($type));
                }
            }
        }

        /// Warning: this method creates a new value.
        pub fn $unwrap_or_method_name(&self, default_value: $type) -> $type {
            match &self {
                Self::$variant(value) => value.clone(),
                _ => default_value
            }
        }
    };
}

macro_rules! define_unwrap_methods_for_object_with_boxes {
    ($unwrap_method_name:ident, $unwrap_or_default_method_name:ident, $unwrap_or_method_name:ident, $variant:ident, $type:ty, $default: expr) => {
        pub fn $unwrap_method_name(&self) -> &$type {
            match &self {
                Self::$variant(value) => value,
                _ => {
                    panic!("No {} value stored.", stringify!($type));
                }
            }
        }

        pub fn $unwrap_or_default_method_name(&self) -> $type {
            match &self {
                Self::$variant(value) => (*value).clone(),
                _ => $default
            }
        }

        /// Warning: this method creates a new value.
        pub fn $unwrap_or_method_name(&self, default_value: $type) -> $type {
            match &self {
                Self::$variant(value) => value.clone(),
                _ => default_value
            }
        }
    };
}



impl ClaydashValue {
    // Add a few methods to help with unwrapping.
    define_unwrap_methods!(
        unwrap_uuid,
        unwrap_uuid_or_default,
        unwrap_uuid_or,
        Uuid,
        uuid::Uuid,
        uuid::Uuid::default()
    );

    define_unwrap_methods!(
        unwrap_i32,
        unwrap_i32_or_default,
        unwrap_i32_or,
        I32,
        i32,
        0
    );

    define_unwrap_methods!(
        unwrap_f32,
        unwrap_f32_or_default,
        unwrap_f32_or,
        F32,
        f32,
        0.0
    );

    define_unwrap_methods!(
        unwrap_vec2,
        unwrap_vec2_or_default,
        unwrap_vec2_or,
        Vec2,
        Vec2,
        Vec2::default()
    );

    define_unwrap_methods!(
        unwrap_vec3,
        unwrap_vec3_or_default,
        unwrap_vec3_or,
        Vec3,
        Vec3,
        Vec3::default()
    );

    define_unwrap_methods!(
        unwrap_vec4,
        unwrap_vec4_or_default,
        unwrap_vec4_or,
        Vec4,
        Vec4,
        Vec4::default()
    );

    define_unwrap_methods!(
        unwrap_transform,
        unwrap_transform_or_default,
        unwrap_transform_or,
        Transform,
        Transform,
        Transform::default()
    );

    define_unwrap_methods!(
        unwrap_fn,
        unwrap_fn_or_default,
        unwrap_fn_or,
        Fn,
        fn(&mut ObservableKVTree<ClaydashValue>),
        panic!("No Fn value stored.")
    );

    define_unwrap_methods!(
        unwrap_bool,
        unwrap_bool_or_default,
        unwrap_bool_or,
        Bool,
        bool,
        false
    );

    define_unwrap_methods!(
        unwrap_control_point_type,
        unwrap_control_point_type_or_default,
        unwrap_control_point_type_or,
        ControlPointType,
        ControlPointType,
        ControlPointType::None
    );

    define_unwrap_methods_for_object_with_boxes!(
        sdf_object_tree,
        unwrap_sdf_object_tree_or_default,
        unwrap_sdf_object_tree_or,
        SDFObjectTree,
        SDFObjectTree,
        SDFObjectTree::default()
    );

    define_unwrap_methods_for_vec!(
        unwrap_editor_state,
        unwrap_editor_state_or,
        EditorState,
        EditorState
    );

    define_unwrap_methods_for_vec!(
        unwrap_vec_uuid,
        unwrap_vec_uuid_or,
        VecUuid,
        Vec<uuid::Uuid>
    );

    define_unwrap_methods_for_vec!(
        unwrap_vec_sdf_object,
        unwrap_vec_sdf_object_or,
        VecSDFObject,
        Vec<SDFObject>
    );

    define_unwrap_methods_for_vec!(
        unwrap_vec_update,
        unwrap_vec_update_or,
        VecUpdate,
        Vec<Update<ClaydashValue>>
    );

    define_unwrap_methods_for_vec!(
        unwrap_vec_snapshot,
        unwrap_vec_snapshot_or,
        VecSnapshot,
        Vec<Snapshot<ClaydashValue>>
    );

    define_unwrap_methods_for_vec!(
        unwrap_vec_i32,
        unwrap_vec_i32_or,
        VecI32,
        Vec<i32>
    );

    pub fn is_none(&self) -> bool {
        match &self {
            Self::None => true,
            _ => false,
        }
    }
}

#[derive(Resource, Default)]
pub struct ClaydashData {
    pub tree: ObservableKVTree<ClaydashValue>
}

pub struct ClaydashDataPlugin;

impl Plugin for ClaydashDataPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Update, sync_to_bevy);
    }
}

lazy_static! {
    static ref LAST_SYNCED_SDF_OBJECTS_VERSION: Arc<Mutex<i32>> = Arc::new(Mutex::new(-1));
}

pub fn get_active_object_index(tree: &ObservableKVTree<ClaydashValue>) -> Option<usize> {
    let objects = tree.get_path("scene.sdf_object_tree");
    let uuids = tree.get_path("scene.selected_uuids");
    let uuids = uuids.unwrap_vec_uuid_or(vec!());

    // Last selected object is the active object
    let sdf_object_tree = objects.unwrap_sdf_object_tree_or_default();
    for (index, object) in sdf_object_tree.get_vec_sdf_object().iter().enumerate().rev() {
        if uuids.contains(&object.uuid) {
            return Some(index);
        }
    }

    return None;
}

// Sync tree to bevy
// Once the tree supports different update flags, we can split this in separate systems again.
fn sync_to_bevy(
    mut data_resource: ResMut<ClaydashData>,
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let data = data_resource.as_mut();

    let version = data.tree.path_version("scene.sdf_object_tree");

    let last_updated_version = LAST_SYNCED_SDF_OBJECTS_VERSION.try_lock();

    let mut last_updated_version = match last_updated_version {
        Ok(version) => { version  }
        _ => { return }
    };

    if version > *last_updated_version  {
        // Potentially: move this block to sdf_object
        // Update sdf objects
        {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            material.sdf_meta[0].w = TYPE_END;

            let sdf_object_tree = data.tree.get_path("scene.sdf_object_tree").unwrap_sdf_object_tree_or_default();
            let sdf_operations = sdf_object_tree.get_vec_sdf_operation();

            let mut counter = 0;

            for operation in sdf_operations.iter() {
                println!("Operation");
                println!("rhs: {:?}", operation.rhs.uuid);
                println!("lhs: {:?}", operation.lhs.uuid);
                println!("rhs: {:?}", operation.rhs.object_type);
                println!("lhs: {:?}", operation.lhs.object_type);
                println!("counter: {:?}", counter);

                let lhs = &operation.lhs;
                let rhs = &operation.rhs;

                lhs.params.update_material(counter, material);
                material.sdf_meta[counter].w = lhs.object_type;
                material.sdf_colors[counter] = lhs.color;
                material.sdf_inverse_transforms[counter] = lhs.inverse_transform_matrix();


                rhs.params.update_material(counter + 1, material);
                material.sdf_meta[counter + 1].w = rhs.object_type;
                material.sdf_colors[counter + 1] = rhs.color;
                material.sdf_inverse_transforms[counter + 1] = rhs.inverse_transform_matrix();


                match operation.operation {
                    SDFOperation::Union => {
                        material.sdf_operations[counter].w = OPERATION_UNION;
                    },
                    SDFOperation::Exclusion => {
                        material.sdf_operations[counter].w = OPERATION_EXCLUSION;
                    },
                    SDFOperation::Intersection => {
                        material.sdf_operations[counter].w = OPERATION_INTERSECTION;
                    },
                    SDFOperation::UseLhsAsIs => {
                        material.sdf_operations[counter].w = OPERATION_USE_LHS_AS_IS;
                    },
                    SDFOperation::UseRhsAsIs => {
                        material.sdf_operations[counter].w = OPERATION_USE_RHS_AS_IS;
                    },
                }

                // todo: update index

                material.sdf_meta[counter + 2].w = TYPE_END;

                counter += 2;
            }
        }

        *last_updated_version = version;
    }

    if data.tree.was_path_updated("scene.selected_uuids") || data.tree.was_path_updated("scene.sdf_object_tree") {
        let active_object_index = get_active_object_index(&data.tree);
        let objects = data.tree.get_path("scene.sdf_object_tree").unwrap_sdf_object_tree_or_default();
        let uuids = data.tree.get_path("scene.selected_uuids");
        let uuids = uuids.unwrap_vec_uuid();

        // Reset in case no material is selected
        let handle = material_handle.single();
        let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
        material.num_control_points[0] = 0;

        for (index, object) in objects.get_vec_sdf_object().iter().enumerate() {
            if uuids.contains(&object.uuid) {
                // Mark as selected
                material.sdf_meta[index].x = 1;
            } else {
                // Mark as not-selected
                material.sdf_meta[index].x = 0;
            }
        }

        match active_object_index  {
            Some(index) => {
                // Show control points
                let object = &objects.get_vec_sdf_object()[index];
                show_control_points(material, index, object);
            },
            _ => {}
        }
    }


    data.tree.reset_update_cycle();
}

fn show_control_points(material: &mut SDFObjectMaterial, index: usize, object: &SDFObject) {
    let mut num_control_points: i32 = 0;

    object.params.update_material(index, material);

    for point in object.get_control_points().iter() {
        material.control_point_positions[num_control_points as usize].x = point.position.x;
        material.control_point_positions[num_control_points as usize].y = point.position.y;
        material.control_point_positions[num_control_points as usize].z = point.position.z;
        num_control_points += 1;
    }

    material.num_control_points[0] = num_control_points;
}
