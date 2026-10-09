use super::*;

#[test]
fn authored_version_ignores_transient_updates() {
    let mut tree = ObservableKVTree::<ExampleValueType>::default();
    assert_eq!(tree.authored_version(), 0);
    tree.set_transient_path("scene.value", ExampleValueType::from(1));
    assert_eq!(tree.authored_version(), 0);
    tree.set_path("scene.value", ExampleValueType::from(2));
    assert_eq!(tree.authored_version(), 1);
}

#[test]
fn it_gets_and_sets_values() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.some", ExampleValueType::I32(1234));
    assert_eq!(data.get_path("scene.some").unwrap_i32(), 1234);
    assert_eq!(data.get_path_ref("scene.some").unwrap().unwrap_i32(), 1234);
    assert!(data.get_path_ref("scene.missing").is_none());
}

#[test]
fn it_gets_and_sets_deep_values() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );
    assert_eq!(
        data.get_path("scene.some.very.deep.property").unwrap_i32(),
        1234
    );
}

#[test]
fn it_gets_and_sets_subtree() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );

    let scene = data.get_tree("scene").unwrap();
    let mut data2 = ObservableKVTree::<ExampleValueType>::default();
    data.reset_update_cycle();

    let initial_version = data2.path_version("scene");

    data2.set_tree("scene", scene.clone());

    assert!(data2.path_version("scene") > initial_version);
    assert_eq!(
        data2.get_path("scene.some.very.deep.property").unwrap_i32(),
        1234
    );
    assert_eq!(
        data2.was_path_updated("scene.some.very.deep.property"),
        true
    );
    assert_eq!(data2.was_path_updated("scene.some.very.deep"), true);
    assert_eq!(data2.was_path_updated("scene.some.very"), true);
    assert_eq!(data2.was_path_updated("scene.some"), true);
    assert_eq!(data2.was_path_updated("scene"), true);
}

#[test]
fn it_increments_version() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );
    data.set_path(
        "scene.some.very.deep.property2",
        ExampleValueType::from(1234),
    );

    let scene = data.get_tree("scene").unwrap();
    let mut data2 = ObservableKVTree::<ExampleValueType>::default();

    assert_eq!(data2.path_version("scene.some.very.deep.property"), -1);
    assert_eq!(data2.path_version("scene.some.very.deep"), -1);
    assert_eq!(data2.path_version("scene.some.very"), -1);
    assert_eq!(data2.path_version("scene.some"), -1);
    assert_eq!(data2.path_version("scene"), -1);

    data2.set_tree("scene", scene.clone());

    // Each replaced node is updated once.
    assert_eq!(data2.path_version("scene.some.very.deep.property"), 1);
    assert_eq!(data2.path_version("scene.some.very.deep"), 1);
    assert_eq!(data2.path_version("scene.some.very"), 1);
    assert_eq!(data2.path_version("scene.some"), 1);
    assert_eq!(data2.path_version("scene"), 1);

    assert_eq!(
        data2.get_path("scene.some.very.deep.property").unwrap_i32(),
        1234
    );

    data2.set_path("scene.some.very.deep", ExampleValueType::I32(5555));

    assert_eq!(data2.path_version("scene.some.very.deep.property"), 1);
    assert_eq!(data2.path_version("scene.some.very.deep"), 2);
    assert_eq!(data2.path_version("scene.some.very"), 2);
    assert_eq!(data2.path_version("scene.some"), 2);
    assert_eq!(data2.path_version("scene"), 2);
}

#[test]
fn it_sends_updates() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );

    let receiver = data.create_update_channel();

    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(2345),
    );
    let update = receiver.recv().unwrap();
    assert_eq!(update.path, "scene.some.very.deep.property".to_string());
    assert_eq!(update.old_value.unwrap_i32(), 1234);
    assert_eq!(update.value.unwrap_i32(), 2345);

    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(3456),
    );
    let update = receiver.recv().unwrap();
    assert_eq!(update.path, "scene.some.very.deep.property".to_string());
    assert_eq!(update.old_value.unwrap_i32(), 2345);
    assert_eq!(update.value.unwrap_i32(), 3456);
}

#[test]
fn it_gets_none_when_not_set() {
    let data = ObservableKVTree::<ExampleValueType>::default();
    assert_eq!(
        data.get_path("scene.property.that.does.not.exist")
            .is_none(),
        true
    );
}

#[test]
fn it_changes_value() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(2345),
    );
    assert_eq!(
        data.get_path("scene.some.very.deep.property").unwrap_i32(),
        2345
    );
}

