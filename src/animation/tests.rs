use super::*;
use crate::model::{
    objects, set_objects, AnimatableProperty, Lattice, PrimitiveKind, SdfObject, VectorAxis,
};

#[test]
fn lattice_shape_keys_use_scalar_bezier_tracks_and_survive_resize() {
    let mut tree = DataTree::default();
    let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
    object.lattice = Some(Lattice::new(glam::Vec3::splat(-1.0), glam::Vec3::ONE, 2));
    let id = object.uuid;
    set_objects(&mut tree, vec![object.clone()]);
    assert!(insert_lattice_keyframe(&mut tree, id, 0));
    let lattice = object.lattice.as_mut().unwrap();
    lattice.offsets[0] = glam::Vec3::new(2.0, 4.0, 6.0);
    assert_eq!(lattice.add_shape_key(), 1);
    set_objects(&mut tree, vec![object]);
    assert!(insert_lattice_keyframe(&mut tree, id, 10));
    let binding = AnimationBinding {
        object: id,
        property: AnimatableProperty::LatticeShape,
    };
    assert_eq!(animation_data(&tree).tracks[0].keyframes[1].value, 1.0);
    assert!(evaluate(&mut tree, 5.0));
    assert!(objects(&tree)[0].lattice.as_ref().unwrap().offsets[0]
        .abs_diff_eq(glam::Vec3::new(1.0, 2.0, 3.0), 0.0001));

    let mut second_shape = objects(&tree);
    let lattice = second_shape[0].lattice.as_mut().unwrap();
    lattice.select_shape_key(0);
    lattice.offsets[0] = glam::Vec3::new(-2.0, -4.0, -6.0);
    assert_eq!(lattice.add_shape_key(), 2);
    set_objects(&mut tree, second_shape);
    assert!(insert_lattice_keyframe(&mut tree, id, 20));
    evaluate(&mut tree, 15.0);
    assert!(objects(&tree)[0].lattice.as_ref().unwrap().offsets[0]
        .abs_diff_eq(glam::Vec3::ZERO, 0.0001));

    set_bezier_handle(
        &mut tree,
        SelectedKeyframe { binding, frame: 0 },
        false,
        BezierHandle {
            frame_offset: 3.0,
            value_offset: 1.0,
        },
    );
    assert!(sample(&animation_data(&tree).tracks[0], 2.5).unwrap() > 0.25);
    let mut resized = objects(&tree);
    resized[0].lattice.as_mut().unwrap().resize(3);
    assert_eq!(
        resized[0].lattice.as_ref().unwrap().shape_keys[0]
            .offsets
            .len(),
        27
    );
    set_objects(&mut tree, resized);
    evaluate(&mut tree, 5.0);
    let value = sample(&animation_data(&tree).tracks[0], 5.0).unwrap();
    assert!(objects(&tree)[0].lattice.as_ref().unwrap().offsets[0]
        .abs_diff_eq(glam::Vec3::new(2.0, 4.0, 6.0) * value, 0.0001));
    let serialized = serde_json::to_string(&objects(&tree)[0].lattice).unwrap();
    let restored: Option<Lattice> = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.unwrap().shape_keys[0].offsets.len(), 27);

    let duplicate = uuid::Uuid::new_v4();
    duplicate_tracks_for_objects(
        &mut tree,
        &std::collections::HashMap::from([(id, duplicate)]),
    );
    remove_tracks_for_objects(&mut tree, &[id]);
    assert_eq!(animation_data(&tree).tracks.len(), 1);
    assert_eq!(animation_data(&tree).tracks[0].binding.object, duplicate);
}

#[test]
fn legacy_lattice_pose_tracks_become_shape_keys() {
    let mut tree = DataTree::default();
    let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
    object.lattice = Some(Lattice::new(glam::Vec3::splat(-1.0), glam::Vec3::ONE, 2));
    let id = object.uuid;
    set_objects(&mut tree, vec![object]);
    let mut data = AnimationData::default();
    data.lattice_tracks
        .push(crate::model::LatticeAnimationTrack {
            object: id,
            resolution: 2,
            keyframes: vec![crate::model::LatticeKeyframe {
                frame: 12,
                offsets: vec![glam::Vec3::ONE; 8],
            }],
        });
    set_animation_data(&mut tree, data);
    evaluate(&mut tree, 12.0);
    let migrated = animation_data(&tree);
    assert!(migrated.lattice_tracks.is_empty());
    assert_eq!(migrated.tracks[0].keyframes[0].value, 1.0);
    assert_eq!(
        objects(&tree)[0].lattice.as_ref().unwrap().shape_keys[0].offsets,
        vec![glam::Vec3::ONE; 8]
    );
}

