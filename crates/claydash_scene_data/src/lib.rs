use std::collections::BTreeMap;

use bevy::prelude::*;

use serde::{Serialize, Deserialize};

#[derive(Default,Serialize,Deserialize,Debug,Clone)]
pub struct SceneDataValue<ValueType: Default + Clone> {
    subtree: Option<ClaydashSceneData<ValueType>>,
    value: Option<ValueType>,
    version: i32,
}

#[derive(Default,Serialize,Deserialize,Debug,Clone)]
pub struct ClaydashSceneData<ValueType: Default + Clone> {
    map: BTreeMap<String, SceneDataValue<ValueType>>,
    version: i32,
}

impl<ValueType: Default + Clone> ClaydashSceneData<ValueType> {
    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), SceneDataValue {
            value: Some(value),
            ..default()
        });
    }

    pub fn get_path(& self, path: &str) -> Option<SceneDataValue<ValueType>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(&mut self, parts: Vec<&str>, value: SceneDataValue<ValueType>) {
        if parts.len() == 1 {
            self.map.insert(parts[0].to_string(), value);
        }
        else {
            if !self.map.contains_key(parts[0]) {
                self.map.insert(parts[0].to_string(), SceneDataValue::<ValueType> { ..default() });
            }
            if self.map[parts[0]].subtree.is_none() {
                println!("creating tree at {}", parts[0]);
                let subtree = ClaydashSceneData {
                    ..default()
                };
                self.map.get_mut(parts[0]).unwrap().subtree = Some(subtree);
            };
            let subtree = self.map.get_mut(parts[0]).unwrap().subtree.as_mut().unwrap();
            subtree.set_path_with_parts(parts[1..].to_vec(), value);
        }
    }

    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<SceneDataValue<ValueType>> {
        if parts.len() == 1 {
            return self.map.get(parts[0]).cloned();
        }
        else {
            if self.map[parts[0]].subtree.is_none() {
                // TODO: return None
                panic!("Value does not exist at {}", parts.join("."));
            };
            let subtree = &self.map.get(parts[0]).unwrap().subtree.as_ref().unwrap();
            let value = subtree.get_path_with_parts(&parts[1..].to_vec()).unwrap();
            return Some(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_and_sets_values() {
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };
        data.set_path("scene.some", 1234);
        assert_eq!(data.get_path("scene.some").unwrap().value.unwrap(), 1234);
    }


    #[test]
    fn it_gets_and_sets_deep_values() {
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap().value.unwrap(), 1234);
    }

    #[test]
    fn it_serializes() {
        let mut data = ClaydashSceneData::<f32> {
            ..default()
        };

        data.set_path("scene.some", 123.4);

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        // Convert JSON back to BevySceneData
        let deserialized: ClaydashSceneData<f32> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_path("scene.some").unwrap().value.unwrap(), 123.4);
    }
}
