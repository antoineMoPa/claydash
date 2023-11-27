use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::Color32;
use epaint::{Stroke, Pos2};
use claydash_data::{ClaydashValue, ClaydashData};
use observable_key_value_tree::{ObservableKVTree, SimpleUpdateTracker};

pub struct ClaydashUIPlugin;

#[derive(Resource)]
pub struct ClaydashUIState {
    pub color: Vec4,
}

impl Default for ClaydashUIState {
    fn default() -> Self {
        Self {
            color: Vec4::new(0.2, 0.1, 1.0, 1.0),
        }
    }
}

impl Plugin for ClaydashUIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClaydashUIState>()
            .add_plugins((
                EguiPlugin,
            ))
            .add_systems(Update, left_panel_ui)
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

fn color_picker_ui(
    mut commands: Commands,
    mut data_resource: ResMut<ClaydashData>,
    asset_server: Res<AssetServer>,
) {
    // Set initial color
    let tree = &mut data_resource.as_mut().tree;
    tree.set_path("editor.colorpicker.color", ClaydashValue::Vec4(Vec4::new(0.8, 0.0, 0.3, 1.0)));
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

fn left_panel_ui(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    assets: Res<Assets<Image>>,
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;
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
                    draw_color_picker(
                        ui,
                        pointer_position,
                        asset_server,
                        assets,
                        tree
                    )
                }
                _ => {}
            }
        });
}

#[inline(always)]
fn draw_color_picker(
    ui: &mut egui::Ui,
    pointer_position: Pos2,
    asset_server: Res<AssetServer>,
    assets: Res<Assets<Image>>,
    tree: &mut ObservableKVTree<ClaydashValue, SimpleUpdateTracker>
) {
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
        let color = Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );
        tree.set_path("editor.colorpicker.color", ClaydashValue::Vec4(color));
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
