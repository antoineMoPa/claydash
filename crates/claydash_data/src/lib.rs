use bevy::prelude::*;

use sdf_consts::*;

use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker,
};

use bevy_sdf_object::*;

#[derive(Clone)]
pub enum ClaydashValue {
    F32(f32),
    VecSDFObject(Vec<SDFObject>)
}

impl Default for ClaydashValue {
    fn default() -> Self {
        ClaydashValue::F32(0.0)
    }
}

#[derive(Resource, Default)]
pub struct ClaydashData {
    tree: ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
}

pub struct ClaydashDataPlugin;

impl Plugin for ClaydashDataPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashData>()
            .add_systems(Startup, init_sdf_objects)
            .add_systems(Update, update_sdf_objects);
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

    data.tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
}

fn update_sdf_objects(
    mut data_resource: ResMut<ClaydashData>,
    material_handle: Query<&Handle<SDFObjectMaterial>>,
    mut materials: ResMut<Assets<SDFObjectMaterial>>,
) {
    let data = data_resource.as_mut();
    if data.tree.update_tracker.was_updated() {

        match data.tree.get_path("scene.sdf_objects").unwrap() {
            ClaydashValue::VecSDFObject(data) => {
                println!("updated!");

                let handle = material_handle.single();
                let material: &mut SDFObjectMaterial = materials.get_mut(handle).unwrap();

                for (index, object) in data.iter().enumerate() {
                    material.sdf_types[index].w = object.object_type;
                    material.sdf_positions[index] = Vec4 {
                        x: object.position.x,
                        y: object.position.y,
                        z: object.position.z,
                        w: 0.0
                    };
                    material.sdf_colors[index] = object.color;
                    material.sdf_types[index + 1].w = TYPE_END;
                }
            },
            _ => { }
        }
        data.tree.reset_update_cycle();
    }
}
