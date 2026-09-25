use super::*;

pub fn insert_keyframe(tree: &mut DataTree, binding: AnimationBinding, frame: u32, value: f32) {
    let mut data = animation_data(tree);
    data.end_frame = data.end_frame.max(frame);
    let track = if let Some(track) = data
        .tracks
        .iter_mut()
        .find(|track| track.binding == binding)
    {
        track
    } else {
        data.tracks.push(AnimationTrack {
            binding,
            keyframes: Vec::new(),
        });
        data.tracks.last_mut().expect("inserted animation track")
    };
    let interpolation = if binding.property.uses_step_interpolation() {
        KeyframeInterpolation::Constant
    } else {
        KeyframeInterpolation::Bezier
    };
    if let Some(keyframe) = track
        .keyframes
        .iter_mut()
        .find(|keyframe| keyframe.frame == frame)
    {
        keyframe.value = value;
    } else {
        track.keyframes.push(Keyframe {
            frame,
            value,
            interpolation,
            incoming_handle: None,
            outgoing_handle: None,
        });
        track.keyframes.sort_by_key(|keyframe| keyframe.frame);
    }
    set_animation_data(tree, data);
}

pub fn insert_lattice_keyframe(tree: &mut DataTree, object_id: uuid::Uuid, frame: u32) -> bool {
    migrate_legacy_lattice_tracks(tree);
    let Some(index) = objects(tree)
        .iter()
        .find(|object| object.uuid == object_id)
        .and_then(|object| object.lattice.as_ref())
        .and_then(|lattice| {
            lattice
                .current_shape_key
                .filter(|index| *index <= lattice.shape_keys.len())
        })
    else {
        return false;
    };
    insert_keyframe(
        tree,
        AnimationBinding {
            object: object_id,
            property: AnimatableProperty::LatticeShape,
        },
        frame,
        index as f32,
    );
    true
}

pub fn remove_lattice_track(tree: &mut DataTree, object_id: uuid::Uuid) {
    let mut data = animation_data(tree);
    data.lattice_tracks
        .retain(|track| track.object != object_id);
    data.tracks.retain(|track| {
        !(track.binding.object == object_id
            && track.binding.property == AnimatableProperty::LatticeShape)
    });
    set_animation_data(tree, data);
}

pub fn remove_repetition_tracks(tree: &mut DataTree, object_id: uuid::Uuid) {
    let mut data = animation_data(tree);
    data.tracks.retain(|track| {
        track.binding.object != object_id
            || !matches!(
                track.binding.property,
                AnimatableProperty::RepetitionEnabled
                    | AnimatableProperty::RepetitionAxis(_)
                    | AnimatableProperty::RepetitionCount(_)
                    | AnimatableProperty::RepetitionSpacing(_)
            )
    });
    set_animation_data(tree, data);
}

pub fn remap_lattice_shape_keys_after_delete(
    tree: &mut DataTree,
    object_id: uuid::Uuid,
    deleted: usize,
) {
    let mut data = animation_data(tree);
    let binding = AnimationBinding {
        object: object_id,
        property: AnimatableProperty::LatticeShape,
    };
    if let Some(track) = data
        .tracks
        .iter_mut()
        .find(|track| track.binding == binding)
    {
        for keyframe in &mut track.keyframes {
            if keyframe.value >= deleted as f32 {
                keyframe.value = (keyframe.value - 1.0).max(0.0);
            }
        }
    }
    set_animation_data(tree, data);
}

fn resample_lattice(
    offsets: &[glam::Vec3],
    resolution: u8,
    target: &Lattice,
) -> Option<Vec<glam::Vec3>> {
    let n = resolution as usize;
    if !(2..=9).contains(&n) || offsets.len() != n * n * n {
        return None;
    }
    let mut lattice = Lattice::new(target.min, target.max, resolution);
    lattice.offsets = offsets.to_vec();
    lattice.resize(target.resolution);
    Some(lattice.offsets)
}

