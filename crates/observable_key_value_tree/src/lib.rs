//! ObservableKVTree is a a nested map data structure designed for applications
//! with update cycles (example: every frame, every network sync).
//!
//! You can efficiently determine which part was changed as part of your application's main loop or using mspc channels.
//!
//! When using was_updated, the intended use is roughly as follows:
//!  - Update some properties in the tree. The node and it's parent will be marked as updated.
//!  - A the next frame, your code can parse the tree structure, skipping subtrees that have not
//!    been updated.
//! For async processing, it becomes useful to use channels instead.
//!
//! Here are the important parts of the API:
//!  - `data.set_path("scene.some.property", 1234)`
//!  - `data.get_path("scene.some.property")`
//!  - `data.update_tracker.was_updated()`
//!  - `data.was_path_updated("scene.some.property")`
//!  - `data.create_update_channel()`
//!
//! # Examples
//!
//! Setting and reading values:
//! ```
//! use observable_key_value_tree::{ObservableKVTree,ExampleValueType};
//! // Creating an observable tree
//! let mut data = ObservableKVTree::<ExampleValueType>::default();
//! // Setting values
//! data.set_path("scene.some.property", ExampleValueType::from(1234));
//! // Reading values
//! let value = data.get_path("scene.some.property").unwrap_i32();
//! ```
//!
//! Detecting changes with was_updated:
//! ```
//! use observable_key_value_tree::{ObservableKVTree,ExampleValueType};
//! // Creating an observable tree
//! let mut data = ObservableKVTree::<ExampleValueType>::default();
//! // Setting values
//! data.set_path("scene.some.property", ExampleValueType::from(1234));
//! // Detecting updates
//! let was_updated: bool = data.was_path_updated("scene.some.property");
//! // Detecting updates (root level)
//! assert_eq!(data.was_updated(), true);
//! // Reset update cycle (typically, you'd call this every frame)
//! data.reset_update_cycle();
//! assert_eq!(data.was_updated(), false);
//! ```
//! Detecting changes with mspc channel:
//! ```
//! use observable_key_value_tree::{ObservableKVTree,ExampleValueType};
//! // Creating an observable tree
//! let mut data = ObservableKVTree::<ExampleValueType>::default();
//! let receiver = data.create_update_channel();
//! // Setting values
//! data.set_path("scene.some.property", ExampleValueType::from(1234));
//! // Detecting updates
//! let update = receiver.recv();
//! println!("{}", update.unwrap().path);
//! ```
//!
//! ## Notes
//!  - We consider a value updated even if it was set to the same value again.
//!  - We consider the parent nodes as updated if a child value was updated.
//!  - Nodes can contain a value and a sub tree at the same time.
//!

use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};
use std::sync::mpsc::{channel, Sender, Receiver};

#[derive(Default,Clone)]
pub struct Update<ValueType> {
    pub path: String,
    pub value: ValueType,
    pub old_value: ValueType,
}

#[derive(Default,Clone,Debug)]
pub struct LeafVersionTracker {
    updated: bool,
    version: i32,
}

/// Provides the leaf version numbering and 'was_updated' flag.
impl LeafVersionTracker {
    pub fn was_updated(&self) -> bool { self.updated }
    pub fn version(&self) -> i32 { self.version }

    fn notify_update(&mut self) {
        self.updated = true;
        self.version += 1;
    }

    fn reset_update_cycle(&mut self) {
        self.updated = false;
    }
}

#[derive(Default,Serialize,Deserialize,Debug,Clone)]
pub struct ObservableKVTree <ValueType: Default + Clone + CanBeNone<ValueType>>
{
    subtree: BTreeMap<String, ObservableKVTree<ValueType>>,
    value: ValueType,
    #[serde(skip)]
    pub update_tracker: LeafVersionTracker,
    #[serde(skip)]
    update_listeners: Vec<Sender<Update<ValueType>>>,
}

/// Shortcut to verify if a path was modified.
impl <ValueType: Default + Clone + CanBeNone<ValueType>> ObservableKVTree<ValueType> {
    pub fn was_updated(&self) -> bool {
        return self.update_tracker.was_updated();
    }

    pub fn was_path_updated(&self, path: &str) -> bool {
        match self.get_tree(&path) {
            Some(value) => {
                return value.update_tracker.was_updated();
            },
            _ => { return false; }
        };
    }

    pub fn path_version(&self, path: &str) -> i32 {
        match self.get_tree(&path) {
            Some(value) => {
                return value.update_tracker.version();
            },
            _ => { return -1; }
        };
    }
}

pub trait CanBeNone<T: Default> {
    fn none() -> T;
}

impl<T> CanBeNone<Option<T>> for Option<T> {
    fn none() -> Option<T> {
        None
    }
}