#[test]
fn it_detects_updates() {
    // Arrange
    let mut data = ObservableKVTree::<ExampleValueType>::default();

    // Pre condition

    // Set value
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(1234),
    );
    assert_eq!(data.was_path_updated("scene.some.very.deep.property"), true);
    assert_eq!(data.was_path_updated("scene.some.very.deep"), true);
    assert_eq!(data.was_path_updated("scene.some.very"), true);
    assert_eq!(data.was_path_updated("scene.some"), true);
    assert_eq!(data.was_path_updated("scene"), true);
    assert_eq!(data.update_tracker.updated, true);

    // Reset update cycle
    data.reset_update_cycle();
    assert_eq!(
        data.was_path_updated("scene.some.very.deep.property"),
        false
    );
    assert_eq!(data.was_path_updated("scene.some.very.deep"), false);
    assert_eq!(data.was_path_updated("scene.some.very"), false);
    assert_eq!(data.was_path_updated("scene.some"), false);
    assert_eq!(data.was_path_updated("scene"), false);
    assert_eq!(data.update_tracker.updated, false);

    // Set value (2nd time)
    data.set_path(
        "scene.some.very.deep.property",
        ExampleValueType::from(2345),
    );

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
    let deserialized: ObservableKVTree<ExampleValueType> =
        serde_json::from_str(&serialized).unwrap();

    assert_eq!(
        deserialized
            .get_path("scene.some.deep.property")
            .unwrap_f32(),
        123.4
    );
}

#[test]
fn it_makes_and_reverts_snapshots() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();

    data.set_path("scene.some.deep.property", ExampleValueType::from(123.4));
    data.make_snapshot();
    data.set_path("scene.some.deep.property", ExampleValueType::from(100.0));
    let v1 = data.make_snapshot();

    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        100.0
    );
    data.revert_snapshot_version(v1);
    data.set_path("scene.some.deep.property", ExampleValueType::from(123.4));
}

#[test]
fn goes_to_snapshot_with_version() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();

    data.set_path("scene.some.deep.property", ExampleValueType::from(123.4));
    let v1 = data.make_snapshot();
    data.set_path("scene.some.deep.property", ExampleValueType::from(100.0));
    data.make_snapshot();
    data.set_path("scene.some.deep.property", ExampleValueType::from(101.0));
    data.make_snapshot();
    data.set_path("scene.some.deep.property", ExampleValueType::from(102.0));
    let v2 = data.make_snapshot();

    data.go_to_snapshot_with_version(v1);
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        123.4
    );
    data.go_to_snapshot_with_version(v2);
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        102.0
    );
}

///  --------------------- UNDO/REDO ---------------------

#[test]
fn performs_undo_redo() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();

    data.set_path("scene.some.deep.property", ExampleValueType::from(123.4));
    data.make_undo_redo_snapshot();
    data.set_path("scene.some.deep.property", ExampleValueType::from(100.0));
    data.set_path("scene.some.deep.property", ExampleValueType::from(101.0));
    data.set_path("scene.some.deep.property", ExampleValueType::from(102.0));
    data.make_undo_redo_snapshot();

    data.undo();
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        123.4
    );
    data.redo();
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        102.0
    );

    // After this point, nothing is available for redo
    data.redo();
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        102.0
    );

    data.undo();
    data.undo();
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        123.4
    );
    data.undo();
    // Before this point, nothing is available for undo
    assert_eq!(
        data.get_path("scene.some.deep.property").unwrap_f32(),
        123.4
    );
}

#[test]
fn transient_updates_are_dirty_but_stay_out_of_undo_redo() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.animated", ExampleValueType::from(1.0));
    data.make_undo_redo_snapshot();
    let version = data.path_version("scene.animated");

    data.set_transient_path("scene.animated", ExampleValueType::from(9.0));
    assert!(data.path_version("scene.animated") > version);
    assert!(data.was_path_updated("scene.animated"));

    data.set_path("scene.authored", ExampleValueType::from(2.0));
    data.make_undo_redo_snapshot();
    data.undo();

    assert_eq!(data.get_path("scene.animated").unwrap_f32(), 9.0);
    assert!(data.get_path("scene.authored").is_none());
}

#[test]
fn undo_commits_and_reverts_a_pending_edit() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.value", ExampleValueType::from(1.0));
    data.make_undo_redo_snapshot();
    data.set_path("scene.value", ExampleValueType::from(2.0));

    data.undo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 1.0);
    data.redo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 2.0);
}

#[test]
fn empty_snapshot_boundaries_do_not_add_history_steps() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.value", ExampleValueType::from(1.0));
    data.make_undo_redo_snapshot();
    data.make_undo_redo_snapshot();
    data.set_path("scene.value", ExampleValueType::from(2.0));
    data.make_undo_redo_snapshot();

    assert_eq!(data.versions.len(), 2);
    data.undo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 1.0);
}

#[test]
fn clear_resets_undo_redo_history() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.value", ExampleValueType::from(1.0));
    data.make_undo_redo_snapshot();

    data.clear();

    assert!(data.snapshots.is_empty());
    assert!(data.versions.is_empty());
    assert_eq!(data.current_version_index, None);
    assert_eq!(data.update_tracker.corresponding_previous_version, None);
}

#[test]
fn new_committed_edit_after_redo_can_be_undone_and_redone() {
    let mut data = ObservableKVTree::<ExampleValueType>::default();
    data.set_path("scene.value", ExampleValueType::from(1.0));
    data.make_undo_redo_snapshot();
    data.set_path("scene.value", ExampleValueType::from(2.0));
    data.make_undo_redo_snapshot();
    data.undo();
    data.redo();
    data.set_path("scene.value", ExampleValueType::from(3.0));
    data.make_undo_redo_snapshot();
    data.undo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 2.0);
    data.redo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 3.0);
    data.undo();
    data.set_path("scene.value", ExampleValueType::from(4.0));
    data.make_undo_redo_snapshot();
    data.undo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 2.0);
    data.redo();
    assert_eq!(data.get_path("scene.value").unwrap_f32(), 4.0);
}

