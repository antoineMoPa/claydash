use bevy::{
    prelude::*,
    winit::WinitWindows,
    tasks::AsyncComputeTaskPool,
};
use crate::bevy_sdf_object::SDFObject;
use crate::command_central_plugin::CommandCentralState;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use egui::containers::Frame;
use egui::Color32;
use epaint::{Stroke, Pos2};
use crate::claydash_data::{ClaydashValue, ClaydashData};
use observable_key_value_tree::ObservableKVTree;
use crate::command_central_egui::{CommandCentralUiState, command_ui};
use rfd::FileHandle;
use std::sync::mpsc::{channel, Sender, Receiver};

use crate::undo_redo::{UNDO_SHORTCUT, REDO_SHORTCUT};

pub struct ClaydashUIPlugin;

impl Plugin for ClaydashUIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin)
            .init_resource::<CommandCentralUiState>()
            .init_resource::<ObjectGenerationUiState>()
            .init_resource::<SpawnedGeneratedModels>()
            .add_systems(Startup, (setup_messages, color_picker_ui, register_ui_commands))
            .add_systems(Update, (
                claydash_ui,
                handle_tasks,
                add_picking_to_generated_models,
                spawn_loaded_generated_models
            ));
    }
}

enum UiMessage {
    SaveFileHandle(FileHandle),
    OpenFileHandle(FileHandle),
    VecU8(Vec<u8>),
    GenerationComplete(Result<crate::object_generation::GeneratedObject, String>),
    SpawnModel { asset_path: String, prompt: Option<String> }, // Model path and optional prompt
}

#[derive(Resource, Default)]
struct ObjectGenerationUiState {
    show_dialog: bool,
    prompt: String,
    is_generating: bool,
    last_error: Option<String>,
}

/// Marker component for generated models
#[derive(Component)]
struct GeneratedModelMarker;

/// Resource to track which generated models have been spawned from the scene tree
#[derive(Resource, Default)]
struct SpawnedGeneratedModels {
    /// Set of UUIDs that have been spawned
    spawned: std::collections::HashSet<uuid::Uuid>,
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
    mut gen_ui_state: ResMut<ObjectGenerationUiState>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let tree = &mut data_resource.as_mut().tree;

