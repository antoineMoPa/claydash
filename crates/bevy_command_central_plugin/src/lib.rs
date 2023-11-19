use bevy::prelude::*;
use command_central::CommandMap;
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};

use claydash_data::ClaydashValue;

/// I don't think bevy supports generic plugins, so we have to create a param type
/// that is as useful as possible in the context of 3d apps.
/// Most importantly, it should be able to contain floats and vectors.
/// Ideally, we find a way to make the Plugin generic.
#[derive(Default, Clone, Copy)]
pub struct ParamType {
    pub f32_value: Option<f32>,
    pub vec3_value: Option<Vec3>,
    pub vec4_value: Option<Vec4>,
}

pub struct BevyCommandCentralPlugin;

#[derive(Resource, Default)]
pub struct CommandCentralState {
    pub commands: CommandMap<ParamType, ObservableKVTree<ClaydashValue, SimpleUpdateTracker>>,
}

impl Plugin for BevyCommandCentralPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandCentralState>()
            .add_systems(Update, run_commands);
    }
}

fn run_commands(
//    mut command_central_state: ResMut<CommandCentralState>
) {

}