/// Convert pose snapshots from documents written by the first lattice animation version.
pub fn migrate_legacy_lattice_tracks(tree: &mut DataTree) {
    let mut data = animation_data(tree);
    if data.lattice_tracks.is_empty() {
        return;
    }
    let mut scene = objects(tree);
    let old_tracks = std::mem::take(&mut data.lattice_tracks);
    for old in old_tracks {
        let Some(lattice) = scene
            .iter_mut()
            .find(|object| object.uuid == old.object)
            .and_then(|object| object.lattice.as_mut())
        else {
            continue;
        };
        let binding = AnimationBinding {
            object: old.object,
            property: AnimatableProperty::LatticeShape,
        };
        let track = if let Some(track) = data
            .tracks
            .iter_mut()
            .find(|track| track.binding == binding)
        {
            track
        } else {
            data.tracks.push(AnimationTrack {
                binding,
                keyframes: Vec::new(),
            });
            data.tracks.last_mut().expect("inserted shape track")
        };
        for pose in old.keyframes {
            let Some(offsets) = resample_lattice(&pose.offsets, old.resolution, lattice) else {
                continue;
            };
            let index = lattice.shape_keys.len() + 1;
            lattice.shape_keys.push(LatticeShapeKey {
                name: format!("Shape {index}"),
                offsets,
            });
            track.keyframes.push(Keyframe {
                frame: pose.frame,
                value: index as f32,
                interpolation: KeyframeInterpolation::Linear,
                incoming_handle: None,
                outgoing_handle: None,
            });
        }
        track.keyframes.sort_by_key(|keyframe| keyframe.frame);
    }
    set_objects(tree, scene);
    set_animation_data(tree, data);
}

pub fn update_keyframe(
    tree: &mut DataTree,
    selected: SelectedKeyframe,
    frame: u32,
    value: f32,
    interpolation: KeyframeInterpolation,
) -> SelectedKeyframe {
    let mut data = animation_data(tree);
    data.end_frame = data.end_frame.max(frame);
    if let Some(track) = data
        .tracks
        .iter_mut()
        .find(|track| track.binding == selected.binding)
    {
        let mut updated = track
            .keyframes
            .iter()
            .find(|keyframe| keyframe.frame == selected.frame)
            .cloned()
            .unwrap_or(Keyframe {
                frame,
                value,
                interpolation,
                incoming_handle: None,
                outgoing_handle: None,
            });
        updated.frame = frame;
        updated.value = value;
        updated.interpolation = interpolation;
        track
            .keyframes
            .retain(|keyframe| keyframe.frame != selected.frame && keyframe.frame != frame);
        track.keyframes.push(updated);
        track.keyframes.sort_by_key(|keyframe| keyframe.frame);
    }
    set_animation_data(tree, data);
    SelectedKeyframe {
        binding: selected.binding,
        frame,
    }
}

pub fn set_bezier_handle(
    tree: &mut DataTree,
    selected: SelectedKeyframe,
    incoming: bool,
    handle: BezierHandle,
) {
    let mut data = animation_data(tree);
    if let Some(keyframe) = data
        .tracks
        .iter_mut()
        .find(|track| track.binding == selected.binding)
        .and_then(|track| {
            track
                .keyframes
                .iter_mut()
                .find(|keyframe| keyframe.frame == selected.frame)
        })
    {
        if incoming {
            keyframe.incoming_handle = Some(handle);
        } else {
            keyframe.outgoing_handle = Some(handle);
        }
        set_animation_data(tree, data);
    }
}