    match ui_messages.rx.try_recv() {
        Ok(UiMessage::SaveFileHandle(file)) => {
            let scene_tree_opt = tree.get_tree("scene");

            match scene_tree_opt {
                Some(scene_tree) => {
                    // Log what we're trying to serialize for debugging
                    match serde_json::to_string_pretty(&scene_tree) {
                        Ok(json_str) => {
                            eprintln!("📝 Saving scene ({} chars):", json_str.len());
                            if json_str.len() < 1000 {
                                eprintln!("{}", json_str);
                            } else {
                                eprintln!("{}...", &json_str[..1000]);
                            }
                        }
                        Err(e) => {
                            eprintln!("⚠️  Warning: Could not preview JSON: {}", e);
                        }
                    }

                    match serde_json::to_vec(&scene_tree) {
                        Ok(serialized_tree) => {
                            eprintln!("✅ Serialized {} bytes", serialized_tree.len());
                            let thread_pool = AsyncComputeTaskPool::get();
                            let _task = thread_pool.spawn(async move {
                                let _ = file.write(&serialized_tree).await;
                                println!("💾 Saved file: {}", file.file_name());
                            });
                            _task.detach();
                        }
                        Err(e) => {
                            eprintln!("❌ Error serializing scene: {}", e);
                            eprintln!("   This should not happen - scene tree exists but can't serialize");
                        }
                    }
                }
                None => {
                    eprintln!("⚠️  No scene tree found! Creating empty scene...");
                    // Create a minimal scene
                    let mut empty_scene = ObservableKVTree::<ClaydashValue>::default();
                    empty_scene.set_path("sdf_objects", ClaydashValue::VecSDFObject(Vec::new()));
                    empty_scene.set_path("selected_uuids", ClaydashValue::VecUuid(Vec::new()));

                    match serde_json::to_vec(&empty_scene) {
                        Ok(serialized_tree) => {
                            eprintln!("✅ Serialized empty scene: {} bytes", serialized_tree.len());
                            let thread_pool = AsyncComputeTaskPool::get();
                            let _task = thread_pool.spawn(async move {
                                let _ = file.write(&serialized_tree).await;
                                println!("💾 Saved empty scene: {}", file.file_name());
                            });
                            _task.detach();
                        }
                        Err(e) => {
                            eprintln!("❌ Error serializing empty scene: {}", e);
                        }
                    }
                }
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
                    println!("✅ Scene loaded successfully! Version: {}", tree.path_version("scene"));
                    eprintln!("✅ Scene loaded successfully! Version: {}", tree.path_version("scene"));
                },
                Err(e) => {
                    error!("❌ Failed to load scene: {}", e);
                    eprintln!("❌ Failed to load scene: {}", e);
                    eprintln!("   This may be due to incompatible file format or corrupted data.");
                    eprintln!("   Error details: {:?}", e);
                    // Don't panic - just show error
                }
            }
        },
        Ok(UiMessage::GenerationComplete(result)) => {
            gen_ui_state.is_generating = false;
            match result {
                Ok(obj) => {
                    info!("✅ Object generation succeeded: {}", obj.filename);
                    eprintln!("✅ Object generation succeeded: {}", obj.filename);
                    eprintln!("   Image URL: {}", obj.image_url);
                    eprintln!("   Model size: {} bytes", obj.model_data.len());

                    // Save model to assets/generated_models/ (native only)
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let models_dir = std::path::Path::new("assets/generated_models");
                        if let Err(e) = std::fs::create_dir_all(models_dir) {
                            error!("Failed to create models directory: {}", e);
                            gen_ui_state.last_error = Some(format!("Failed to create directory: {}", e));
                            return;
                        }

                        let model_path = models_dir.join(&obj.filename);
                        if let Err(e) = std::fs::write(&model_path, &obj.model_data) {
                            error!("Failed to write model file: {}", e);
                            gen_ui_state.last_error = Some(format!("Failed to save model: {}", e));
                            return;
                        }

                        eprintln!("💾 Saved model to: {:?}", model_path);

                        // Send message to spawn the model
                        let asset_path = format!("generated_models/{}", obj.filename);
                        // TODO: Store and pass the actual prompt
                        let _ = ui_messages.tx.send(UiMessage::SpawnModel {
                            asset_path,
                            prompt: None
                        });
                    }

                    #[cfg(target_arch = "wasm32")]
                    {
                        // For WASM, we'd need to use IndexedDB or similar
                        // For now, just log that WASM is not yet supported
                        error!("Model loading in WASM not yet implemented");
                        gen_ui_state.last_error = Some("WASM model loading not yet supported".to_string());
                    }

                    gen_ui_state.show_dialog = false;
                    gen_ui_state.prompt.clear();
                    gen_ui_state.last_error = None;
                },
                Err(error) => {
                    error!("❌ Object generation failed: {}", error);
                    eprintln!("❌ Object generation failed: {}", error);
                    gen_ui_state.last_error = Some(error);
                }
            }
        },
        Ok(UiMessage::SpawnModel { asset_path, prompt }) => {
            use crate::claydash_data::SceneObject;
            use crate::interactions::ModelEntity;

            eprintln!("🎮 Spawning model in scene: {}", asset_path);

            let transform = Transform::from_xyz(0.0, 0.5, 0.0);
            let uuid = uuid::Uuid::new_v4();

            // Load and spawn the GLB model - we'll add picking in a separate system
            commands.spawn((
                SceneBundle {
                    scene: asset_server.load(format!("{}#Scene0", &asset_path)),
                    transform,
                    ..default()
                },
                GeneratedModelMarker, // Mark this as a generated model
                ModelEntity { uuid }, // Track UUID for interactions
            ));

            // Add to scene tree
            let scene_object = SceneObject::Model {
                uuid,
                asset_path: asset_path.clone(),
                transform,
                prompt,
            };

            // Get current scene objects or create empty list
            let tree = &mut data_resource.as_mut().tree;
            let mut objects = tree.get_path("scene.objects")
                .unwrap_vec_scene_object_or(Vec::new());

            objects.push(scene_object);
            tree.set_path("scene.objects", ClaydashValue::VecSceneObject(objects));

            info!("✅ Model spawned and added to scene tree");
            eprintln!("✅ Model spawned and added to scene tree (UUID: {})", uuid);
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
    ui_messages: NonSendMut<UiMessagesTxRxResource>,
    mut gen_ui_state: ResMut<ObjectGenerationUiState>,
    generation: Res<crate::object_generation::ObjectGeneration>,
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
                ui.menu_button("Generate", |ui| {
                    if ui.button("Generate 3D Object...").clicked() {
                        gen_ui_state.show_dialog = true;
                        ui.close_menu();
                    }
                });
            });
        });

    // Check if command requested to open the generation dialog
    if let ClaydashValue::Bool(true) = tree.get_path("editor.show_generation_dialog") {
        gen_ui_state.show_dialog = true;
        tree.set_path("editor.show_generation_dialog", ClaydashValue::Bool(false));
    }

    // Object generation dialog
    if gen_ui_state.show_dialog {
        egui::Window::new("Generate 3D Object")
            .collapsible(false)
            .resizable(false)
            .default_size([500.0, 280.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 14.0);
                ui.spacing_mut().button_padding = egui::vec2(16.0, 8.0);

                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label("Enter a text prompt to generate a 3D object:");
                    });

                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.add_sized(
                            [456.0, 70.0],
                            egui::TextEdit::multiline(&mut gen_ui_state.prompt)
                                .hint_text("e.g., a red sports car, a wooden chair...")
                                .margin(egui::vec2(8.0, 8.0))
                        );
                        ui.add_space(12.0);
                    });

                    ui.add_space(14.0);

                    if let Some(error) = &gen_ui_state.last_error {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.colored_label(Color32::RED, format!("Error: {}", error));
                        });
                        ui.add_space(8.0);
                    }

                    if gen_ui_state.is_generating {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.colored_label(Color32::from_rgb(255, 200, 0), "Generating... This may take 30-60 seconds.");
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.spinner();
                            ui.add_space(8.0);
                            ui.label("Please wait...");
                        });
                    }

                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let generate_button = ui.add_enabled(
                            !gen_ui_state.is_generating,
                            egui::Button::new("Generate")
                        );

                        if generate_button.clicked() {
                            if !gen_ui_state.prompt.is_empty() {
                                let prompt = gen_ui_state.prompt.clone();
                                let service = generation.service.clone();
                                let tx = ui_messages.tx.clone();

                                gen_ui_state.is_generating = true;
                                gen_ui_state.last_error = None;

                                // Spawn generation task
                                #[cfg(target_arch = "wasm32")]
                                {
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let result = service.generate_object(&prompt).await;
                                        let msg = match result {
                                            Ok(obj) => UiMessage::GenerationComplete(Ok(obj)),
                                            Err(e) => UiMessage::GenerationComplete(Err(e.to_string())),
                                        };
                                        let _ = tx.send(msg);
                                    });
                                }

                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    // Spawn in a separate thread with its own Tokio runtime
                                    std::thread::spawn(move || {
                                        let runtime = tokio::runtime::Runtime::new().unwrap();
                                        runtime.block_on(async {
                                            let result = service.generate_object(&prompt).await;
                                            let msg = match result {
                                                Ok(obj) => UiMessage::GenerationComplete(Ok(obj)),
                                                Err(e) => UiMessage::GenerationComplete(Err(e.to_string())),
                                            };
                                            let _ = tx.send(msg);
                                        });
                                    });
                                }
                            } else {
                                gen_ui_state.last_error = Some("Please enter a prompt".to_string());
                            }
                        }

                        ui.add_space(8.0);

                        let cancel_button = ui.add_enabled(
                            !gen_ui_state.is_generating,
                            egui::Button::new("Cancel")
                        );

                        if cancel_button.clicked() {
                            gen_ui_state.show_dialog = false;
                            gen_ui_state.prompt.clear();
                            gen_ui_state.last_error = None;
                        }
                    });
                });

                ui.add_space(8.0);
            });
    }

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
        update_selection_color(tree, color);
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

