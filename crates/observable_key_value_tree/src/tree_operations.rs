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
        self.authored_version = self.authored_version.wrapping_add(1);
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

    /// Replaces persisted subtree data, removing absent keys.
    /// This load boundary resets this tree's entire history and pending edits;
    /// replacement is not undoable and does not emit channel notifications.
    pub fn set_tree(&mut self, path: &str, value: ObservableKVTree<ValueType>) {
        self.authored_version = self.authored_version.wrapping_add(1);
        let parts = path.split(".");
        self.set_path_with_parts(parts.collect(), value, true);
        self.reset_history();
    }

    /// Returns a cloned value, or the explicit none value for a missing path.
    pub fn get_path(&self, path: &str) -> ValueType {
        self.get_path_ref(path)
            .cloned()
            .unwrap_or_else(ValueType::none)
    }

    /// Borrows a value without cloning its containing tree.
    pub fn get_path_ref(&self, path: &str) -> Option<&ValueType> {
        self.get_tree_ref(path).map(|node| &node.value)
    }

    pub(super) fn get_tree_ref(&self, path: &str) -> Option<&Self> {
        let mut node = self;
        for part in path.split('.') {
            node = node.subtree.get(part)?;
        }
        Some(node)
    }

    /// Returns an owned copy of the subtree, including its runtime metadata.
    pub fn get_tree(&self, path: &str) -> Option<Self> {
        self.get_tree_ref(path).cloned()
    }

    fn set_path_with_parts(
        &mut self,
        parts: Vec<&str>,
        value: ObservableKVTree<ValueType>,
        override_subtree: bool,
    ) {
        let leaf = self.subtree.entry(parts[0].to_owned()).or_default();
        if parts.len() == 1 {
            if override_subtree {
                leaf.replace_subtree_values(value);
            } else {
                leaf.value = value.value;
                leaf.notify_change();
            }
        } else {
            leaf.set_path_with_parts(parts[1..].to_vec(), value, override_subtree);
        }
        self.notify_change();
    }

    // Copy only persisted data; imported listeners and undo history belong to
    // the source tree. Retained nodes keep their local version counters.
    fn replace_subtree_values(&mut self, value: Self) {
        self.reset_history();
        self.value = value.value;
        self.subtree
            .retain(|key, _| value.subtree.contains_key(key));
        for (key, child) in value.subtree {
            self.subtree
                .entry(key)
                .or_default()
                .replace_subtree_values(child);
        }
        self.notify_change();
    }

    /// Resets data, listeners and history. Authored revision still advances.
    pub fn clear(&mut self) {
        self.authored_version = self.authored_version.wrapping_add(1);
        self.subtree.clear();
        self.value = ValueType::none();
        self.update_tracker.clear();
        self.update_listeners.clear();
        self.reset_history();
    }

    fn reset_history(&mut self) {
        self.update_tracker.corresponding_previous_version = None;
        self.snapshot_change_accumulator.clear();
        self.snapshots.clear();
        self.last_snapshot_version = i32::default();
        self.versions.clear();
        self.current_version_index = None;
    }

    ///  ---------------------  SNAPSHOT MANAGEMENT  ---------------------

    pub fn make_snapshot(&mut self) -> i32 {
        if self.snapshot_change_accumulator.is_empty() {
            if let Some(version) = self.update_tracker.corresponding_previous_version {
                return version;
            }
        }
        // Snapshots are deltas along one timeline. A new branch must remove
        // abandoned deltas as well as their Undo/Redo version references.
        if let Some(current) = self.update_tracker.corresponding_previous_version {
            if let Some(position) = self.snapshots.iter().position(|s| s.version == current) {
                self.snapshots.truncate(position + 1);
                // Snapshot versions increase along the retained timeline.
                self.versions.retain(|version| *version <= current);
                self.current_version_index = self.versions.len().checked_sub(1).map(|i| i as i32);
            }
        }
        let version = self.update_tracker.version;
        if self
            .snapshots
            .last()
            .is_some_and(|snapshot| snapshot.version == version)
        {
            return version;
        }
        self.snapshots.push(Snapshot {
            version,
            old_values: self.snapshot_change_accumulator.old_values.clone(),
            new_values: self.snapshot_change_accumulator.new_values.clone(),
        });
        self.snapshot_change_accumulator.clear();
        self.last_snapshot_version = version;
        // The materialized tree now corresponds to this newly captured state.
        self.update_tracker.corresponding_previous_version = Some(version);
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

    /// Restores a captured state, discarding pending authored changes.
    /// Transient values on paths untouched by replay remain unchanged.
    pub fn go_to_snapshot_with_version(&mut self, version: i32) {
        let target = self
            .snapshots
            .iter()
            .position(|s| s.version == version)
            .expect("snapshot with this version does not exist");
        let current = self
            .update_tracker
            .corresponding_previous_version
            .and_then(|v| self.snapshots.iter().position(|s| s.version == v))
            .expect("tree has no current snapshot");

        let pending = self.snapshot_change_accumulator.clone();
        self.revert_snapshot(&pending);
        if target < current {
            for i in (target + 1..=current).rev() {
                self.revert_snapshot(&self.snapshots[i].clone());
            }
        } else {
            for i in current + 1..=target {
                self.apply_snapshot(&self.snapshots[i].clone());
            }
        }
        self.snapshot_change_accumulator.clear();
        self.update_tracker.corresponding_previous_version = Some(version);
        self.current_version_index = self
            .versions
            .iter()
            .position(|v| *v == version)
            .map(|i| i as i32);
    }

    pub fn rewind_to_version(&mut self, version: i32) {
        self.go_to_snapshot_with_version(version);
    }

    pub fn fast_forward_to_version(&mut self, version: i32) {
        self.go_to_snapshot_with_version(version);
    }

    /// Applies a snapshot delta as ordinary authored edits.
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
        if !self
            .snapshot_change_accumulator
            .old_values
            .contains_key(path)
        {
            let old_value = self.get_path(path);
            self.snapshot_change_accumulator
                .old_values
                .insert(path.to_owned(), old_value);
        }
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
