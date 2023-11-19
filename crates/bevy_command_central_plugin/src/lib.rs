use bevy::prelude::*;
use command_central::CommandMap;
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};

use claydash_data::ClaydashValue;

pub struct BevyCommandCentralPlugin;

#[derive(Resource, Default)]
pub struct CommandCentralState {
    pub commands: CommandMap<ClaydashValue, ObservableKVTree<ClaydashValue, SimpleUpdateTracker>>,
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
