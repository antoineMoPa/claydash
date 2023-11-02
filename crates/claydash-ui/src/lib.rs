use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::style::{
    Widgets,
    WidgetVisuals
};
use egui::Color32;
use epaint::Stroke;
use epaint::Rounding;

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
    let ctx = contexts.ctx_mut();
    let rounding = Rounding::same(5.0);

    egui::SidePanel::right("my_left_panel")
        .frame(Frame {
            outer_margin: egui::style::Margin::symmetric(20.0, 0.0),
            inner_margin: egui::style::Margin::same(0.0),
            fill: Color32::TRANSPARENT,
            ..default()
        })
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(320.0);
            let widget_offset = egui::vec2(10.0, 20.0);
            let widget_size = egui::vec2(300.0, 20.0);
            let widget_rect = egui::Rect::from_min_size(
                ui.min_rect().min + widget_offset,
                widget_size
            );

            let mut visuals = ui.visuals().clone();
            visuals.override_text_color = Some(Color32::from_rgb(170, 170, 170));
            let widget_visuals = WidgetVisuals {
                weak_bg_fill: Color32::from_gray(27),
                bg_fill: Color32::from_gray(27),
                bg_stroke: Stroke::new(1.0, Color32::TRANSPARENT), // separators, indentation lines
                fg_stroke: Stroke::new(1.0, Color32::TRANSPARENT),

                rounding: rounding,
                expansion: 10.0,
            };
            visuals.widgets = Widgets {
                noninteractive: widget_visuals.clone(),
                inactive: widget_visuals.clone(),
                hovered: widget_visuals.clone(),
                active: widget_visuals.clone(),
                open: widget_visuals.clone(),
            };
            ctx.set_visuals(visuals);

            let bg_color = Color32::from_rgba_unmultiplied(200, 200, 200, 10);
            ui.style_mut().visuals.extreme_bg_color = bg_color;
            ui.put(
                widget_rect,
                egui::TextEdit::singleline(&mut command_search_state.command_search_str)
                    .hint_text("Search Command...")
            );
            ui.end_row();
            ui.add_space(10.0);

            let command_search_str: &mut String = &mut command_search_state.command_search_str;
            if command_search_str.len() > 0 {
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(200, 200, 200, 10))
                    .rounding(rounding)
                    .outer_margin(egui::style::Margin::symmetric(0.0, 10.0))
                    .inner_margin(egui::style::Margin::symmetric(10.0, 0.0))
                    .show(ui, |ui| {
                        ui.set_width(280.0);
                        command_results_ui(ui, command_search_state);
                    });
            }
        });
}

fn command_results_ui(
    ui: &mut egui::Ui,
    mut command_search_state: ResMut<CommandSearchState>
) {
    let rounding = Rounding::same(5.0);
    let command_search_str: &mut String = &mut command_search_state.command_search_str;
    let commands = match command_search_str.len() {
        0 => { return },
        _ => { command_central::search(command_search_str, 5) }
    };

    for (system_name, command_info) in commands.iter() {
        let bg_color = Color32::from_rgba_unmultiplied(217, 217, 217, 10);

        egui::Frame::none()
            .fill(bg_color)
            .rounding(rounding)
            .inner_margin(egui::style::Margin::symmetric(10.0, 10.0))
            .outer_margin(egui::style::Margin::symmetric(0.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(280.0);
                ui.heading(&command_info.title);
                ui.label(system_name) ;
                ui.separator();
                ui.label(&command_info.docs);
                ui.end_row();

                ui.add_space(10.0);

                if command_info.parameters.len() > 0 {
                    ui.heading("Parameters:");
                }

                for (param_name, param) in command_info.parameters.iter() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                        ui.label(param_name);
                        ui.label(":");
                        ui.label(&param.docs);
                        ui.end_row();
                    });
                }

                ui.set_height(30.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    ui.add_space(10.0);
                    if ui.small_button("Run").clicked() {
                        command_central::run(system_name);
                    }
                });
                ui.add_space(10.0);
            });
    }
}