#[test]
fn branching_history_does_not_replay_abandoned_paths() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("base", Some(1));
    tree.make_undo_redo_snapshot();
    tree.set_path("abandoned", Some(2));
    tree.make_undo_redo_snapshot();
    tree.undo();
    tree.set_path("branch", Some(3));
    tree.make_undo_redo_snapshot();
    tree.undo();
    tree.redo();
    assert_eq!(tree.get_path("abandoned"), None);
    assert_eq!(tree.get_path("branch"), Some(3));
}

#[test]
fn redo_does_not_replay_starting_snapshot_over_transient_values() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("animated", Some(1));
    tree.make_undo_redo_snapshot();
    tree.set_path("other", Some(2));
    tree.make_undo_redo_snapshot();
    tree.undo();
    tree.set_transient_path("animated", Some(9));
    tree.redo();
    assert_eq!(tree.get_path("animated"), Some(9));
}

#[test]
fn navigating_to_current_snapshot_discards_pending_edits() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("value", Some(1));
    let version = tree.make_snapshot();
    tree.set_path("value", Some(2));
    tree.go_to_snapshot_with_version(version);
    assert_eq!(tree.get_path("value"), Some(1));
    assert!(tree.snapshot_change_accumulator.is_empty());
}

#[test]
fn subtree_replacement_marks_each_ancestor_once_and_removes_old_keys() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("scene.old.deep", Some(1));
    let mut scene = ObservableKVTree::default();
    scene.set_path("new", Some(2));
    let version = tree.update_tracker.version();
    tree.set_tree("scene", scene);
    assert_eq!(tree.update_tracker.version(), version + 1);
    assert_eq!(tree.path_version("scene.new"), 1);
    assert!(tree.get_tree("scene.old").is_none());
    assert_eq!(tree.get_path("scene.new"), Some(2));
}

#[test]
fn clear_changes_authored_version() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("scene.value", Some(1));
    let version = tree.authored_version();
    tree.clear();
    assert_ne!(tree.authored_version(), version);
}

#[test]
fn replacing_a_scene_discards_old_pending_edits_and_history() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("scene.old", Some(1));
    tree.make_undo_redo_snapshot();
    tree.set_path("scene.old", Some(2));
    tree.make_undo_redo_snapshot();
    tree.undo();
    tree.set_path("scene.pending", Some(3));
    let mut next = ObservableKVTree::default();
    next.set_path("new", Some(4));
    next.make_undo_redo_snapshot();
    tree.set_tree("scene", next);
    for _ in 0..3 {
        tree.undo();
        tree.redo();
        assert!(tree.get_tree("scene.old").is_none());
        assert!(tree.get_tree("scene.pending").is_none());
        assert_eq!(tree.get_path("scene.new"), Some(4));
    }
    assert!(tree.snapshots.is_empty());
    assert!(tree.get_tree("scene").unwrap().snapshots.is_empty());
}

#[test]
fn empty_raw_snapshot_preserves_forward_history_and_version_identity() {
    let mut tree = ObservableKVTree::<Option<i32>>::default();
    tree.set_path("value", Some(1));
    let first = tree.make_snapshot();
    assert_eq!(tree.make_snapshot(), first);
    tree.set_path("value", Some(2));
    let second = tree.make_snapshot();
    tree.rewind_to_version(first);
    assert_eq!(tree.make_snapshot(), first);
    tree.fast_forward_to_version(second);
    assert_eq!(tree.get_path("value"), Some(2));
}

#[test]
fn history_matches_a_value_model_across_many_branches() {
    for seed in 1..=16u64 {
        let mut rng = seed;
        let mut tree = ObservableKVTree::<Option<i32>>::default();
        tree.set_path("a", Some(0));
        tree.make_undo_redo_snapshot();
        let mut states = vec![[Some(0), None, None]];
        let mut cursor: usize = 0;
        let paths = ["a", "a.child", "b"];
        for step in 0..300 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            match (rng >> 32) % 5 {
                0 => {
                    tree.undo();
                    cursor = cursor.saturating_sub(1);
                }
                1 => {
                    tree.redo();
                    cursor = (cursor + 1).min(states.len() - 1);
                }
                _ => {
                    let mut state = states[cursor];
                    let key = ((rng >> 16) % 3) as usize;
                    state[key] = Some(step);
                    tree.set_path(paths[key], Some(-1));
                    tree.set_path(paths[key], state[key]);
                    tree.make_undo_redo_snapshot();
                    states.truncate(cursor + 1);
                    states.push(state);
                    cursor += 1;
                }
            }
            for (key, path) in paths.iter().enumerate() {
                assert_eq!(
                    tree.get_path(path),
                    states[cursor][key],
                    "seed {seed}, step {step}, {path}"
                );
            }
        }
    }
}