fn update_selection_color(
    tree: &mut ObservableKVTree<ClaydashValue>,
    color: Vec4,
) {
    let mut objects: Vec<SDFObject> = match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(data) => data,
        _ => { return; }
    };

    let selected_object_uuids = tree.get_path("scene.selected_uuids").unwrap_vec_uuid_or(Vec::new());

    for object in objects.iter_mut() {
        if selected_object_uuids.contains(&object.uuid) {
            object.color = color;
        }
    }

    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(objects));
}

/// System to add picking support to all meshes in generated models
fn add_picking_to_generated_models(
    mut commands: Commands,
    generated_models: Query<(Entity, &crate::interactions::ModelEntity), With<GeneratedModelMarker>>,
    meshes_without_picking: Query<Entity, (With<Handle<Mesh>>, Without<bevy_mod_picking::prelude::Pickable>)>,
    all_children: Query<&Children>,
    // Add query to verify picking was added
    meshes_with_picking: Query<Entity, (With<Handle<Mesh>>, With<bevy_mod_picking::prelude::Pickable>)>,
    mesh_query: Query<(&GlobalTransform, Option<&bevy::render::primitives::Aabb>), With<Handle<Mesh>>>,
) {
    // Early return if no models with marker
    if generated_models.is_empty() {
        return;
    }

    for (model_entity, model_component) in generated_models.iter() {
        // Check if the scene has loaded (has children)
        if let Ok(children) = all_children.get(model_entity) {
            eprintln!("🔍 Adding picking to model {} (entity {:?}) with {} children", model_component.uuid, model_entity, children.len());

            let mut mesh_count = 0;
            // Find all mesh entities in the scene hierarchy and copy the ModelEntity component to them
            for &child in children.iter() {
                mesh_count += add_picking_to_entity_recursive(&mut commands, child, &meshes_without_picking, &all_children, model_component, 0);
            }

            // Remove the marker once we've processed it
            commands.entity(model_entity).remove::<GeneratedModelMarker>();
            if mesh_count > 0 {
                eprintln!("✅ Added picking support to {} mesh(es)", mesh_count);

                // Verify picking was added and log mesh bounds
                eprintln!("🔍 Verifying: {} mesh(es) now have Pickable component", meshes_with_picking.iter().count());

                // Log mesh bounds for debugging
                for mesh_entity in meshes_with_picking.iter() {
                    if let Ok((global_transform, aabb)) = mesh_query.get(mesh_entity) {
                        eprintln!("  📦 Mesh {:?} - Position: {:?}", mesh_entity, global_transform.translation());
                        if let Some(aabb) = aabb {
                            eprintln!("       AABB center: {:?}, half_extents: {:?}", aabb.center, aabb.half_extents);
                        } else {
                            eprintln!("       ⚠️  No AABB!");
                        }
                    }
                }
            }
        }
        // If scene not loaded yet (no children), keep the marker and try again next frame
    }
}

