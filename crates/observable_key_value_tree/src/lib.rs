//! ObservableKVTree is a a nested map data structure designed for applications
//! with update cycles (example: every frame, every network sync) It supports making snapshots and
//! basic undo/redo.
//!
//! It's basically built to be an application's fundamental data storage.
//!
//! You can efficiently determine which part was changed as part of your application's main loop or using mspc channels.
//!
//! When using was_updated, the intended use is roughly as follows:
//!  - Update some properties in the tree. The node and it's parent will be marked as updated.
//!  - At the next frame, your code can parse the tree structure, skipping subtrees that have not
//!    been updated.
//! For async processing, it becomes useful to use channels instead.
//!
//! Here are the important parts of the API:
//!  - `data.set_path("scene.some.property", 1234)`
//!  - `data.get_path("scene.some.property")`
//!  - `data.update_tracker.was_updated()`
//!  - `data.was_path_updated("scene.some.property")`
//!  - `data.create_update_channel()`
//!  - `data.make_undo_redo_snapshot()`
//!  - `data.undo()`
//!  - `data.redo()`
//!
//! # Examples
//!
//! ## Setting and reading values:
//!
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
//! ## Detecting changes with was_updated:
//!
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
//!
//! ## Detecting changes with mspc channel:
//!
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
//! ## Undo/Redo
//!
//! ```
//! use observable_key_value_tree::{ObservableKVTree,ExampleValueType};
//! let mut data = ObservableKVTree::<ExampleValueType>::default();
//!
//! data.set_path("some.property", ExampleValueType::from(123.4));
//! data.make_undo_redo_snapshot();
//! data.set_path("some.property", ExampleValueType::from(100.0));
//! data.set_path("some.property", ExampleValueType::from(101.0));
//! data.set_path("some.property", ExampleValueType::from(102.0));
//! data.make_undo_redo_snapshot();
//!
//! data.undo();
//! assert_eq!(data.get_path("some.property").unwrap_f32(), 123.4);
//! data.redo();
//! assert_eq!(data.get_path("some.property").unwrap_f32(), 102.0);
//! ```
//!
//! # Notes
//!  - We consider a value updated even if it was set to the same value again.
//!  - We consider the parent nodes as updated if a child value was updated.
//!  - Nodes can contain a value and a sub tree at the same time.
//!
//! # Current drawbacks:
//!  - Network sync not implemented/tested
//!  - It's not clear if undo/redo will work well if other sources of updates appear,
//!    such as  network sync. Currently, it works well locally for one user.
//!  - Not so appropriate for graph structure
//!  - No granular updates for arrays

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Default, Clone)]
pub struct Update<ValueType> {
    pub path: String,
    pub value: ValueType,
    pub old_value: ValueType,
}

#[derive(Default, Debug, Clone)]
pub struct Snapshot<ValueType> {
    new_values: BTreeMap<String, ValueType>,
    old_values: BTreeMap<String, ValueType>,
    version: i32,
}

impl<ValueType> Snapshot<ValueType> {
    fn clear(&mut self) {
        self.new_values.clear();
        self.old_values.clear();
        self.version = i32::default();
    }

    fn is_empty(&self) -> bool {
        self.new_values.is_empty()
    }
}

#[derive(Default, Clone, Debug)]
pub struct LeafVersionTracker {
    updated: bool,
    version: i32,
    pub corresponding_previous_version: Option<i32>,
}

/// Provides the leaf version numbering and 'was_updated' flag.
impl LeafVersionTracker {
    pub fn was_updated(&self) -> bool {
        self.updated
    }
    pub fn version(&self) -> i32 {
        self.version
    }

    fn notify_update(&mut self) {
        self.updated = true;
        self.version += 1;
    }

    fn reset_update_cycle(&mut self) {
        self.updated = false;
    }

    fn clear(&mut self) {
        self.updated = bool::default();
        self.version = i32::default();
        self.corresponding_previous_version = None;
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct ObservableKVTree<ValueType: Default + Clone + CanBeNone<ValueType>> {
    subtree: BTreeMap<String, ObservableKVTree<ValueType>>,
    value: ValueType,
    #[serde(skip)]
    pub update_tracker: LeafVersionTracker,
    #[serde(skip)]
    update_listeners: Vec<Sender<Update<ValueType>>>,
    /// Maps snapshot versions to (old_value, new_value)
    #[serde(skip)]
    pub snapshots: Vec<Snapshot<ValueType>>,
    /// Map path to (old_value, new_value)
    #[serde(skip)]
    pub snapshot_change_accumulator: Snapshot<ValueType>,
    #[serde(skip)]
    pub last_snapshot_version: i32,
    #[serde(skip)]
    pub versions: Vec<i32>,
    #[serde(skip)]
    pub current_version_index: Option<i32>,
}

/// Shortcut to verify if a path was modified.
impl<ValueType: Default + Clone + CanBeNone<ValueType>> ObservableKVTree<ValueType> {
    pub fn was_updated(&self) -> bool {
        return self.update_tracker.was_updated();
    }

    pub fn was_path_updated(&self, path: &str) -> bool {
        match self.get_tree(&path) {
            Some(value) => {
                return value.update_tracker.was_updated();
            }
            _ => {
                return false;
            }
        };
    }

    pub fn path_version(&self, path: &str) -> i32 {
        match self.get_tree(&path) {
            Some(value) => {
                return value.update_tracker.version();
            }
            _ => {
                return -1;
            }
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

mod tree_operations;

// This is a simple value type for docs and testing.
// In real applications, we expect that a more complex value type will be used
// to store whatever is needed depending on the context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExampleValueType {
    I32(i32),
    F32(f32),
    None,
}

impl From<i32> for ExampleValueType {
    fn from(value: i32) -> Self {
        return Self::I32(value);
    }
}

impl From<f32> for ExampleValueType {
    fn from(value: f32) -> Self {
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
            _ => {
                panic!("No i32 value stored.")
            }
        }
    }

    pub fn unwrap_f32(&self) -> f32 {
        match &self {
            Self::F32(value) => *value,
            _ => {
                panic!("No f32 value stored.")
            }
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
mod tests;