impl <ValueType: Default + Clone + CanBeNone<ValueType>> ObservableKVTree<ValueType>
{
    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        let old_value = self.get_path(path);

        self.set_path_with_parts(parts.collect(), ObservableKVTree {
            value: value.clone(),
            ..ObservableKVTree::default()
        }, false);

        for listener in self.update_listeners.iter() {
            _ = listener.send(Update{
                path: path.to_string(),
                value: value.clone(),
                old_value: old_value.clone(),
            });
        }
    }

    /// Set the whole subtree at given path
    /// This is useful to deserialize the tree.
    pub fn set_tree(&mut self, path: &str, value: ObservableKVTree<ValueType>) {
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), value, true);
        self.notify_change();
    }

    /// Get the whole subtree at given path
    /// This is useful to serialize the tree.
    pub fn get_path(&self, path: &str) -> ValueType {
        match self.get_path_with_parts(&path.split(".").collect()) {
            Some(data) => data.value,
            _ => ValueType::none()
        }
    }

    pub fn get_tree(& self, path: &str) -> Option<ObservableKVTree<ValueType>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(&mut self, parts: Vec<&str>, value: ObservableKVTree<ValueType>, override_subtree: bool) {

        if parts.len() == 1 {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableKVTree::default());
            }

            let mut notified_update = false;

            let leaf = &mut self.subtree.get_mut(parts[0]).unwrap();
            leaf.value = value.value;
            leaf.update_tracker.notify_update();

            if override_subtree {
                let mut keys_to_remove: Vec<String> = Vec::new();
                for (key, _subvalue) in leaf.subtree.iter() {
                    if !value.subtree.contains_key(key) {
                        // Value does not exist in new subtree. remove.
                        keys_to_remove.push(key.clone());
                    }
                }

                for key in keys_to_remove {
                    leaf.subtree.remove(&key);
                }

                for (key, subvalue) in value.subtree.iter() {
                    if !value.subtree.contains_key(key) {
                        leaf.subtree.insert(key.clone(), subvalue.clone());
                    } else {
                        let parts: Vec<&str> = vec!(key);
                        leaf.set_path_with_parts(parts, subvalue.clone(), override_subtree);
                        // Prevent a double update
                        notified_update = true;
                    }
                }

                if !notified_update {
                    leaf.update_tracker.notify_update();
                }

                return;
            }
        }
        else {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree.insert(parts[0].to_string(), ObservableKVTree::default());
            }
            let subtree = &mut self.subtree.get_mut(parts[0]).unwrap();
            subtree.set_path_with_parts(parts[1..].to_vec(), value, override_subtree);
        }

        self.notify_change();
    }

    pub fn reset_update_cycle(&mut self) {
        self.update_tracker.reset_update_cycle();
        for (_, node) in self.subtree.iter_mut() {
            node.reset_update_cycle();
        }
    }

    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<ObservableKVTree<ValueType>> {
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

    pub fn create_update_channel(&mut self) -> Receiver<Update<ValueType>> {
        let (sender, receiver) = channel();
        self.update_listeners.push(sender);
        return receiver;
    }
}


// This is a simple value type for docs and testing.
// In real applications, we expect that a more complex value type will be used
// to store whatever is needed depending on the context.
#[derive(Debug,Clone,Serialize,Deserialize)]
pub enum ExampleValueType{
    I32(i32),
    F32(f32),
    None,
}

impl From<i32> for ExampleValueType {
    fn from (value: i32) -> Self {
        return Self::I32(value);
    }
}

impl From<f32> for ExampleValueType {
    fn from (value: f32) -> Self {
        return Self::F32(value);
    }
}

impl CanBeNone<ExampleValueType> for ExampleValueType {
    fn none() -> ExampleValueType {
        return ExampleValueType::None;
    }
}

impl Default for ExampleValueType {
    fn default() -> Self {
        return Self::None;
    }
}

impl ExampleValueType {
    pub fn unwrap_i32(&self) -> i32 {
        match &self {
            Self::I32(value) => *value,
            _ => { panic!("No i32 value stored.") }
        }
    }

    pub fn unwrap_f32(&self) -> f32 {
        match &self {
            Self::F32(value) => *value,
            _ => { panic!("No f32 value stored.") }
        }
    }

