use web_time::Instant;

use crate::model::{
    objects, scene_cameras, set_objects, set_objects_transient, AnimatableProperty,
    AnimationBinding, AnimationData, AnimationTrack, BezierHandle, ClaydashValue, DataTree,
    Keyframe, KeyframeInterpolation, Lattice, LatticeShapeKey,
};

const ANIMATION_PATH: &str = "scene.animation";

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct SelectedKeyframe {
    pub binding: AnimationBinding,
    pub frame: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeDrag {
    pub anchor: SelectedKeyframe,
    pub keyframes: Vec<SelectedKeyframe>,
    pub preview_delta: i32,
    pub start_pointer_x: f32,
    pub pixels_per_frame: f32,
    pub keyboard_initiated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineBoxSelection {
    pub start: egui::Pos2,
    pub end: egui::Pos2,
    pub initial: Vec<SelectedKeyframe>,
    pub additive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldAnimationState {
    NotAnimated,
    Animated,
    KeyedAtCurrentFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingPreset {
    Smooth,
    EaseIn,
    EaseOut,
    Linear,
    Constant,
    CustomBezier,
}

impl EasingPreset {
    pub const EDITABLE: [Self; 5] = [
        Self::Smooth,
        Self::EaseIn,
        Self::EaseOut,
        Self::Linear,
        Self::Constant,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::Linear => "Linear",
            Self::Constant => "Constant",
            Self::CustomBezier => "Custom Bézier",
        }
    }
}

pub struct AnimationRuntime {
    pub current_frame: f32,
    pub playing: bool,
    pub looping: bool,
    pub selected_keyframes: Vec<SelectedKeyframe>,
    pub keyframe_drag: Option<KeyframeDrag>,
    pub timeline_box_selection: Option<TimelineBoxSelection>,
    pub timeline_time_scale: f32,
    pub timeline_time_center: Option<f32>,
    pub timeline_track_height: f32,
    pub timeline_scroll_axis: Option<TimelineScrollAxis>,
    last_tick: Option<Instant>,
}

impl Default for AnimationRuntime {
    fn default() -> Self {
        Self {
            current_frame: 0.0,
            playing: false,
            looping: true,
            selected_keyframes: Vec::new(),
            keyframe_drag: None,
            timeline_box_selection: None,
            timeline_time_scale: 1.0,
            timeline_time_center: None,
            timeline_track_height: 64.0,
            timeline_scroll_axis: None,
            last_tick: None,
        }
    }
}

impl AnimationRuntime {
    pub fn toggle_playback(&mut self) {
        self.playing = !self.playing;
        self.last_tick = None;
    }

    pub fn stop(&mut self, tree: &mut DataTree) {
        self.playing = false;
        self.last_tick = None;
        let data = animation_data(tree);
        self.current_frame = data.start_frame as f32;
        evaluate(tree, self.current_frame);
    }

    pub fn set_frame(&mut self, tree: &mut DataTree, frame: f32) {
        let data = animation_data(tree);
        self.current_frame = frame.clamp(data.start_frame as f32, data.end_frame as f32);
        self.last_tick = None;
        evaluate(tree, self.current_frame);
    }

    pub fn tick(&mut self, tree: &mut DataTree, now: Instant) -> bool {
        if !self.playing {
            self.last_tick = None;
            return false;
        }
        let data = animation_data(tree);
        let last = self.last_tick.replace(now).unwrap_or(now);
        let elapsed = now.duration_since(last).as_secs_f32().min(0.25);
        if elapsed == 0.0 {
            return false;
        }
        self.current_frame += elapsed * data.fps.max(1.0);
        if self.current_frame > data.end_frame as f32 {
            if self.looping {
                let length = data.end_frame.saturating_sub(data.start_frame).max(1) as f32;
                self.current_frame = data.start_frame as f32
                    + (self.current_frame - data.start_frame as f32) % length;
            } else {
                self.current_frame = data.end_frame as f32;
                self.playing = false;
                self.last_tick = None;
            }
        }
        evaluate(tree, self.current_frame)
    }

    pub fn reset_for_document(&mut self, tree: &DataTree) {
        self.playing = false;
        self.last_tick = None;
        self.selected_keyframes.clear();
        self.keyframe_drag = None;
        self.timeline_box_selection = None;
        self.timeline_time_scale = 1.0;
        self.timeline_time_center = None;
        self.timeline_track_height = 64.0;
        self.timeline_scroll_axis = None;
        self.current_frame = animation_data(tree).start_frame as f32;
    }
}

pub fn animation_data(tree: &DataTree) -> AnimationData {
    match tree.get_path(ANIMATION_PATH) {
        ClaydashValue::Animation(data) => data,
        _ => AnimationData::default(),
    }
}

pub fn field_animation_state(
    tree: &DataTree,
    binding: AnimationBinding,
    current_frame: f32,
) -> FieldAnimationState {
    let Some(ClaydashValue::Animation(data)) = tree.get_path_ref(ANIMATION_PATH) else {
        return FieldAnimationState::NotAnimated;
    };
    let Some(track) = data.tracks.iter().find(|track| track.binding == binding) else {
        return FieldAnimationState::NotAnimated;
    };
    if track
        .keyframes
        .iter()
        .any(|keyframe| (keyframe.frame as f32 - current_frame).abs() < 0.001)
    {
        FieldAnimationState::KeyedAtCurrentFrame
    } else {
        FieldAnimationState::Animated
    }
}

pub fn set_animation_data(tree: &mut DataTree, data: AnimationData) {
    tree.set_path(ANIMATION_PATH, ClaydashValue::Animation(data));
}

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

fn cubic_bezier(a: f32, b: f32, c: f32, d: f32, amount: f32) -> f32 {
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

pub fn evaluate(tree: &mut DataTree, frame: f32) -> bool {
    migrate_legacy_lattice_tracks(tree);
    let data = animation_data(tree);
    if data.tracks.is_empty() && data.lattice_tracks.is_empty() {
        return false;
    }
    let mut scene = objects(tree);
    let mut cameras = scene_cameras(tree);
    let mut applied = false;
    let mut linked_materials = Vec::new();
    for track in &data.tracks {
        let Some(value) = sample(track, frame) else {
            continue;
        };
        if let Some(object) = scene
            .iter_mut()
            .find(|object| object.uuid == track.binding.object)
        {
            if track
                .binding
                .property
                .value(object)
                .is_some_and(|current| current.to_bits() != value.to_bits())
            {
                track.binding.property.apply(object, value);
                if track.binding.property.is_material() {
                    if let Some(material_id) = object.material_id {
                        linked_materials.push((material_id, object.material));
                    }
                }
                applied = true;
            }
        } else if let Some(camera) = cameras
            .iter_mut()
            .find(|camera| camera.uuid == track.binding.object)
        {
            if track
                .binding
                .property
                .camera_value(camera)
                .is_some_and(|current| current.to_bits() != value.to_bits())
            {
                track.binding.property.apply_camera(camera, value);
                applied = true;
            }
        }
    }
    for (material_id, material) in linked_materials {
        for object in &mut scene {
            if object.material_id == Some(material_id) {
                object.material = material;
                object.color = material.color;
            }
        }
    }
    if applied {
        set_objects_transient(tree, scene);
        tree.set_transient_path("scene.cameras", ClaydashValue::VecCamera(cameras));
    }
    applied
}

pub fn sample(track: &AnimationTrack, frame: f32) -> Option<f32> {
    let first = track.keyframes.first()?;
    if frame <= first.frame as f32 {
        return Some(first.value);
    }
    let last = track.keyframes.last()?;
    if frame >= last.frame as f32 {
        return Some(last.value);
    }
    for pair in track.keyframes.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if frame > right.frame as f32 {
            continue;
        }
        return Some(match left.interpolation {
            KeyframeInterpolation::Constant => left.value,
            KeyframeInterpolation::Linear => {
                let span = (right.frame - left.frame) as f32;
                let amount = (frame - left.frame as f32) / span;
                left.value + (right.value - left.value) * amount
            }
            KeyframeInterpolation::Bezier => {
                let [start, outgoing, incoming, end] = bezier_control_points(left, right);
                let mut low = 0.0;
                let mut high = 1.0;
                for _ in 0..24 {
                    let amount = (low + high) * 0.5;
                    let sampled_frame =
                        cubic_bezier(start.0, outgoing.0, incoming.0, end.0, amount);
                    if sampled_frame < frame {
                        low = amount;
                    } else {
                        high = amount;
                    }
                }
                let amount = (low + high) * 0.5;
                bezier_point([start, outgoing, incoming, end], amount).1
            }
        });
    }
    Some(last.value)
}

#[cfg(test)]
mod tests {
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
}
