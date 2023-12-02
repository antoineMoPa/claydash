use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use sdf_consts::*;

use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker, CanBeNone,
};

use bevy_sdf_object::*;

use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    VecUuid(Vec<uuid::Uuid>),
    I32(i32),
    F32(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    String(String),
    Transform(Transform),
    VecSDFObject(Vec<SDFObject>),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>)),
    EditorState(EditorState),
    Bool(bool),
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



impl ClaydashValue {
    // Add a few methods to help with unwrapping.
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
        fn(&mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>),
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

    pub fn is_none(&self) -> bool {
        match &self {
            Self::None => true,
            _ => false,
        }
    }
}

#[derive(Resource, Default)]
pub struct ClaydashData {
    pub tree: ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
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

// Sync tree to bevy
// Once the tree supports different update flags, we can split this in separate systems again.
// Q: Why is this not in bevy_sdf_object?
// R: Because bevy_sdf_object should not depend on the tree
fn sync_to_bevy(
    mut data_resource: ResMut<ClaydashData>,
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let data = data_resource.as_mut();

    let version = data.tree.path_version("scene.sdf_objects");

    let last_updated_version = LAST_SYNCED_SDF_OBJECTS_VERSION.try_lock();

    let mut last_updated_version = match last_updated_version {
        Ok(version) => { version  }
        _ => { return }
    };

    if version > *last_updated_version  {
        // Potentially: move this block to bevy_sdf_object
        // Update sdf objects
        {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            material.sdf_meta[0].w = TYPE_END;

            let value = data.tree.get_path("scene.sdf_objects");
            for (index, object) in value.unwrap_vec_sdf_object().iter().enumerate() {
                material.sdf_meta[index].w = object.object_type;
                material.sdf_colors[index] = object.color;
                material.sdf_inverse_transforms[index] = object.inverse_transform_matrix();
                material.sdf_meta[index + 1].w = TYPE_END;
            }
        }

        *last_updated_version = version;
    }

    if data.tree.was_path_updated("scene.selected_uuids") {
        // Potentially: move this block to interactions.
        // Update selection state
        {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();

            let objects = data.tree.get_path("scene.sdf_objects");

            let uuids = data.tree.get_path("scene.selected_uuids");
            let uuids = uuids.unwrap_vec_uuid();

            for (index, object) in objects.unwrap_vec_sdf_object().iter().enumerate() {
                if uuids.contains(&object.uuid) {
                    // Mark as selected
                    material.sdf_meta[index].x = 1;
                } else {
                    // Mark as not-selected
                    material.sdf_meta[index].x = 0;
                }
            }
        }
    }

    data.tree.reset_update_cycle();
}
