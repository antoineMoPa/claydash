use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::widgets::Label;
use egui::Color32;

pub struct ClaydashUIPlugin;

#[derive(Default,Resource)]
pub struct CommandSearchState {
    pub command_search_str: String,
}

impl Plugin for ClaydashUIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandSearchState>()
            .add_plugins(EguiPlugin)
            .add_systems(Update, command_ui);
    }
}

fn command_ui(
    mut contexts: EguiContexts,
    mut command_search_state: ResMut<CommandSearchState>
) {
    let command_search_str: &mut String = &mut command_search_state.command_search_str;
    let commands = match command_search_str.len() {
        0 => None,
        _ => { Some(command_central::search(command_search_str, 5)) }
    };

    let ctx = contexts.ctx_mut();

    egui::SidePanel::right("my_left_panel")
        .frame(Frame {
            inner_margin: egui::style::Margin {
                left: 10.0,
                right: 10.0,
                bottom: 10.0,
                top: 10.0
            },
            fill: Color32::TRANSPARENT,
            ..default()
        })
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(300.0);
            ui.add(
                egui::TextEdit::singleline(command_search_str)
                    .hint_text("Search Command...")
            );
            ui.end_row();

            match commands {
                Some(commands) => {
                    for (system_name, command_info) in commands.iter() {
                        ui.add(Label::new(&command_info.title));
                        ui.label(system_name);
                        ui.label(&command_info.docs);
                        ui.end_row();
                        if ui.small_button("Run").clicked() {
                            command_central::run(system_name);
                        }
                    }
                },
                _ => {}
            }
        });
}
