use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::style::{
    Widgets,
    WidgetVisuals
};
use egui::Color32;
use epaint::{Stroke, Pos2};
use epaint::Rounding;
use bevy_command_central_plugin::*;

pub struct ClaydashUIPlugin;

#[derive(Default, Resource)]
pub struct ClaydashUIState {
    pub command_search_str: String,
    pub color: Vec4,
}

impl Plugin for ClaydashUIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashUIState>()
            .add_plugins(EguiPlugin)
            .add_systems(Update, command_ui)
            .add_systems(Update, color_picker_egui_ui)
            .add_systems(Startup, color_picker_ui);
    }
}

const IMAGE_WIDTH: f32 = 131.0;
const IMAGE_HEIGHT: f32 = 131.0;
const CIRCLE_MARGIN_LEFT: f32 = 10.0;
const CIRCLE_MARGIN_TOP: f32 = 10.0;
const CIRCLE_CENTER_X: f32 = IMAGE_WIDTH / 2.0 + CIRCLE_MARGIN_LEFT;
const CIRCLE_CENTER_Y: f32 = IMAGE_HEIGHT / 2.0 + CIRCLE_MARGIN_TOP;
const CIRCLE_BORDER_APPROX: f32 = 8.0;
const CIRCLE_USEFUL_RADIUS: f32 = 65.0 - CIRCLE_BORDER_APPROX;

fn command_ui(
    mut contexts: EguiContexts,
    mut claydash_ui_state: ResMut<ClaydashUIState>,
    command_central_state: ResMut<CommandCentralState>
) {
    let ctx = contexts.ctx_mut();
    let rounding = Rounding::same(5.0);

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
                        command_results_ui(ui, claydash_ui_state, command_central_state);
                    });
            }
        });
}

fn command_results_ui(
    ui: &mut egui::Ui,
    mut claydash_ui_state: ResMut<ClaydashUIState>,
    mut bevy_command_central: ResMut<CommandCentralState>
) {
    let rounding = Rounding::same(5.0);
    let command_search_str: &mut String = &mut claydash_ui_state.command_search_str;
    let commands = match command_search_str.len() {
        0 => { return },
        _ => { bevy_command_central.commands.search(command_search_str, 5) }
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
                        bevy_command_central.commands.run(system_name);
                    }
                });
                ui.add_space(10.0);
            });
    }
}

fn color_picker_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    // TODO: activate color picker
    commands.spawn(ImageBundle {
        style: Style {
            width: Val::Px(IMAGE_WIDTH),
            height: Val::Px(IMAGE_HEIGHT),
            margin: UiRect {
                left: Val::Px(CIRCLE_MARGIN_LEFT),
                top: Val::Px(CIRCLE_MARGIN_TOP),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0)
            },
            ..default()
        },
        image: asset_server.load("colorpicker.png").into(),
        ..default()
    });
}

fn color_picker_egui_ui(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    assets: Res<Assets<Image>>,
    mut claydash_ui_state: ResMut<ClaydashUIState>,
) {
    let ctx = contexts.ctx_mut();

    egui::SidePanel::left("left_panel")
        .frame(Frame {
            outer_margin: egui::style::Margin::symmetric(20.0, 0.0),
            inner_margin: egui::style::Margin::same(0.0),
            fill: Color32::TRANSPARENT,
            ..default()
        })
        .show(ctx, |ui| {
            let (pointer_position, any_button_down) = ctx.input(| reader | {
                return (
                    reader.pointer.latest_pos(),
                    reader.pointer.any_down()
                );
            });

            if !any_button_down {
                return;
            }

            match pointer_position {
                Some(pointer_position) => {

                    let distance_from_wheel_center =
                        ((pointer_position.x - CIRCLE_CENTER_X).powi(2) +
                         (pointer_position.y - CIRCLE_CENTER_Y).powi(2)).sqrt();

                    if distance_from_wheel_center > CIRCLE_USEFUL_RADIUS {
                        return;
                    }

                    let image_handle: Handle<Image> = asset_server.load("colorpicker.png");
                    let image = assets.get(&image_handle).unwrap();
                    let index_i_in_image = (pointer_position.x - CIRCLE_MARGIN_LEFT) as i32;
                    let index_j_in_image = (pointer_position.y - CIRCLE_MARGIN_TOP) as i32;
                    let image_size = image.size();
                    let width = image_size.x;
                    let datatype_size = 4; // I assume 4 rgba bytes
                    let line_size = datatype_size * (width as i32);
                    let index_in_image =
                        index_i_in_image * datatype_size +
                        index_j_in_image * line_size;

                    if index_in_image < (image.data.len() as i32 - 4) {
                        let r = image.data[index_in_image as usize + 0];
                        let g = image.data[index_in_image as usize + 1];
                        let b = image.data[index_in_image as usize + 2];
                        let a = image.data[index_in_image as usize + 3];

                        claydash_ui_state.color.x = r as f32 / 255.0;
                        claydash_ui_state.color.y = g as f32 / 255.0;
                        claydash_ui_state.color.z = b as f32 / 255.0;
                        claydash_ui_state.color.w = a as f32 / 255.0;

                        ui.painter()
                            .circle(
                                Pos2 {
                                    x: pointer_position.x,
                                    y: pointer_position.y
                                },
                                6.0,
                                Color32::from_rgba_unmultiplied(r, g, b, a),
                                Stroke {
                                    width: 2.0,
                                    color: Color32::BLACK,
                                }
                            );
                    }
                }
                _ => {}
            }
        });
}
