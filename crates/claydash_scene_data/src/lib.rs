use bevy::prelude::*;

use serde::{Serialize, Deserialize};
use serde_json;

#[derive(Default,Serialize,Deserialize,Debug)]
pub struct ClaydashSceneData {
    pub sdf_types: Vec<IVec4>,
    pub sdf_positions: Vec<IVec4>,
    pub sdf_colors: Vec<IVec4>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_serializes() {
        let mut data = ClaydashSceneData {
            ..default()
        };

        data.sdf_colors.push(IVec4 { x: 1234, ..default() });

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        println!("serialized = {}", serialized);

        // Convert JSON back to BevySceneData
        let deserialized: ClaydashSceneData = serde_json::from_str(&serialized).unwrap();

        println!("deserialized = {:?}", deserialized);
        assert_eq!(deserialized.sdf_colors[0].x, 1234);
    }
}