pub fn easing_preset(track: &AnimationTrack, frame: u32) -> Option<EasingPreset> {
    let index = track
        .keyframes
        .iter()
        .position(|keyframe| keyframe.frame == frame)?;
    let left = &track.keyframes[index];
    let right = track.keyframes.get(index + 1)?;
    match left.interpolation {
        KeyframeInterpolation::Constant => Some(EasingPreset::Constant),
        KeyframeInterpolation::Linear => Some(EasingPreset::Linear),
        KeyframeInterpolation::Bezier => {
            let span = (right.frame - left.frame) as f32;
            let delta = right.value - left.value;
            let outgoing = left.outgoing_handle.unwrap_or(BezierHandle {
                frame_offset: span / 3.0,
                value_offset: 0.0,
            });
            let incoming = right.incoming_handle.unwrap_or(BezierHandle {
                frame_offset: -span / 3.0,
                value_offset: 0.0,
            });
            let close = |a: f32, b: f32| (a - b).abs() < 0.0001;
            let standard_times = close(outgoing.frame_offset, span / 3.0)
                && close(incoming.frame_offset, -span / 3.0);
            if standard_times
                && close(outgoing.value_offset, 0.0)
                && close(incoming.value_offset, 0.0)
            {
                Some(EasingPreset::Smooth)
            } else if standard_times
                && close(outgoing.value_offset, 0.0)
                && close(incoming.value_offset, -delta / 3.0)
            {
                Some(EasingPreset::EaseIn)
            } else if standard_times
                && close(outgoing.value_offset, delta / 3.0)
                && close(incoming.value_offset, 0.0)
            {
                Some(EasingPreset::EaseOut)
            } else {
                Some(EasingPreset::CustomBezier)
            }
        }
    }
}

pub fn apply_easing_preset(tree: &mut DataTree, selected: SelectedKeyframe, preset: EasingPreset) {
    let mut data = animation_data(tree);
    let Some(track) = data
        .tracks
        .iter_mut()
        .find(|track| track.binding == selected.binding)
    else {
        return;
    };
    let Some(index) = track
        .keyframes
        .iter()
        .position(|keyframe| keyframe.frame == selected.frame)
    else {
        return;
    };
    if index + 1 >= track.keyframes.len() {
        return;
    }
    let (left, right) = track.keyframes.split_at_mut(index + 1);
    let left = &mut left[index];
    let right = &mut right[0];
    let span = (right.frame - left.frame) as f32;
    let delta = right.value - left.value;
    match preset {
        EasingPreset::Smooth => {
            left.interpolation = KeyframeInterpolation::Bezier;
            left.outgoing_handle = None;
            right.incoming_handle = None;
        }
        EasingPreset::EaseIn => {
            left.interpolation = KeyframeInterpolation::Bezier;
            left.outgoing_handle = Some(BezierHandle {
                frame_offset: span / 3.0,
                value_offset: 0.0,
            });
            right.incoming_handle = Some(BezierHandle {
                frame_offset: -span / 3.0,
                value_offset: -delta / 3.0,
            });
        }
        EasingPreset::EaseOut => {
            left.interpolation = KeyframeInterpolation::Bezier;
            left.outgoing_handle = Some(BezierHandle {
                frame_offset: span / 3.0,
                value_offset: delta / 3.0,
            });
            right.incoming_handle = Some(BezierHandle {
                frame_offset: -span / 3.0,
                value_offset: 0.0,
            });
        }
        EasingPreset::Linear => left.interpolation = KeyframeInterpolation::Linear,
        EasingPreset::Constant => left.interpolation = KeyframeInterpolation::Constant,
        EasingPreset::CustomBezier => return,
    }
    set_animation_data(tree, data);
}

pub fn bezier_control_points(left: &Keyframe, right: &Keyframe) -> [(f32, f32); 4] {
    let frame_span = (right.frame - left.frame) as f32;
    let outgoing = left.outgoing_handle.unwrap_or(BezierHandle {
        frame_offset: frame_span / 3.0,
        value_offset: 0.0,
    });
    let incoming = right.incoming_handle.unwrap_or(BezierHandle {
        frame_offset: -frame_span / 3.0,
        value_offset: 0.0,
    });
    [
        (left.frame as f32, left.value),
        (
            left.frame as f32 + outgoing.frame_offset.clamp(0.0, frame_span),
            left.value + outgoing.value_offset,
        ),
        (
            right.frame as f32 + incoming.frame_offset.clamp(-frame_span, 0.0),
            right.value + incoming.value_offset,
        ),
        (right.frame as f32, right.value),
    ]
}

