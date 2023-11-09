use std::collections::BTreeMap;

use bevy::prelude::*;

use serde::{Serialize, Deserialize};

#[derive(Default,Serialize,Deserialize,Debug,Clone)]
pub struct ClaydashSceneData<ValueType: Default + Clone> {
    subtree: BTreeMap<String, ClaydashSceneData<ValueType>>,
    value: Option<ValueType>,
    version: i32,
    updated: bool,
}

impl<ValueType: Default + Clone> ClaydashSceneData<ValueType> {
    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), ClaydashSceneData {
            value: Some(value),
            ..default()
        });
    }

    pub fn get_path(&self, path: &str) -> Option<ValueType> {
        return match self.get_path_with_parts(&path.split(".").collect()) {
            Some(data) => data.value,
            _ => None
        }
    }

    pub fn get_path_meta(& self, path: &str) -> Option<ClaydashSceneData<ValueType>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(&mut self, parts: Vec<&str>, value: ClaydashSceneData<ValueType>) {
        if parts.len() == 1 {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ClaydashSceneData::<ValueType> {
                    ..default()
                });
            }
            let leaf = &mut self.subtree.get_mut(parts[0]).unwrap();
            leaf.value = value.value;
            leaf.notify_change();
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ClaydashSceneData::<ValueType> {
                    ..default()
                });
            }
            let subtree = &mut self.subtree.get_mut(parts[0]).unwrap();
            subtree.set_path_with_parts(parts[1..].to_vec(), value);
        }

        self.notify_change();
    }

    pub fn was_updated(&self) -> bool {
        return self.updated;
    }

    pub fn reset_update_cycle(&mut self) {
        self.updated = false;
        for (_, node) in self.subtree.iter_mut() {
            node.reset_update_cycle();
        }
    }

    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<ClaydashSceneData<ValueType>> {
        if parts.len() == 1 {
            return self.subtree.get(parts[0]).cloned();
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                return None;
            }
            let subtree = &self.subtree.get(parts[0]).unwrap();
            let value = subtree.get_path_with_parts(&parts[1..].to_vec()).unwrap();
            return Some(value);
        }
    }

    fn notify_change(&mut self) {
        self.version += 1;
        self.updated = true;
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
        assert_eq!(data.get_path("scene.some").unwrap(), 1234);
    }

    #[test]
    fn it_gets_and_sets_deep_values() {
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 1234);
    }

    #[test]
    fn it_gets_none_when_not_set() {
        let data = ClaydashSceneData::<i32> {
            ..default()
        };
        assert_eq!(data.get_path("scene.property.that.does.not.exist"), None);
    }

    #[test]
    fn it_changes_value() {
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };
        data.set_path("scene.some.very.deep.property", 1234);
        data.set_path("scene.some.very.deep.property", 2345);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 2345);
    }

    #[test]
    fn it_increments_version_number_on_change() {
        // Arrange
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };

        // Pre condition
        assert_eq!(data.version, 0);

        // Set value
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().version, 1);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().version, 1);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().version, 1);
        assert_eq!(data.get_path_meta("scene.some").unwrap().version, 1);
        assert_eq!(data.get_path_meta("scene").unwrap().version, 1);

        assert_eq!(data.version, 1);

        // Set value (2nd time)
        data.set_path("scene.some.very.deep.property", 2345);
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().version, 2);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().version, 2);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().version, 2);
        assert_eq!(data.get_path_meta("scene.some").unwrap().version, 2);
        assert_eq!(data.get_path_meta("scene").unwrap().version, 2);

        assert_eq!(data.version, 2);

        // Set value (3rd time)
        data.set_path("scene.some.very.deep.property", 3456);
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().version, 3);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().version, 3);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().version, 3);
        assert_eq!(data.get_path_meta("scene.some").unwrap().version, 3);
        assert_eq!(data.get_path_meta("scene").unwrap().version, 3);

        assert_eq!(data.version, 3);
    }

    #[test]
    fn it_detects_updates() {
        // Arrange
        let mut data = ClaydashSceneData::<i32> {
            ..default()
        };

        // Pre condition

        // Set value
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene").unwrap().was_updated(), true);
        assert_eq!(data.updated, true);

        // Reset update cycle
        data.reset_update_cycle();
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some").unwrap().was_updated(), false);
        assert_eq!(data.get_path_meta("scene").unwrap().was_updated(), false);
        assert_eq!(data.updated, false);

        // Set value (2nd time)
        data.set_path("scene.some.very.deep.property", 2345);

        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some").unwrap().was_updated(), true);
        assert_eq!(data.get_path_meta("scene").unwrap().was_updated(), true);
        assert_eq!(data.updated, true);
    }

    #[test]
    fn it_serializes() {
        let mut data = ClaydashSceneData::<f32> {
            ..default()
        };

        data.set_path("scene.some.deep.property", 123.4);

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        // Convert JSON back to BevySceneData
        let deserialized: ClaydashSceneData<f32> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_path("scene.some.deep.property").unwrap(), 123.4);
    }
}
