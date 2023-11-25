use bevy::prelude::*;

use sdf_consts::*;

use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker,
};

use bevy_sdf_object::*;

#[derive(Clone)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
}

#[derive(Clone)]
pub enum ClaydashValue {
    UUIDList(Vec<uuid::Uuid>),
    F32(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Transform(Transform),
    VecSDFObject(Vec<SDFObject>),
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

impl ClaydashValue {
    pub fn get_vec4_or(&self, default_value: Vec4) -> Vec4 {
        match self {
            ClaydashValue::Vec4(value) => { return *value },
            _ => { return default_value }
        }
    }

    pub fn get_uuid_list_or_empty_vec(&self) -> Vec<uuid::Uuid> {
        match self {
            ClaydashValue::UUIDList(value) => { return value.to_vec(); },
            _ => { return vec!() }
        }
    }

    pub fn get_vec_sdf_objects_or_empty_vec(&self) -> Vec<SDFObject> {
        match self {
            ClaydashValue::VecSDFObject(value) => { return value.to_vec(); },
            _ => { return vec!() }
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
            .add_systems(Startup, init_sdf_objects)
            .add_systems(Update, sync_to_bevy);
    }
}

fn init_sdf_objects(mut data_resource: ResMut<ClaydashData>) {
    let data = data_resource.as_mut();

    let mut sdf_objects: Vec<SDFObject> = Vec::new();
    sdf_objects.push(SDFObject {
        object_type: TYPE_SPHERE,
        color: Vec4::new(0.3, 0.0, 0.6, 1.0),
        ..default()
    });

    sdf_objects.push(SDFObject {
        object_type: TYPE_BOX,
        transform: Transform::from_translation(Vec3::new(-0.2, 0.3, 0.0)),
        color: Vec4::new(0.8, 0.0, 0.6, 1.0),
        ..default()
    });

    data.tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
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

    if data.tree.was_path_updated("scene.sdf_objects") {
        // Potentially: move this block to bevy_sdf_object
        // Update sdf objects
        {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
            material.sdf_meta[0].w = TYPE_END;

            match data.tree.get_path("scene.sdf_objects").unwrap() {
                ClaydashValue::VecSDFObject(data) => {
                    for (index, object) in data.iter().enumerate() {
                        material.sdf_meta[index].w = object.object_type;
                        material.sdf_colors[index] = object.color;
                        material.sdf_inverse_transforms[index] = object.inverse_transform_matrix();
                        material.sdf_meta[index + 1].w = TYPE_END;
                    }
                },
                _ => { }
            }
        }
    }

    if data.tree.was_path_updated("scene.selected_uuids") {
        // Potentially: move this block to interactions.
        // Update selection state
        {
            let handle = material_handle.single();
            let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();

            let objects = match data.tree.get_path("scene.sdf_objects").unwrap() {
                ClaydashValue::VecSDFObject(data) => data,
                _ => { return; }
            };

            match data.tree.get_path("scene.selected_uuids").unwrap_or(ClaydashValue::None) {
                ClaydashValue::UUIDList(uuids) => {
                    for (index, object) in objects.iter().enumerate() {
                        if uuids.contains(&object.uuid) {
                            // Mark as selected
                            material.sdf_meta[index].x = 1;
                        } else {
                            // Mark as not-selected
                            material.sdf_meta[index].x = 0;
                        }
                    }
                },
                _ => { return; }
            }
        }
    }

    data.tree.reset_update_cycle();
}
