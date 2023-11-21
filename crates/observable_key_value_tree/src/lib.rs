//! ObservableKVTree is a a nested map data structure designed for applications
//! with update cycles (example: every frame, every network sync).
//!
//! A note about the "Observable" word, that usually comes with callback expectations:
//!  - It's **not** observable in the sense that a callback will be run on updates.
//!
//!  - It's obsevable in the sense that you can efficiently determine which part was changed as part of your application's main loop.
//!  - Although, potentially, with a custom update tracker, it could be possible to work with callbacks.
//!
//! The intended use is roughly as follows:
//!  - Update some properties in the tree. The node and it's parent will be marked as updated.
//!  - A the next frame, your code can parse the tree structure, skipping subtrees that have not
//!    been updated.
//!
//! Here are the important parts of the API:
//!  - `data.set_path("scene.some.property", 1234)`
//!  - `data.get_path("scene.some.property")`
//!  - `data.update_tracker.was_updated()`
//!  - `data.was_path_updated("scene.some.property")`
//!
//! # Examples
//!
//! Setting and reading values:
//! ```
//! use observable_key_value_tree::ObservableKVTree;
//! use observable_key_value_tree::SimpleUpdateTracker;
//! // Creating an observable tree
//! let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
//! // Setting values
//! data.set_path("scene.some.property", 1234);
//! // Reading values
//! let value = data.get_path("scene.some.property").unwrap();
//! ```
//!
//! Detecting changes:
//! ```
//! use observable_key_value_tree::ObservableKVTree;
//! use observable_key_value_tree::SimpleUpdateTracker;
//! // Creating an observable tree
//! let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
//! // Setting values
//! data.set_path("scene.some.property", 1234);
//! // Detecting updates
//! let was_updated: bool = data.was_path_updated("scene.some.property");
//! // Detecting updates (root level)
//! assert_eq!(data.was_updated(), true);
//! // Reset update cycle (typically, you'd call this every frame)
//! data.reset_update_cycle();
//! assert_eq!(data.was_updated(), false);
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
pub struct ObservableKVTree
    <ValueType: Default + Clone,
     UpdateTracker: Default + Clone+ NotifyUpdate>
{
    subtree: BTreeMap<String, ObservableKVTree<ValueType, UpdateTracker>>,
    value: Option<ValueType>,
    #[serde(skip)]
    pub update_tracker: UpdateTracker,
}

/// Shortcut to verify if a path was modified.
impl <ValueType: Default + Clone> ObservableKVTree<ValueType, SimpleUpdateTracker> {
    pub fn was_updated(&self) -> bool {
        return self.update_tracker.was_updated();
    }

    pub fn was_path_updated(&self, path: &str) -> bool {
        match self.get_path_meta(&path) {
            Some(value) => {
                return value.update_tracker.was_updated();
            },
            _ => { return false; }
        };
    }
}

impl <ValueType: Default + Clone,
      UpdateTracker: Default + NotifyUpdate + Clone>
    ObservableKVTree<ValueType, UpdateTracker>
{
    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), ObservableKVTree {
            value: Some(value),
            ..ObservableKVTree::default()
        });
    }

    pub fn get_path(&self, path: &str) -> Option<ValueType> {
        return match self.get_path_with_parts(&path.split(".").collect()) {
            Some(data) => data.value,
            _ => None
        }
    }

    pub fn get_path_meta(& self, path: &str) -> Option<ObservableKVTree<ValueType, UpdateTracker>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(&mut self, parts: Vec<&str>, value: ObservableKVTree<ValueType, UpdateTracker>) {
        if parts.len() == 1 {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableKVTree::default());
            }
            let leaf = &mut self.subtree.get_mut(parts[0]).unwrap();
            leaf.value = value.value;
            leaf.notify_change();
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableKVTree::default());
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
    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<ObservableKVTree<ValueType, UpdateTracker>> {
        if parts.len() == 1 {
            return self.subtree.get(parts[0]).cloned();
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                return None;
            }
            let subtree = &self.subtree.get(parts[0]).unwrap();
            let value = match subtree.get_path_with_parts(&parts[1..].to_vec()) {
                Some(value) => value,
                _ => { return None },
            };
            return Some(value);
        }
    }

    fn notify_change(&mut self) {
        self.update_tracker.notify_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_and_sets_values() {
        let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some", 1234);
        assert_eq!(data.get_path("scene.some").unwrap(), 1234);
    }

    #[test]
    fn it_gets_and_sets_deep_values() {
        let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 1234);
    }

    #[test]
    fn it_gets_none_when_not_set() {
        let data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
        assert_eq!(data.get_path("scene.property.that.does.not.exist"), None);
    }

    #[test]
    fn it_changes_value() {
        let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();
        data.set_path("scene.some.very.deep.property", 1234);
        data.set_path("scene.some.very.deep.property", 2345);
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap(), 2345);
    }

    #[test]
    fn it_detects_updates() {
        // Arrange
        let mut data = ObservableKVTree::<i32, SimpleUpdateTracker>::default();

        // Pre condition

        // Set value
        data.set_path("scene.some.very.deep.property", 1234);
        assert_eq!(data.was_path_updated("scene.some.very.deep.property"), true);
        assert_eq!(data.was_path_updated("scene.some.very.deep"), true);
        assert_eq!(data.was_path_updated("scene.some.very"), true);
        assert_eq!(data.was_path_updated("scene.some"), true);
        assert_eq!(data.was_path_updated("scene"), true);
        assert_eq!(data.update_tracker.updated, true);

        // Reset update cycle
        data.reset_update_cycle();
        assert_eq!(data.was_path_updated("scene.some.very.deep.property"), false);
        assert_eq!(data.was_path_updated("scene.some.very.deep"), false);
        assert_eq!(data.was_path_updated("scene.some.very"), false);
        assert_eq!(data.was_path_updated("scene.some"), false);
        assert_eq!(data.was_path_updated("scene"), false);
        assert_eq!(data.update_tracker.updated, false);

        // Set value (2nd time)
        data.set_path("scene.some.very.deep.property", 2345);

        assert_eq!(data.was_path_updated("scene.some.very.deep.property"), true);
        assert_eq!(data.was_path_updated("scene.some.very.deep"), true);
        assert_eq!(data.was_path_updated("scene.some.very"), true);
        assert_eq!(data.was_path_updated("scene.some"), true);
        assert_eq!(data.was_path_updated("scene"), true);
        assert_eq!(data.update_tracker.updated, true);
    }

    #[test]
    fn it_serializes() {
        let mut data = ObservableKVTree::<f32, SimpleUpdateTracker>::default();

        data.set_path("scene.some.deep.property", 123.4);

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        // Convert JSON back to BevySceneData
        let deserialized: ObservableKVTree<f32, SimpleUpdateTracker> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_path("scene.some.deep.property").unwrap(), 123.4);
    }
}
