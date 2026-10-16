/// Cancel pointer ownership when the window cannot guarantee button releases.
/// egui deliberately retains pressed buttons on PointerGone and focus loss.
pub(super) fn release_interrupted_pointer(ctx: &egui::Context, input: &mut egui::RawInput) {
    let Some(last_interruption) = input.events.iter().rposition(|event| {
        matches!(
            event,
            egui::Event::PointerGone | egui::Event::WindowFocused(false)
        )
    }) else {
        return;
    };
    // Discard the interrupted pointer sequence, preserving later reentry events.
    // Reset state instead of synthesizing a release/move: those can click a
    // widget or apply an unwanted final delta to a dragged numeric property.
    let mut index = 0;
    input.events.retain(|event| {
        let keep = index > last_interruption
            || !matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::MouseMoved(_)
            );
        index += 1;
        keep
    });
    ctx.stop_dragging();
    ctx.input_mut(|state| state.pointer = egui::PointerState::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) {
        let mut input = egui::RawInput {
            events,
            ..Default::default()
        };
        release_interrupted_pointer(ctx, &mut input);
        let mut output = ctx.run_ui(input, |_| {});
        output.textures_delta.clear();
    }
    fn button(button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(100.0, 100.0),
            button,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }
    #[test]
    fn interrupted_pointer_releases_buttons_without_clicking_and_allows_reentry() {
        for interruption in [egui::Event::PointerGone, egui::Event::WindowFocused(false)] {
            for pressed in [
                egui::PointerButton::Primary,
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
                egui::PointerButton::Extra1,
                egui::PointerButton::Extra2,
            ] {
                let ctx = egui::Context::default();
                let camera = claydash_engine::Camera::new();
                let mut tree = Default::default();
                let mut interactions = crate::interactions::InteractionState::default();
                interactions.set_right_button(true, false, &camera, &tree);
                frame(
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(100.0, 100.0)),
                        button(pressed, true),
                    ],
                );
                assert!(ctx.input(|i| i.pointer.any_down()));
                frame(&ctx, vec![interruption.clone()]);
                interactions.interrupt_navigation(&mut tree);
                assert!(!interactions
                    .viewport_interaction_active(ctx.input(|i| i.pointer.any_down()), &tree));
                assert!(
                    !ctx.input(|i| i.pointer.any_down()),
                    "{interruption:?}: {pressed:?}"
                );
                assert!(!ctx.input(|i| i.pointer.any_click()));
                frame(
                    &ctx,
                    vec![
                        egui::Event::WindowFocused(true),
                        egui::Event::PointerMoved(egui::pos2(100.0, 100.0)),
                        button(pressed, true),
                    ],
                );
                assert!(ctx.input(|i| i.pointer.button_down(pressed)));
                frame(&ctx, vec![button(pressed, false)]);
                assert!(!ctx.input(|i| i.pointer.any_down()));
            }
        }
    }
    #[test]
    fn every_held_button_subset_is_cancelled() {
        let buttons = [
            egui::PointerButton::Primary,
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
            egui::PointerButton::Extra1,
            egui::PointerButton::Extra2,
        ];
        for subset in 0..32 {
            for interruption in [egui::Event::PointerGone, egui::Event::WindowFocused(false)] {
                let ctx = egui::Context::default();
                frame(
                    &ctx,
                    buttons
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| subset & (1 << i) != 0)
                        .map(|(_, &b)| button(b, true))
                        .collect(),
                );
                frame(
                    &ctx,
                    vec![
                        interruption.clone(),
                        button(egui::PointerButton::Primary, true),
                        interruption,
                    ],
                );
                assert!(
                    !ctx.input(|i| i.pointer.any_down() || i.pointer.any_click()),
                    "subset {subset}"
                );
            }
        }
    }

    #[test]
    fn reentry_press_in_same_frame_survives_interruption() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![button(egui::PointerButton::Secondary, true)]);
        frame(
            &ctx,
            vec![
                egui::Event::PointerGone,
                egui::Event::PointerMoved(egui::pos2(100.0, 100.0)),
                button(egui::PointerButton::Primary, true),
            ],
        );
        assert!(!ctx.input(|i| i.pointer.secondary_down()));
        assert!(ctx.input(|i| i.pointer.primary_down()));
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;
    #[test]
    fn interrupted_widget_drag_preserves_value() {
        for interruption in [egui::Event::PointerGone, egui::Event::WindowFocused(false)] {
            let ctx = egui::Context::default();
            let mut value = 10.0_f64;
            let mut rect = egui::Rect::NOTHING;
            let mut frame = |events| {
                let mut input = egui::RawInput {
                    events,
                    ..Default::default()
                };
                release_interrupted_pointer(&ctx, &mut input);
                let mut output = ctx.run_ui(input, |ui| {
                    rect = ui.add(egui::DragValue::new(&mut value)).rect;
                });
                output.textures_delta.clear();
                (rect, value)
            };
            let (rect, _) = frame(vec![]);
            let pos = rect.center();
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ]);
            frame(vec![egui::Event::PointerMoved(pos + egui::vec2(30.0, 0.0))]);
            let (_, before) = frame(vec![egui::Event::PointerMoved(pos + egui::vec2(40.0, 0.0))]);
            assert!(ctx.dragged_id().is_some());
            let (_, after) = frame(vec![interruption]);
            assert_eq!(after, before);
            assert!(!ctx.input(|i| i.pointer.any_down() || i.pointer.any_click()));
            assert!(ctx.dragged_id().is_none());
        }
    }
}
