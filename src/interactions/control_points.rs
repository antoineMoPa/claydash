use bevy::prelude::*;
use claydash_data::{ClaydashData, ClaydashValue};

pub fn init_control_points(mut data_resource: ResMut<ClaydashData>) {

}

pub fn update_control_points(mut data_resource: ResMut<ClaydashData>) {
    let tree = &mut data_resource.as_mut().tree;

}