#[test]
fn linear_and_constant_tracks_sample_explicitly() {
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::X),
    };
    let mut track = AnimationTrack {
        binding,
        keyframes: vec![
            Keyframe {
                frame: 0,
                value: 2.0,
                interpolation: KeyframeInterpolation::Linear,
                incoming_handle: None,
                outgoing_handle: None,
            },
            Keyframe {
                frame: 10,
                value: 6.0,
                interpolation: KeyframeInterpolation::Linear,
                incoming_handle: None,
                outgoing_handle: None,
            },
        ],
    };
    assert_eq!(sample(&track, 5.0), Some(4.0));
    track.keyframes[0].interpolation = KeyframeInterpolation::Constant;
    assert_eq!(sample(&track, 5.0), Some(2.0));
}

#[test]
fn bezier_is_default_and_edited_handles_curve_the_segment() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::X),
    };
    insert_keyframe(&mut tree, binding, 0, 0.0);
    insert_keyframe(&mut tree, binding, 10, 10.0);
    assert_eq!(
        animation_data(&tree).tracks[0].keyframes[0].interpolation,
        KeyframeInterpolation::Bezier
    );
    assert_eq!(
        easing_preset(&animation_data(&tree).tracks[0], 0),
        Some(EasingPreset::Smooth)
    );
    assert!(sample(&animation_data(&tree).tracks[0], 2.5).unwrap() < 2.5);
    set_bezier_handle(
        &mut tree,
        SelectedKeyframe { binding, frame: 0 },
        false,
        BezierHandle {
            frame_offset: 3.0,
            value_offset: 9.0,
        },
    );
    set_bezier_handle(
        &mut tree,
        SelectedKeyframe { binding, frame: 10 },
        true,
        BezierHandle {
            frame_offset: -3.0,
            value_offset: 0.0,
        },
    );
    assert!(sample(&animation_data(&tree).tracks[0], 5.0).unwrap() > 6.0);
    assert_eq!(
        easing_preset(&animation_data(&tree).tracks[0], 0),
        Some(EasingPreset::CustomBezier)
    );
}

#[test]
fn easing_presets_map_to_explicit_segment_shapes() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::X),
    };
    insert_keyframe(&mut tree, binding, 0, 0.0);
    insert_keyframe(&mut tree, binding, 10, 10.0);
    let selected = SelectedKeyframe { binding, frame: 0 };

    for preset in EasingPreset::EDITABLE {
        apply_easing_preset(&mut tree, selected, preset);
        assert_eq!(
            easing_preset(&animation_data(&tree).tracks[0], 0),
            Some(preset)
        );
    }
}

#[test]
fn keyframes_serialize_and_drive_scene_properties() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::Y),
    };
    set_objects(&mut tree, vec![object]);
    insert_keyframe(&mut tree, binding, 0, 0.0);
    insert_keyframe(&mut tree, binding, 24, 4.0);
    assert!(evaluate(&mut tree, 12.0));
    assert!((objects(&tree)[0].transform.translation.y - 2.0).abs() < 0.0001);

    let bytes = serde_json::to_vec(&tree).unwrap();
    let restored: DataTree = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(animation_data(&restored).tracks[0].keyframes.len(), 2);
}

#[test]
fn camera_tracks_drive_serialized_scene_cameras() {
    let mut tree = DataTree::default();
    let view = crate::camera::Camera::new();
    let camera = crate::camera::SceneCamera::from_view("Shot", &view);
    let id = camera.uuid;
    crate::model::set_scene_cameras(&mut tree, vec![camera]);
    let binding = AnimationBinding {
        object: id,
        property: crate::model::AnimatableProperty::Position(crate::model::VectorAxis::X),
    };
    insert_keyframe(&mut tree, binding, 0, -2.0);
    insert_keyframe(&mut tree, binding, 10, 4.0);
    evaluate(&mut tree, 10.0);
    assert_eq!(
        crate::model::scene_cameras(&tree)[0]
            .transform
            .translation
            .x,
        4.0
    );
}