    pub fn is_none(&self) -> bool {
        match &self {
            Self::None => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_and_sets_values() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some", ExampleValueType::I32(1234));
        assert_eq!(data.get_path("scene.some").unwrap_i32(), 1234);
    }

    #[test]
    fn it_gets_and_sets_deep_values() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap_i32(), 1234);
    }

    #[test]
    fn it_gets_and_sets_subtree() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));

        let scene = data.get_tree("scene").unwrap();
        let mut data2 = ObservableKVTree::<ExampleValueType>::default();
        data.reset_update_cycle();

        let initial_version = data2.path_version("scene");

        data2.set_tree("scene", scene.clone());

        assert!(data2.path_version("scene") > initial_version);
        assert_eq!(data2.get_path("scene.some.very.deep.property").unwrap_i32(), 1234);
        assert_eq!(data2.was_path_updated("scene.some.very.deep.property"), true);
        assert_eq!(data2.was_path_updated("scene.some.very.deep"), true);
        assert_eq!(data2.was_path_updated("scene.some.very"), true);
        assert_eq!(data2.was_path_updated("scene.some"), true);
        assert_eq!(data2.was_path_updated("scene"), true);
    }

    #[test]
    fn it_increments_version() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));
        data.set_path("scene.some.very.deep.property2", ExampleValueType::from(1234));

        let scene = data.get_tree("scene").unwrap();
        let mut data2 = ObservableKVTree::<ExampleValueType>::default();

        assert_eq!(data2.path_version("scene.some.very.deep.property"), -1);
        assert_eq!(data2.path_version("scene.some.very.deep"), -1);
        assert_eq!(data2.path_version("scene.some.very"), -1);
        assert_eq!(data2.path_version("scene.some"), -1);
        assert_eq!(data2.path_version("scene"), -1);

        data2.set_tree("scene", scene.clone());

        // TODO: I would expect this to start at 0
        // Not so important because at least versions are increasing.
        assert_eq!(data2.path_version("scene.some.very.deep.property"), 2);
        assert_eq!(data2.path_version("scene.some.very.deep"), 1);
        assert_eq!(data2.path_version("scene.some.very"), 1);
        assert_eq!(data2.path_version("scene.some"), 1);
        assert_eq!(data2.path_version("scene"), 1);

        assert_eq!(data2.get_path("scene.some.very.deep.property").unwrap_i32(), 1234);

        data2.set_path("scene.some.very.deep", ExampleValueType::I32(5555));

        assert_eq!(data2.path_version("scene.some.very.deep.property"), 2);
        assert_eq!(data2.path_version("scene.some.very.deep"), 2);
        assert_eq!(data2.path_version("scene.some.very"), 2);
        assert_eq!(data2.path_version("scene.some"), 2);
        assert_eq!(data2.path_version("scene"), 2);
    }

    #[test]
    fn it_sends_updates() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));

        let receiver = data.create_update_channel();

        data.set_path("scene.some.very.deep.property", ExampleValueType::from(2345));
        let update = receiver.recv().unwrap();
        assert_eq!(update.path, "scene.some.very.deep.property".to_string());
        assert_eq!(update.old_value.unwrap_i32(), 1234);
        assert_eq!(update.value.unwrap_i32(), 2345);

        data.set_path("scene.some.very.deep.property", ExampleValueType::from(3456));
        let update = receiver.recv().unwrap();
        assert_eq!(update.path, "scene.some.very.deep.property".to_string());
        assert_eq!(update.old_value.unwrap_i32(), 2345);
        assert_eq!(update.value.unwrap_i32(), 3456);
    }

    #[test]
    fn it_gets_none_when_not_set() {
        let data = ObservableKVTree::<ExampleValueType>::default();
        assert_eq!(data.get_path("scene.property.that.does.not.exist").is_none(), true);
    }

    #[test]
    fn it_changes_value() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(2345));
        assert_eq!(data.get_path("scene.some.very.deep.property").unwrap_i32(), 2345);
    }

    #[test]
    fn it_detects_updates() {
        // Arrange
        let mut data = ObservableKVTree::<ExampleValueType>::default();

        // Pre condition

        // Set value
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(1234));
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
        data.set_path("scene.some.very.deep.property", ExampleValueType::from(2345));

        assert_eq!(data.was_path_updated("scene.some.very.deep.property"), true);
        assert_eq!(data.was_path_updated("scene.some.very.deep"), true);
        assert_eq!(data.was_path_updated("scene.some.very"), true);
        assert_eq!(data.was_path_updated("scene.some"), true);
        assert_eq!(data.was_path_updated("scene"), true);
        assert_eq!(data.update_tracker.updated, true);
    }

    #[test]
    fn it_serializes() {
        let mut data = ObservableKVTree::<ExampleValueType>::default();

        data.set_path("scene.some.deep.property", ExampleValueType::from(123.4));

        // Convert BevySceneData to JSON
        let serialized = serde_json::to_string(&data).unwrap();

        // Convert JSON back to BevySceneData
        let deserialized: ObservableKVTree<ExampleValueType> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_path("scene.some.deep.property").unwrap_f32(), 123.4);
    }
}
