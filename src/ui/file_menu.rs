use super::*;

pub(super) enum CursorMenuAction {
    Reset,
    SetPosition,
    ToObjectCenter,
}

pub(super) fn draw_file_menu(
    viewport_ui: &mut egui::Ui,
    document: &DocumentState,
    layout: &mut Layout<EditorPane>,
    cursor_target_available: bool,
    placing_cursor: bool,
) -> (Option<FileMenuAction>, Option<CursorMenuAction>) {
    let mut action = None;
    let mut cursor_action = None;
    egui::Panel::top("file-menu").show(viewport_ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Open…").shortcut_text("Cmd/Ctrl+O"))
                    .clicked()
                {
                    action = Some(FileMenuAction::Open);
                    ui.close();
                }
                ui.menu_button("Open Recent", |ui| {
                    if document.recent_paths().is_empty() {
                        ui.add_enabled(false, egui::Button::new("No Recent Projects"));
                    }
                    for path in document.recent_paths() {
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy())
                            .unwrap_or_else(|| path.as_os_str().to_string_lossy());
                        if ui
                            .button(name)
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            action = Some(FileMenuAction::OpenRecent(path.clone()));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text("Cmd/Ctrl+S"))
                    .clicked()
                {
                    action = Some(FileMenuAction::Save);
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Save As…").shortcut_text("Cmd/Ctrl+Shift+S"))
                    .clicked()
                {
                    action = Some(FileMenuAction::SaveAs);
                    ui.close();
                }
            });
            ui.menu_button("Panels", |ui| {
                let animation_open = layout
                    .find_pane(|pane| *pane == EditorPane::Animation)
                    .is_some();
                if ui
                    .add_enabled(!animation_open, egui::Button::new("Animation Timeline"))
                    .clicked()
                {
                    layout.add_pane_against_edge(DropSide::Bottom, 0.30, EditorPane::Animation);
                    ui.close();
                }
                if animation_open {
                    ui.weak("Animation Timeline is open");
                }
            });
            ui.menu_button("Tool", |ui| {
                ui.menu_button("Cursor", |ui| {
                    if ui.button("Reset 3D cursor").clicked() {
                        cursor_action = Some(CursorMenuAction::Reset);
                        ui.close();
                    }
                    if ui
                        .selectable_label(placing_cursor, "Set position")
                        .clicked()
                    {
                        cursor_action = Some(CursorMenuAction::SetPosition);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            cursor_target_available,
                            egui::Button::new("Cursor to object center"),
                        )
                        .clicked()
                    {
                        cursor_action = Some(CursorMenuAction::ToObjectCenter);
                        ui.close();
                    }
                });
            });
            ui.menu_button("Render", |ui| {
                for format in crate::document::RenderFormat::ALL {
                    if ui.button(format.label()).clicked() {
                        action = Some(FileMenuAction::Render(format));
                        ui.close();
                    }
                }
            });
            if let Some(path) = document.current_path() {
                ui.separator();
                ui.weak(
                    path.file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_else(|| path.as_os_str().to_string_lossy()),
                );
            }
        });
    });

    let open = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
    let save = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
    let save_as = egui::KeyboardShortcut::new(
        egui::Modifiers {
            shift: true,
            command: true,
            ..Default::default()
        },
        egui::Key::S,
    );
    viewport_ui.ctx().input_mut(|input| {
        if input.consume_shortcut(&open) {
            action = Some(FileMenuAction::Open);
        } else if input.consume_shortcut(&save_as) {
            action = Some(FileMenuAction::SaveAs);
        } else if input.consume_shortcut(&save) {
            action = Some(FileMenuAction::Save);
        }
    });
    (action, cursor_action)
}

pub(super) fn draw_file_error(
    ctx: &egui::Context,
    document: &mut DocumentState,
) -> Option<egui::Rect> {
    let message = document.error().map(str::to_owned)?;
    let mut open = true;
    let mut dismiss = false;
    let response = egui::Window::new("File error")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add(egui::Label::new(&message).selectable(true));
            ui.horizontal(|ui| {
                if ui.button("Copy error").clicked() {
                    ui.ctx().copy_text(message.clone());
                }
                dismiss = ui.button("Dismiss").clicked();
            });
        });
    let copy = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::C);
    if ctx.input_mut(|input| input.consume_shortcut(&copy)) {
        ctx.copy_text(message);
    }
    if dismiss || !open {
        document.clear_error();
    }
    response.map(|response| response.response.rect)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn draw_web_save_dialog(
    ctx: &egui::Context,
    file_name: &mut Option<String>,
) -> Option<FileMenuAction> {
    let mut action = None;
    let Some(name) = file_name.as_mut() else {
        return None;
    };
    let mut cancel = false;
    let modal = egui::Modal::new("web-project-download".into()).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Download project");
        ui.label("Choose a name for the Claydash project.");
        let response = ui.add(
            egui::TextEdit::singleline(name)
                .desired_width(f32::INFINITY)
                .hint_text("untitled.claydash"),
        );
        if !response.has_focus() {
            response.request_focus();
        }
        let confirm = ui.input(|input| input.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            cancel = ui.button("Cancel").clicked();
            let valid = !name.trim().is_empty();
            if ui
                .add_enabled(valid, egui::Button::new("Download"))
                .clicked()
                || (valid && confirm)
            {
                action = Some(FileMenuAction::SaveNamed(project_download_name(name)));
            }
        });
    });
    if cancel || modal.should_close() || action.is_some() {
        *file_name = None;
    }
    action
}

#[cfg(target_arch = "wasm32")]
fn project_download_name(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.to_ascii_lowercase().ends_with(".claydash") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.claydash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_c_copies_the_complete_file_error() {
        let ctx = egui::Context::default();
        let mut document = DocumentState::default();
        document.set_error("encode the animation", "missing codec details");
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::C,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |_ui| {
                draw_file_error(&ctx, &mut document);
            },
        );
        let commands = output.platform_output.commands.clone();
        output.textures_delta.clear();
        assert!(commands.contains(&egui::OutputCommand::CopyText(
            "Could not encode the animation: missing codec details".to_owned()
        )));
    }
}