/// Recursively add picking to mesh entities
fn add_picking_to_entity_recursive(
    commands: &mut Commands,
    entity: Entity,
    meshes_without_picking: &Query<Entity, (With<Handle<Mesh>>, Without<bevy_mod_picking::prelude::Pickable>)>,
    all_children: &Query<&Children>,
    model_component: &crate::interactions::ModelEntity,
    depth: usize,
) -> usize {
    use bevy_mod_picking::prelude::*;

    let mut count = 0;

    // If this entity has a mesh and doesn't have picking, add it
    if meshes_without_picking.contains(entity) {
        eprintln!("  📌 Adding picking + ModelEntity to mesh entity {:?} (UUID: {})", entity, model_component.uuid);

        commands.entity(entity).insert((
            PickableBundle::default(), // Use bundle which includes RaycastPickable backend
            On::<Pointer<Down>>::run(crate::interactions::on_mouse_down),
            On::<Pointer<Up>>::run(crate::interactions::on_mouse_up),
            model_component.clone(), // Add the ModelEntity component so clicks can find it
        ));
        count += 1;
    }

    // Recursively process children
    if let Ok(children) = all_children.get(entity) {
        eprintln!("  🔽 Entity {:?} has {} children", entity, children.len());
        for &child in children.iter() {
            count += add_picking_to_entity_recursive(commands, child, meshes_without_picking, all_children, model_component, depth + 1);
        }
    }

    count
}

