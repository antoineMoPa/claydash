use super::*;

impl<ValueType: Default + Clone + CanBeNone<ValueType>> ObservableKVTree<ValueType> {
    ///  ---------------------  GETTING/SETTING VALUES  ---------------------

    pub fn set_path(&mut self, path: &str, value: ValueType) {
        let old_value = self.get_path(path);

        self.set_path_without_notifying(path, value.clone());

        for listener in self.update_listeners.iter() {
            _ = listener.send(Update {
                path: path.to_string(),
                value: value.clone(),
                old_value: old_value.clone(),
            });
        }
    }

    /// This method is like set path, but it will not notify mspc channels.
    /// was_updated is still set, changes are still accumulated as part of snapshots.
    /// version numbers are still incremented.
    pub fn set_path_without_notifying(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.update_snapshot_accumulator(path, value.clone());
        self.set_path_with_parts(
            parts.collect(),
            ObservableKVTree {
                value,
                ..ObservableKVTree::default()
            },
            false,
        );
    }

    /// Set a value produced by a runtime system without adding it to Undo/Redo.
    /// Update flags, versions, and parent dirty state are still maintained.
    pub fn set_transient_path(&mut self, path: &str, value: ValueType) {
        let parts = path.split(".");
        self.set_path_with_parts(
            parts.collect(),
            ObservableKVTree {
                value,
                ..ObservableKVTree::default()
            },
            false,
        );
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
            _ => ValueType::none(),
        }
    }

    /// Borrows a value without cloning its containing tree.
    pub fn get_path_ref(&self, path: &str) -> Option<&ValueType> {
        let mut node = self;
        for part in path.split('.') {
            node = node.subtree.get(part)?;
        }
        Some(&node.value)
    }

    pub fn get_tree(&self, path: &str) -> Option<ObservableKVTree<ValueType>> {
        return self.get_path_with_parts(&path.split(".").collect());
    }

    fn set_path_with_parts(
        &mut self,
        parts: Vec<&str>,
        value: ObservableKVTree<ValueType>,
        override_subtree: bool,
    ) {
        if parts.len() == 1 {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree
                    .insert(parts[0].to_string(), ObservableKVTree::default());
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
                        let parts: Vec<&str> = vec![key];
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
        } else {
            if !self.subtree.contains_key(parts[0]) {
                self.subtree
                    .insert(parts[0].to_string(), ObservableKVTree::default());
            }
            let subtree = &mut self.subtree.get_mut(parts[0]).unwrap();
            subtree.set_path_with_parts(parts[1..].to_vec(), value, override_subtree);
        }

        self.notify_change();
    }

    fn get_path_with_parts(&self, parts: &Vec<&str>) -> Option<ObservableKVTree<ValueType>> {
        if parts.len() == 1 {
            return self.subtree.get(parts[0]).cloned();
        } else {
            if !self.subtree.contains_key(parts[0]) {
                return None;
            }
            let subtree = &self.subtree.get(parts[0]).unwrap();
            let value = match subtree.get_path_with_parts(&parts[1..].to_vec()) {
                Some(value) => value,
                _ => return None,
            };
            return Some(value);
        }
    }

    pub fn clear(&mut self) {
        self.subtree.clear();
        self.value = ValueType::none();
        self.update_tracker.clear();
        self.update_listeners.clear();
        self.snapshot_change_accumulator.clear();
        self.snapshots.clear();
        self.last_snapshot_version = i32::default();
        self.versions.clear();
        self.current_version_index = None;
    }

    ///  ---------------------  SNAPSHOT MANAGEMENT  ---------------------

    pub fn make_snapshot(&mut self) -> i32 {
        let version = self.update_tracker.version;
        self.snapshots.push(Snapshot {
            version,
            old_values: self.snapshot_change_accumulator.old_values.clone(),
            new_values: self.snapshot_change_accumulator.new_values.clone(),
        });
        self.snapshot_change_accumulator.clear();
        self.last_snapshot_version = version;
        return version;
    }

    pub fn last_snapshot_version(&mut self) -> Option<i32> {
        return match self.snapshots.last() {
            Some(snapshot) => Some(snapshot.version),
            _ => None,
        };
    }

    pub fn revert_snapshot_version(&mut self, version: i32) {
        let snapshot: Option<Snapshot<ValueType>> = self
            .snapshots
            .iter()
            .find(|snapshot| snapshot.version == version)
            .cloned();

        match snapshot {
            Some(snapshot) => {
                for (path, old_value) in snapshot.old_values.iter() {
                    self.set_path(path.as_str(), old_value.to_owned());
                }
            }
            None => {
                panic!("snapshot with this name does not exist");
            }
        }
    }

    pub fn go_to_snapshot_with_version(&mut self, version: i32) {
        let snapshot: Option<Snapshot<ValueType>> = self
            .snapshots
            .iter()
            .find(|snapshot| snapshot.version == version)
            .cloned();

        match snapshot {
            Some(snapshot) => {
                let current_version = match self.update_tracker.corresponding_previous_version {
                    Some(version) => version,
                    None => self.update_tracker.version,
                };

                if snapshot.version < current_version {
                    self.rewind_to_version(snapshot.version);
                }
                if snapshot.version > current_version {
                    self.fast_forward_to_version(snapshot.version);
                }
                self.snapshot_change_accumulator.clear();
            }
            None => {
                panic!("snapshot with this name does not exist");
            }
        }
    }

    pub fn rewind_to_version(&mut self, version: i32) {
        let current_version = match self.update_tracker.corresponding_previous_version {
            Some(version) => version,
            None => self.update_tracker.version,
        };
        let current_position = match self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.version == current_version)
        {
            Some(position) => position,
            None => {
                self.make_snapshot();
                self.snapshots.len() - 1
            }
        };

        let snapshot_position = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.version == version)
            .unwrap();
        let mut i = current_position;

        while i > snapshot_position {
            self.revert_snapshot(&self.snapshots[i].clone());
            i -= 1;
        }

        self.update_tracker.corresponding_previous_version = Some(version);
    }

    pub fn fast_forward_to_version(&mut self, version: i32) {
        let current_version = match self.update_tracker.corresponding_previous_version {
            Some(version) => version,
            None => self.update_tracker.version,
        };
        let current_position = match self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.version == current_version)
        {
            Some(position) => position,
            None => {
                //self.make_snapshot();
                self.snapshots.len() - 1
            }
        };

        let snapshot_position = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.version == version)
            .unwrap();
        let mut i = current_position;

        while i <= snapshot_position {
            self.apply_snapshot(&self.snapshots[i].clone());
            i += 1;
        }

        self.update_tracker.corresponding_previous_version = Some(version);
    }

    // Reverts a snapshot version and returns the reverted snapshot (if found)
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot<ValueType>) {
        for (path, new_value) in snapshot.new_values.iter() {
            self.set_path(path.as_str(), new_value.to_owned())
        }
    }

    pub fn revert_snapshot(&mut self, snapshot: &Snapshot<ValueType>) {
        for (path, old_value) in snapshot.old_values.iter() {
            self.set_path(path.as_str(), old_value.to_owned())
        }
    }

    // After setting a path, this method updates
    // the accumulator to set the old_value and the new_value
    fn update_snapshot_accumulator(&mut self, path: &str, value: ValueType) {
        let old_value: ValueType = self
            .snapshot_change_accumulator
            .old_values
            .get(path)
            .unwrap_or(&self.get_path(path))
            .clone();
        self.snapshot_change_accumulator
            .old_values
            .insert(path.to_owned(), old_value);
        self.snapshot_change_accumulator
            .new_values
            .insert(path.to_owned(), value);
    }

    ///  --------------------- UNDO/REDO ---------------------

    pub fn make_undo_redo_snapshot(&mut self) {
        if self.snapshot_change_accumulator.is_empty() {
            return;
        }
        let version = self.make_snapshot();

        // Slice, since after an action, we can't redo.
        let current_version_index = self
            .current_version_index
            .unwrap_or(self.versions.len() as i32 - 1);

        let new_len: i32 = current_version_index + 1;
        self.versions = self.versions[0..new_len as usize].to_vec();
        self.versions.push(version);

        let len = self.versions.len();
        self.current_version_index = Some(len as i32 - 1);
    }

    pub fn undo(&mut self) {
        // Inspector edits can span multiple UI frames. Commit their pending
        // transaction before navigating history so Undo always sees them.
        self.make_undo_redo_snapshot();
        let mut current_version_index = self.current_version_index.unwrap_or(0);

        if current_version_index == 0 {
            // nothing to undo
            return;
        }

        if self.versions.len() == 0 {
            // nothing to undo
            return;
        }

        current_version_index -= 1;

        let version = self.versions[current_version_index as usize];

        self.go_to_snapshot_with_version(version);

        self.current_version_index = Some(current_version_index);
    }

    pub fn redo(&mut self) {
        // A new pending edit creates a branch and invalidates forward history.
        self.make_undo_redo_snapshot();
        let mut current_version_index = self.current_version_index.unwrap_or(0);

        if current_version_index == self.versions.len() as i32 - 1 {
            // nothing to redo
            return;
        }

        if self.versions.len() == 0 {
            // nothing to redo
            return;
        }

        current_version_index += 1;

        let version = self.versions[current_version_index as usize];

        self.go_to_snapshot_with_version(version);

        // Make sure we keep same versions array after moving to a snapshot
        self.current_version_index = Some(current_version_index);
    }

    pub fn dump_undo_state(&mut self) {
        let versions = &self.versions;
        let current_version_index = self.current_version_index;

        for (index, version) in versions.iter().enumerate() {
            let arrow = if index == current_version_index.unwrap_or(-1) as usize {
                " <-"
            } else {
                ""
            };
            println!("{} {}", version, arrow);
        }
    }

    ///  --------------------- UPDATE NOTIFICATION MANAGEMENT ---------------------

    fn notify_change(&mut self) {
        self.update_tracker.notify_update();
    }

    pub fn create_update_channel(&mut self) -> Receiver<Update<ValueType>> {
        let (sender, receiver) = channel();
        self.update_listeners.push(sender);
        return receiver;
    }

    pub fn reset_update_cycle(&mut self) {
        self.update_tracker.reset_update_cycle();
        for (_, node) in self.subtree.iter_mut() {
            node.reset_update_cycle();
        }
    }
}