pub(super) fn cubic_bezier(a: f32, b: f32, c: f32, d: f32, amount: f32) -> f32 {
    let inverse = 1.0 - amount;
    inverse * inverse * inverse * a
        + 3.0 * inverse * inverse * amount * b
        + 3.0 * inverse * amount * amount * c
        + amount * amount * amount * d
}

pub fn bezier_point(points: [(f32, f32); 4], amount: f32) -> (f32, f32) {
    (
        cubic_bezier(points[0].0, points[1].0, points[2].0, points[3].0, amount),
        cubic_bezier(points[0].1, points[1].1, points[2].1, points[3].1, amount),
    )
}

pub fn delete_keyframe(tree: &mut DataTree, selected: SelectedKeyframe) {
    delete_keyframes(tree, &[selected]);
}

pub fn delete_keyframes(tree: &mut DataTree, selected: &[SelectedKeyframe]) {
    let mut data = animation_data(tree);
    for track in &mut data.tracks {
        track.keyframes.retain(|keyframe| {
            !selected.iter().any(|selected| {
                selected.binding == track.binding && selected.frame == keyframe.frame
            })
        });
    }
    data.tracks.retain(|track| !track.keyframes.is_empty());
    set_animation_data(tree, data);
}

pub fn move_keyframes(
    tree: &mut DataTree,
    selected: &[SelectedKeyframe],
    frame_delta: i32,
) -> Vec<SelectedKeyframe> {
    if frame_delta == 0 || selected.is_empty() {
        return selected.to_vec();
    }
    let mut data = animation_data(tree);
    let mut moved = Vec::new();
    for track in &mut data.tracks {
        let track_selection: Vec<_> = selected
            .iter()
            .copied()
            .filter(|selected| selected.binding == track.binding)
            .collect();
        if track_selection.is_empty() {
            continue;
        }
        let mut moving = Vec::new();
        track.keyframes.retain(|keyframe| {
            if track_selection
                .iter()
                .any(|selected| selected.frame == keyframe.frame)
            {
                moving.push(keyframe.clone());
                false
            } else {
                true
            }
        });
        for mut keyframe in moving {
            let frame = (keyframe.frame as i64 + frame_delta as i64).max(0) as u32;
            keyframe.frame = frame;
            track.keyframes.retain(|existing| existing.frame != frame);
            track.keyframes.push(keyframe);
            moved.push(SelectedKeyframe {
                binding: track.binding,
                frame,
            });
        }
        track.keyframes.sort_by_key(|keyframe| keyframe.frame);
    }
    set_animation_data(tree, data);
    moved
}

pub fn remove_tracks_for_objects(tree: &mut DataTree, objects: &[uuid::Uuid]) {
    let mut data = animation_data(tree);
    data.tracks
        .retain(|track| !objects.contains(&track.binding.object));
    data.lattice_tracks
        .retain(|track| !objects.contains(&track.object));
    set_animation_data(tree, data);
}

pub fn duplicate_tracks_for_objects(
    tree: &mut DataTree,
    object_map: &std::collections::HashMap<uuid::Uuid, uuid::Uuid>,
) {
    let mut data = animation_data(tree);
    let copies: Vec<_> = data
        .tracks
        .iter()
        .filter_map(|track| {
            let object = object_map.get(&track.binding.object)?;
            let mut copy = track.clone();
            copy.binding.object = *object;
            Some(copy)
        })
        .collect();
    data.tracks.extend(copies);
    let lattice_copies: Vec<_> = data
        .lattice_tracks
        .iter()
        .filter_map(|track| {
            let object = object_map.get(&track.object)?;
            let mut copy = track.clone();
            copy.object = *object;
            Some(copy)
        })
        .collect();
    data.lattice_tracks.extend(lattice_copies);
    set_animation_data(tree, data);
}
