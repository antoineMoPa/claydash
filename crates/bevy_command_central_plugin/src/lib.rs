use bevy::prelude::*;
use command_central::CommandMap;

use claydash_data::ClaydashValue;

pub struct BevyCommandCentralPlugin;

#[derive(Resource, Default)]
pub struct CommandCentralState {
    pub commands: CommandMap<ClaydashValue>,
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
