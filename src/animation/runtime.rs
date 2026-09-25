use super::*;

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
    pub(super) last_tick: Option<Instant>,
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
