use super::*;

#[derive(Clone, Copy)]
pub(super) struct KeyframeRequest {
    pub(super) binding: AnimationBinding,
    pub(super) value: f32,
}

pub(super) fn animatable_widget(
    ui: &mut egui::Ui,
    tree: &DataTree,
    runtime: &AnimationRuntime,
    binding: AnimationBinding,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let state = animation::field_animation_state(tree, binding, runtime.current_frame);
    ui.scope(|ui| {
        let light = !ui.visuals().dark_mode;
        let fill = match state {
            animation::FieldAnimationState::NotAnimated => None,
            animation::FieldAnimationState::Animated => Some(if light {
                Color32::from_rgb(205, 237, 211)
            } else {
                Color32::from_rgb(45, 104, 57)
            }),
            animation::FieldAnimationState::KeyedAtCurrentFrame => Some(if light {
                Color32::from_rgb(255, 229, 172)
            } else {
                Color32::from_rgb(168, 116, 17)
            }),
        };
        if let Some(fill) = fill {
            let visuals = &mut ui.style_mut().visuals;
            visuals.extreme_bg_color = fill;
            visuals.selection.bg_fill = fill;
            visuals.widgets.inactive.bg_fill = fill;
            visuals.widgets.inactive.weak_bg_fill = fill;
            visuals.widgets.hovered.bg_fill = fill.gamma_multiply(1.16);
            visuals.widgets.hovered.weak_bg_fill = fill.gamma_multiply(1.16);
            visuals.widgets.active.bg_fill = fill.gamma_multiply(0.86);
            visuals.widgets.active.weak_bg_fill = fill.gamma_multiply(0.86);
            visuals.widgets.open.bg_fill = fill.gamma_multiply(1.08);
            visuals.widgets.open.weak_bg_fill = fill.gamma_multiply(1.08);
        }
        add(ui)
    })
    .inner
}

pub(super) fn plain_i_pressed(input: &egui::InputState) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: egui::Key::I,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if modifiers.is_none()
        )
    })
}

pub(super) fn animatable_response(
    ui: &egui::Ui,
    response: &egui::Response,
    binding: AnimationBinding,
    value: f32,
    requests: &mut Vec<KeyframeRequest>,
) {
    response
        .clone()
        .on_hover_text("Press I to insert a keyframe at the current frame");
    if response.hovered() && ui.input(plain_i_pressed) {
        requests.push(KeyframeRequest { binding, value });
    }
}

pub(super) fn apply_keyframe_requests(
    tree: &mut DataTree,
    runtime: &mut AnimationRuntime,
    requests: Vec<KeyframeRequest>,
) {
    if requests.is_empty() {
        return;
    }
    let frame = runtime.current_frame.round().max(0.0) as u32;
    for request in requests {
        animation::insert_keyframe(tree, request.binding, frame, request.value);
        runtime.selected_keyframes = vec![SelectedKeyframe {
            binding: request.binding,
            frame,
        }];
    }
    tree.make_undo_redo_snapshot();
}
