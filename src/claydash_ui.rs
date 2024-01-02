use bevy::{
    prelude::*,
    winit::WinitWindows,
    tasks::AsyncComputeTaskPool,
};
use crate::command_central_plugin::CommandCentralState;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::Color32;
use epaint::{Stroke, Pos2};
use crate::claydash_data::{ClaydashValue, ClaydashData};
use observable_key_value_tree::{ObservableKVTree};
use crate::command_central_egui::{CommandCentralUiState, command_ui};
use rfd::FileHandle;
use std::sync::mpsc::{channel, Sender, Receiver};

use crate::undo_redo::{UNDO_SHORTCUT, REDO_SHORTCUT};

pub struct ClaydashUIPlugin;

impl Plugin for ClaydashUIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin)
            .init_resource::<CommandCentralUiState>()
            .add_systems(Startup, (setup_messages, color_picker_ui))
            .add_systems(Update, (
                claydash_ui,
                handle_tasks
            ));
    }
}

enum UiMessage {
    SaveFileHandle(FileHandle),
    OpenFileHandle(FileHandle),
    VecU8(Vec<u8>),
}

struct UiMessagesTxRxResource {
    tx: Sender<UiMessage>,
    rx: Receiver<UiMessage>,
}

fn setup_messages(world: &mut World) {
    let (tx, rx) = channel::<UiMessage>();
    let ui_message: UiMessagesTxRxResource = UiMessagesTxRxResource { tx, rx };

    world.insert_non_send_resource(ui_message);
}

fn handle_tasks(
    ui_messages: NonSendMut<UiMessagesTxRxResource>,
    mut data_resource: ResMut<ClaydashData>,
) {
    let tree = &mut data_resource.as_mut().tree;

    match ui_messages.rx.try_recv() {
        Ok(UiMessage::SaveFileHandle(file)) => {
            match serde_json::to_vec(&tree.get_tree("scene")) {
                Ok(serialized_tree) => {
                    let thread_pool = AsyncComputeTaskPool::get();
                    let _task = thread_pool.spawn(async move {
                        let _ = file.write(&serialized_tree).await;
                        println!("Saved file {}", file.file_name());
                    });
                    _task.detach();
                }
                _ => { panic!("Error serializing.") }
            }
        },
        Ok(UiMessage::OpenFileHandle(file)) => {
            let thread_pool = AsyncComputeTaskPool::get();
            let tx = ui_messages.tx.clone();
            let _task = thread_pool.spawn(async move {
                let data = file.read().await;
                _ = tx.send(UiMessage::VecU8(data));
            });
            _task.detach();
        },
        Ok(UiMessage::VecU8(data)) => {
            let tree = &mut data_resource.as_mut().tree;
            let scene: Result<ObservableKVTree<ClaydashValue>, serde_json::Error> = serde_json::from_slice(&data);
            match scene {
                Ok(scene) => {
                    tree.set_tree("scene", scene);
                    println!("Updated tree! {}", tree.path_version("scene"));
                },
                _ => {
                    panic!("could not load data.");
                }
            }
        }
        _ => {}
    }
}

fn claydash_ui(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    assets: Res<Assets<Image>>,
    mut data_resource: ResMut<ClaydashData>,
    claydash_ui_state: ResMut<CommandCentralUiState>,
    command_central_state: ResMut<CommandCentralState>,
    mut _windows: NonSend<WinitWindows>,
    ui_messages: NonSendMut<UiMessagesTxRxResource>
) {
    let tree = &mut data_resource.as_mut().tree;
    let ctx = contexts.ctx_mut();

    use egui::menu;

    egui::TopBottomPanel::top("top_panel")
        .show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Save").clicked() {
                        let task = rfd::AsyncFileDialog::new()
                            .add_filter("claydash workspace", &["claydash"])
                            .save_file();

                        let thread_pool = AsyncComputeTaskPool::get();
                        let tx = ui_messages.tx.clone();
                        let _task = thread_pool.spawn(async move {
                            let file = task.await;
                            _ = tx.send(UiMessage::SaveFileHandle(file.unwrap()));
                        });
                        _task.detach();
                    }
                    if ui.button("Open").clicked() {
                        let task = rfd::AsyncFileDialog::new().pick_file();

                        let thread_pool = AsyncComputeTaskPool::get();
                        let tx = ui_messages.tx.clone();
                        let _task = thread_pool.spawn(async move {
                            let file = task.await;
                            _ = tx.send(UiMessage::OpenFileHandle(file.unwrap()));
                        });
                        _task.detach();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add(
                            egui::Button::new("Undo")
                                .shortcut_text(UNDO_SHORTCUT),
                        )
                        .clicked() {
                        tree.undo();
                    }

                    if ui
                        .add(
                            egui::Button::new("Redo")
                                .shortcut_text(REDO_SHORTCUT),
                        )
                        .clicked() {
                        tree.redo();
                    }
                });
            });
        });

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

    command_ui(ctx, claydash_ui_state, command_central_state, data_resource);
}

const IMAGE_WIDTH: f32 = 66.0;
const IMAGE_HEIGHT: f32 = 66.0;
const CIRCLE_MARGIN_LEFT: f32 = 10.0;
const CIRCLE_MARGIN_TOP: f32 = 35.0;
const CIRCLE_CENTER_X: f32 = IMAGE_WIDTH / 2.0 + CIRCLE_MARGIN_LEFT;
const CIRCLE_CENTER_Y: f32 = IMAGE_HEIGHT / 2.0 + CIRCLE_MARGIN_TOP;
const CIRCLE_BORDER_APPROX: f32 = 4.0;
const CIRCLE_USEFUL_RADIUS: f32 = 32.0 - CIRCLE_BORDER_APPROX;

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

#[inline(always)]
fn draw_color_picker(
    ui: &mut egui::Ui,
    pointer_position: Pos2,
    asset_server: Res<AssetServer>,
    assets: Res<Assets<Image>>,
    tree: &mut ObservableKVTree<ClaydashValue>
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