#[test]
fn inserting_same_frame_replaces_value_without_duplicate() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Box);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Scale(VectorAxis::Z),
    };
    insert_keyframe(&mut tree, binding, 7, 1.0);
    insert_keyframe(&mut tree, binding, 7, 3.0);
    let track = &animation_data(&tree).tracks[0];
    assert_eq!(track.keyframes.len(), 1);
    assert_eq!(track.keyframes[0].value, 3.0);
}

#[test]
fn field_state_distinguishes_animated_and_current_keyframes() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Box);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Scale(VectorAxis::X),
    };
    let other_binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Scale(VectorAxis::Y),
    };
    insert_keyframe(&mut tree, binding, 8, 2.0);

    assert_eq!(
        field_animation_state(&tree, binding, 8.0),
        FieldAnimationState::KeyedAtCurrentFrame
    );
    assert_eq!(
        field_animation_state(&tree, binding, 7.0),
        FieldAnimationState::Animated
    );
    assert_eq!(
        field_animation_state(&tree, other_binding, 8.0),
        FieldAnimationState::NotAnimated
    );
}

#[test]
fn object_lifecycle_updates_its_tracks() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Box);
    let copy = uuid::Uuid::new_v4();
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Scale(VectorAxis::X),
    };
    insert_keyframe(&mut tree, binding, 2, 1.0);
    duplicate_tracks_for_objects(
        &mut tree,
        &std::collections::HashMap::from([(object.uuid, copy)]),
    );
    assert_eq!(animation_data(&tree).tracks.len(), 2);
    remove_tracks_for_objects(&mut tree, &[object.uuid]);
    let tracks = animation_data(&tree).tracks;
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].binding.object, copy);
}

#[test]
fn realtime_tick_advances_at_the_serialized_frame_rate() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Sphere);
    let binding = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::X),
    };
    set_objects(&mut tree, vec![object]);
    insert_keyframe(&mut tree, binding, 0, 0.0);
    insert_keyframe(&mut tree, binding, 24, 24.0);
    let mut runtime = AnimationRuntime {
        playing: true,
        ..AnimationRuntime::default()
    };
    let start = Instant::now();
    assert!(!runtime.tick(&mut tree, start));
    assert!(runtime.tick(&mut tree, start + std::time::Duration::from_millis(250)));
    assert_eq!(runtime.current_frame, 6.0);
    assert!((objects(&tree)[0].transform.translation.x - 3.75).abs() < 0.0001);
}

#[test]
fn moving_and_deleting_multiple_keyframes_is_atomic_across_tracks() {
    let mut tree = DataTree::default();
    let object = SdfObject::create_kind(PrimitiveKind::Box);
    let x = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::X),
    };
    let y = AnimationBinding {
        object: object.uuid,
        property: AnimatableProperty::Position(VectorAxis::Y),
    };
    insert_keyframe(&mut tree, x, 3, 1.0);
    insert_keyframe(&mut tree, x, 9, 2.0);
    insert_keyframe(&mut tree, y, 5, 3.0);
    let outgoing_handle = BezierHandle {
        frame_offset: 2.0,
        value_offset: 0.75,
    };
    set_bezier_handle(
        &mut tree,
        SelectedKeyframe {
            binding: x,
            frame: 3,
        },
        false,
        outgoing_handle,
    );
    let selected = [
        SelectedKeyframe {
            binding: x,
            frame: 3,
        },
        SelectedKeyframe {
            binding: y,
            frame: 5,
        },
    ];

    let moved = move_keyframes(&mut tree, &selected, 4);

    assert_eq!(
        moved,
        vec![
            SelectedKeyframe {
                binding: x,
                frame: 7,
            },
            SelectedKeyframe {
                binding: y,
                frame: 9,
            },
        ]
    );
    let data = animation_data(&tree);
    assert_eq!(
        data.tracks[0]
            .keyframes
            .iter()
            .map(|key| key.frame)
            .collect::<Vec<_>>(),
        vec![7, 9]
    );
    assert_eq!(data.tracks[1].keyframes[0].frame, 9);
    let moved_handle = data.tracks[0].keyframes[0]
        .outgoing_handle
        .expect("moved key keeps its outgoing handle");
    assert_eq!(moved_handle.frame_offset, outgoing_handle.frame_offset);
    assert_eq!(moved_handle.value_offset, outgoing_handle.value_offset);

    delete_keyframes(&mut tree, &moved);

    let data = animation_data(&tree);
    assert_eq!(data.tracks.len(), 1);
    assert_eq!(data.tracks[0].keyframes[0].frame, 9);
}
