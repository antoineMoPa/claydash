//! ObservableTree is a a nested map data structure designed for applications
//! with update cycles (example: every frame, every network sync).
//!
//! It's **not** observable in the sense that a callback will be run on updates.
//!
//! It works roughly as follows:
//!  - Update some properties in the tree. The node and it's parent will be marked as updated.
//!  - A the next frame, your code can parse the tree structure, skipping subtrees that have not
//!    been updated.
//!
//! # Examples
//!
//! Setting and reading values:
//! ```
//! // Creating an observable tree
//! let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();
//! // Setting values
//! data.set_path("scene.some.property", 1234);
//! // Reading values
//! let value = data.get_path("scene.some.property").unwrap();
//! ```
//!
//! Detecting changes:
//! ```
//! // Detecting updates
//! let was_updated: bool = data.get_path_meta("scene.some.property").unwrap().update_tracker.was_updated()
//! // Detecting updates (root level)
//! assert_eq!(data.update_tracker.updated, true);
//! // Reset update cycle (typically, you'd call this every frame)
//! data.reset_update_cycle();
//! ```
//!
//! ## Customizing Update Tracker
//! You can build your own UpdateTracker as long as it implements NotifyUpdate trait.
//! This could be useful if you application has multiple update cycles.
//!
//! See SimpleUpdateTracker for a reference implementation.
//!
//! ## Notes
//!  - We consider a value updated even if it was set to the same value again.
//!  - We consider the parent nodes as updated if a child value was updated.
//!  - Nodes can contain a value and a sub tree at the same time.
//!

use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

pub trait NotifyUpdate {
    fn notify_update(&mut self);
    fn reset_update_cycle(&mut self);
}

#[derive(Default,Clone,Serialize,Deserialize)]
pub struct SimpleUpdateTracker {
    updated: bool,
}

impl SimpleUpdateTracker {
    pub fn was_updated(&self) -> bool { self.updated }
}

impl NotifyUpdate for SimpleUpdateTracker {
    fn notify_update(&mut self) {
        self.updated = true;
    }

    fn reset_update_cycle(&mut self) {
        self.updated = false;
    }
}

#[derive(Default,Serialize,Deserialize,Debug,Clone)]
pub struct ObservableTree
    <ValueType: Default + Clone,
     UpdateTracker: Default + Clone+ NotifyUpdate>
{
    subtree: BTreeMap<String, ObservableTree<ValueType, UpdateTracker>>,
    value: Option<ValueType>,
    version: i32,
    #[serde(skip)]
    update_tracker: UpdateTracker,
}

impl <ValueType: Default + Clone,
      UpdateTracker: Default + NotifyUpdate + Clone>
    ObservableTree<ValueType, UpdateTracker>
{
    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), ObservableTree {
            value: Some(value),
            ..ObservableTree::default()
        });
    }

    pub fn get_path(&self, path: &str) -> Option<ValueType> {
        return match self.get_path_with_parts(&path.split(".").collect()) {
            Some(data) => data.value,
            _ => None
        }
    }

    pub fn get_path_meta(& self, path: &str) -> Option<ObservableTree<ValueType, UpdateTracker>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(&mut self, parts: Vec<&str>, value: ObservableTree<ValueType, UpdateTracker>) {
        if parts.len() == 1 {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableTree::default());
            }
            let leaf = &mut self.subtree.get_mut(parts[0]).unwrap();
            leaf.value = value.value;
            leaf.notify_change();
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableTree::default());
            }
            let subtree = &mut self.subtree.get_mut(parts[0]).unwrap();
            subtree.set_path_with_parts(parts[1..].to_vec(), value);
        }

        self.notify_change();
    }

    pub fn reset_update_cycle(&mut self) {
        self.update_tracker.reset_update_cycle();
        for (_, node) in self.subtree.iter_mut() {
            node.reset_update_cycle();
        }
    }
    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<ObservableTree<ValueType, UpdateTracker>> {
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
        self.update_tracker.notify_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_and_sets_values() {
        let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some", 1234);
        assert_eq!(data.get_path("scene.some").unwrap(), 1234);
    }

    #[test]
    fn it_gets_and_sets_deep_values() {
        let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 1234);
    }

    #[test]
    fn it_gets_none_when_not_set() {
        let data = ObservableTree::<i32, SimpleUpdateTracker>::default();
        assert_eq!(data.get_path("scene.property.that.does.not.exist"), None);
    }

    #[test]
    fn it_changes_value() {
        let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some.very.deep.property", 1234);
        data.set_path("scene.some.very.deep.property", 2345);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 2345);
    }

    #[test]
    fn it_increments_version_number_on_change() {
        // Arrange
        let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();

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
        let mut data = ObservableTree::<i32, SimpleUpdateTracker>::default();

        // Pre condition

        // Set value
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.update_tracker.updated, true);

        // Reset update cycle
        data.reset_update_cycle();
        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().update_tracker.was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().update_tracker.was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().update_tracker.was_updated(), false);
        assert_eq!(data.get_path_meta("scene.some").unwrap().update_tracker.was_updated(), false);
        assert_eq!(data.get_path_meta("scene").unwrap().update_tracker.was_updated(), false);
        assert_eq!(data.update_tracker.updated, false);

        // Set value (2nd time)
        data.set_path("scene.some.very.deep.property", 2345);

        assert_eq!(data.get_path_meta("scene.some.very.deep.property").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very.deep").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some.very").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene.some").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.get_path_meta("scene").unwrap().update_tracker.was_updated(), true);
        assert_eq!(data.update_tracker.updated, true);
    }

    #[test]
    fn it_serializes() {
        let mut data = ObservableTree::<f32, SimpleUpdateTracker>::default();

        data.set_path("scene.some.deep.property", 123.4);

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        // Convert JSON back to BevySceneData
        let deserialized: ObservableTree<f32, SimpleUpdateTracker> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_path("scene.some.deep.property").unwrap(), 123.4);
    }
}