/// Register UI commands
fn register_ui_commands(mut bevy_command_central: ResMut<CommandCentralState>) {
    use command_central::CommandBuilder;

    let commands = &mut bevy_command_central.commands;

    CommandBuilder::new()
        .title("Generate 3D Object")
        .system_name("generate-object")
        .docs("Open the object generation dialog to create a 3D model from a text prompt using AI.")
        .insert_param("callback", "system callback", Some(ClaydashValue::Fn(open_generation_dialog)))
        .write(commands);
}

/// Command callback to open the generation dialog
fn open_generation_dialog(tree: &mut ObservableKVTree<ClaydashValue>) {
    eprintln!("🎨 Opening object generation dialog via command...");
    // Set a flag that the UI will check
    tree.set_path("editor.show_generation_dialog", ClaydashValue::Bool(true));
}

/// System to spawn generated models from loaded scene tree
fn spawn_loaded_generated_models(
    mut commands: Commands,
    mut data_resource: ResMut<ClaydashData>,
    mut spawned_tracker: ResMut<SpawnedGeneratedModels>,
    asset_server: Res<AssetServer>,
) {
    use crate::claydash_data::SceneObject;
    use crate::interactions::ModelEntity;

    let tree = &mut data_resource.as_mut().tree;

    // Get scene objects
    let objects = tree.get_path("scene.objects").unwrap_vec_scene_object_or(Vec::new());

    if objects.is_empty() {
        return; // No objects to spawn
    }

    // Check if there are any unspawned models before logging
    let mut has_unspawned = false;
    for scene_object in objects.iter() {
        if let SceneObject::Model { uuid, .. } = scene_object {
            if !spawned_tracker.spawned.contains(uuid) {
                has_unspawned = true;
                break;
            }
        }
    }

    if !has_unspawned {
        return; // All models already spawned
    }

    let mut spawned_count = 0;
    for scene_object in objects.iter() {
        // Only process Model objects
        if let SceneObject::Model { uuid, asset_path, transform, .. } = scene_object {
            // Skip if already spawned
            if spawned_tracker.spawned.contains(uuid) {
                continue;
            }

            eprintln!("🎮 Loading model from scene: {} (UUID: {})", asset_path, uuid);

            // Spawn the model
            commands.spawn((
                SceneBundle {
                    scene: asset_server.load(format!("{}#Scene0", asset_path)),
                    transform: *transform,
                    ..default()
                },
                GeneratedModelMarker,
                ModelEntity { uuid: *uuid }, // Track UUID for interactions
            ));

            // Mark as spawned
            spawned_tracker.spawned.insert(*uuid);
            spawned_count += 1;
        }
    }

    if spawned_count > 0 {
        eprintln!("✅ Spawned {} model(s) from loaded scene", spawned_count);
    }
}
