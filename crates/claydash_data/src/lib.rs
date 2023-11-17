use bevy::prelude::*;

use sdf_consts::*;

use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker,
};

use bevy_sdf_object::*;

#[derive(Clone)]
pub enum ClaydashValue {
    UUIDList(Vec<uuid::Uuid>),
    F32(f32),
    VecSDFObject(Vec<SDFObject>),
    None,
}

impl Default for ClaydashValue {
    fn default() -> Self {
        ClaydashValue::None
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
            .add_systems(Update, update_sdf_objects)
            .add_systems(Update, update_selected_objects);
    }
}

fn init_sdf_objects(mut data_resource: ResMut<ClaydashData>) {
    let data = data_resource.as_mut();

    let mut sdf_objects: Vec<SDFObject> = Vec::new();
    sdf_objects.push(SDFObject {
        object_type: TYPE_SPHERE,
        position: Vec3::new(0.0, 0.0, 0.0),
        color: Vec4::new(0.3, 0.0, 0.6, 1.0),
        ..default()
    });

    sdf_objects.push(SDFObject {
        object_type: TYPE_CUBE,
        position: Vec3::new(-0.2, 0.3, 0.0),
        color: Vec4::new(0.8, 0.0, 0.6, 1.0),
        ..default()
    });

    data.tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
}


/// Translate SDFObject in our data structure into material uniform parameters
/// That can be consumed by the GPU.
fn update_sdf_objects(
    mut data_resource: ResMut<ClaydashData>,
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let data = data_resource.as_mut();
    if data.tree.update_tracker.was_updated() {
        let handle = material_handle.single();
        let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();
        material.sdf_meta[0].w = TYPE_END;

        match data.tree.get_path("scene.sdf_objects").unwrap() {
            ClaydashValue::VecSDFObject(data) => {
                for (index, object) in data.iter().enumerate() {
                    material.sdf_meta[index].w = object.object_type;
                    material.sdf_positions[index] = Vec4 {
                        x: object.position.x,
                        y: object.position.y,
                        z: object.position.z,
                        w: 0.0
                    };
                    material.sdf_colors[index] = object.color;
                    material.sdf_meta[index + 1].w = TYPE_END;
                }
            },
            _ => { }
        }
        data.tree.reset_update_cycle();
    }
}


fn update_selected_objects(
    mut data_resource: ResMut<ClaydashData>,
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let data = data_resource.as_mut();
    if data.tree.update_tracker.was_updated() {
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
        data.tree.reset_update_cycle();
    }
}
