use bevy::prelude::*;
use bevy_egui::egui;
use egui::containers::Frame;
use egui::style::{
    Widgets,
    WidgetVisuals
};
use egui::Color32;
use epaint::{
    Stroke,
    Rounding
};
use bevy_command_central_plugin::*;
use claydash_data::{ClaydashData, ClaydashValue};
use observable_key_value_tree::{
    ObservableKVTree,
    SimpleUpdateTracker
};

#[derive(Resource)]
pub struct CommandCentralUiState {
    pub command_search_str: String,
}

impl Default for CommandCentralUiState {
    fn default() -> Self {
        Self {
            command_search_str: "".to_string(),
        }
    }
}

pub fn command_ui(
    ctx: &egui::Context,
    claydash_ui_state: ResMut<CommandCentralUiState>,
    command_central_state: ResMut<CommandCentralState>,
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;

    egui::SidePanel::right("right_panel")
        .frame(Frame {
            outer_margin: egui::style::Margin::symmetric(20.0, 0.0),
            inner_margin: egui::style::Margin::same(0.0),
            fill: Color32::TRANSPARENT,
            ..default()
        })
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(320.0);
            command_search(ui, ctx.clone(), claydash_ui_state, command_central_state, tree);
        });
}

fn command_search(
    ui: &mut egui::Ui,
    ctx: egui::Context,
    mut claydash_ui_state: ResMut<CommandCentralUiState>,
    command_central_state: ResMut<CommandCentralState>,
    tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>,
) {
    let rounding: Rounding = Rounding::same(5.0);
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
        rounding,
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
        egui::TextEdit::singleline(&mut claydash_ui_state.command_search_str)
            .hint_text("Search Commands...")
    );
    ui.end_row();
    ui.add_space(10.0);

    let command_search_str: &mut String = &mut claydash_ui_state.command_search_str;
    if command_search_str.len() > 0 {
        egui::Frame::none()
            .fill(Color32::from_rgba_unmultiplied(200, 200, 200, 10))
            .rounding(rounding)
            .outer_margin(egui::style::Margin::symmetric(0.0, 10.0))
            .inner_margin(egui::style::Margin::symmetric(10.0, 0.0))
            .show(ui, |ui| {
                ui.set_width(280.0);
                command_results_ui(ui, claydash_ui_state, command_central_state, tree);
            });
    }
}

fn command_results_ui(
    ui: &mut egui::Ui,
    mut claydash_ui_state: ResMut<CommandCentralUiState>,
    mut bevy_command_central: ResMut<CommandCentralState>,
    tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
) {
    let rounding = Rounding::same(5.0);
    let command_search_str: &mut String = &mut claydash_ui_state.command_search_str;
    let commands = match command_search_str.len() {
        0 => { return },
        _ => { bevy_command_central.commands.search(command_search_str, 5) }
    };

    for (system_name, command) in commands.iter() {
        let bg_color = Color32::from_rgba_unmultiplied(217, 217, 217, 10);

        egui::Frame::none()
            .fill(bg_color)
            .rounding(rounding)
            .inner_margin(egui::style::Margin::symmetric(10.0, 10.0))
            .outer_margin(egui::style::Margin::symmetric(0.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(280.0);
                ui.heading(&command.title);
                ui.label(system_name) ;
                ui.separator();
                ui.label(&command.docs);
                ui.end_row();

                ui.add_space(10.0);

                if !command.parameters.is_empty() {
                    ui.heading("Parameters:");
                }

                for (param_name, param) in command.parameters.iter() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                        ui.label(param_name);
                        ui.label(":");
                        ui.label(&param.docs);
                        ui.end_row();
                    });
                }

                if !command.shortcut.is_empty() {
                    ui.add_space(10.0);
                    ui.heading("Shortcut:");
                    ui.label(&command.shortcut);
                    ui.end_row();
                }

                ui.set_height(30.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    ui.add_space(10.0);
                    if ui.small_button("Run").clicked() {
                        claydash_ui_state.command_search_str = "".to_string();
                        match command.parameters["callback"].value.clone().unwrap() {
                            ClaydashValue::Fn(callback) => {
                                callback(tree);
                            },
                            _ => {}
                        };
                    }
                });
                ui.add_space(10.0);
            });
    }
}
