mod animation_panel;
mod animation_widgets;
mod boolean_overlay;
mod camera_overlay;
mod camera_panel;
mod file_menu;
mod materials_panel;
mod object_gizmos;
mod object_panel;
pub(crate) mod scene_actions;
mod scene_panel;
mod secondary_panels;
mod selection_tools;
mod status_bar;
mod ui_widgets;
mod viewport_controls;
mod workspace;

use animation_panel::*;
use animation_widgets::*;
pub(crate) use camera_panel::exit_camera_view;
use camera_panel::{sync_camera_view, toggle_camera_view};
use file_menu::*;
use materials_panel::*;
#[cfg(test)]
use object_gizmos::*;
use object_panel::*;
use scene_panel::*;
use secondary_panels::*;
use ui_widgets::*;
#[cfg(test)]
use viewport_controls::*;
use workspace::*;

use egui::{Color32, CornerRadius, RichText, Stroke};
use egui_command_palette::{Command as PaletteCommand, CommandPalette};
use egui_frames::{DropSide, Frames, FramesEvent, FramesStyle, Layout, PaneId, PaneView, Tab};
use glam::{EulerRot, Vec2, Vec3, Vec4};

use crate::{
    animation::{
        self, AnimationRuntime, KeyframeDrag, SelectedKeyframe, TimelineBoxSelection,
        TimelineScrollAxis,
    },
    camera::{Camera, ViewAngle},
    commands::{self, Commands},
    document::{DocumentState, FileMenuAction},
    model::{
        objects, selected, set_objects, set_selected, AnimatableProperty, AnimationBinding,
        AnimationTrack, BooleanOperation, ColorChannel, DataTree, KeyframeInterpolation, Material,
        MaterialKind, PrimitiveKind, SdfObject, SdfParams, VectorAxis, WoodSpecies, WoodStain,
    },
    undo_redo,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum EditorPane {
    Animation,
    Scene,
    Viewport,
    Object,
    Materials,
    Repetition,
    Operand,
}

impl EditorPane {
    fn title(self) -> &'static str {
        match self {
            Self::Animation => "Animation",
            Self::Scene => "Scene",
            Self::Viewport => "Viewport",
            Self::Object => "Object",
            Self::Materials => "Materials",
            Self::Repetition => "Repeat",
            Self::Operand => "Operand",
        }
    }
}

pub struct UiState {
    animation: AnimationRuntime,
    frames: Frames,
    layout: Layout<EditorPane>,
    palette: CommandPalette,
    regions: Vec<egui::Rect>,
    viewport_rect: Option<egui::Rect>,
    ghosts: boolean_overlay::Ghosts,
    selection_tools: selection_tools::SelectionTools,
    active_guide: Option<crate::guides::ActiveGuideSet>,
    resize_guide_drag: Option<ResizeGuideDrag>,
    polygon_cap_drag: Option<PolygonCapDrag>,
    insert_keyframe_menu_position: Option<egui::Pos2>,
    #[cfg(target_arch = "wasm32")]
    web_save_name: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        let mut layout = Layout::with_pane(EditorPane::Viewport);
        layout.add_pane_against_edge(DropSide::Left, 0.30, EditorPane::Scene);
        let inspector = layout.add_pane_against_edge(DropSide::Right, 0.28, EditorPane::Object);
        let right = layout.frame_of(inspector).expect("inspector frame");
        layout.add_pane(right, EditorPane::Materials, None);
        layout.add_pane(right, EditorPane::Repetition, None);
        layout.add_pane(right, EditorPane::Operand, None);
        layout.focus_pane(inspector);

        let mut style = FramesStyle::default();
        style.background = Color32::TRANSPARENT;
        style.frame_fill = Color32::TRANSPARENT;
        style.active_border = Color32::from_rgb(95, 142, 246);
        style.accent = Color32::from_rgb(95, 142, 246);
        Self {
            animation: AnimationRuntime::default(),
            frames: Frames::new().with_style(style),
            layout,
            palette: CommandPalette::default(),
            regions: Vec::new(),
            viewport_rect: None,
            ghosts: boolean_overlay::Ghosts::default(),
            selection_tools: selection_tools::SelectionTools::default(),
            active_guide: None,
            resize_guide_drag: None,
            polygon_cap_drag: None,
            insert_keyframe_menu_position: None,
            #[cfg(target_arch = "wasm32")]
            web_save_name: None,
        }
    }
}

impl UiState {
    pub fn animation_timeline_contains_pointer(
        &self,
        physical_pointer: Vec2,
        pixels_per_point: f32,
    ) -> bool {
        let Some((pane, _)) = self.layout.find_pane(|pane| *pane == EditorPane::Animation) else {
            return false;
        };
        self.frames.pane_rect(pane).is_some_and(|rect| {
            rect.contains(egui::pos2(
                physical_pointer.x / pixels_per_point,
                physical_pointer.y / pixels_per_point,
            ))
        })
    }

    pub fn animation_timeline_open(&self) -> bool {
        self.layout
            .find_pane(|pane| *pane == EditorPane::Animation)
            .is_some()
    }

    pub fn set_animation_timeline_open(&mut self, open: bool) {
        let current = self.layout.find_pane(|pane| *pane == EditorPane::Animation);
        match (open, current) {
            (true, None) => {
                self.layout
                    .add_pane_against_edge(DropSide::Bottom, 0.30, EditorPane::Animation);
            }
            (false, Some((pane, _))) => {
                self.layout.close_pane(pane);
            }
            _ => {}
        }
    }

    pub fn draw(
        &mut self,
        viewport_ui: &mut egui::Ui,
        tree: &mut DataTree,
        command_map: &mut Commands,
        camera: &mut Camera,
        document: &mut DocumentState,
        interaction_guide: Option<crate::guides::ActiveGuideSet>,
    ) -> Option<FileMenuAction> {
        self.regions.clear();
        self.active_guide = None;
        self.ghosts = boolean_overlay::Ghosts::default();
        egui_extras::install_image_loaders(viewport_ui.ctx());
        self.handle_animation_shortcuts(viewport_ui.ctx(), tree);
        self.animation.tick(tree, web_time::Instant::now());
        if self.animation.playing {
            viewport_ui.ctx().request_repaint();
        }
        let file_action = draw_file_menu(viewport_ui, document, &mut self.layout);
        #[cfg(target_arch = "wasm32")]
        let file_action = {
            let mut file_action = file_action;
            if matches!(
                file_action,
                Some(FileMenuAction::Save | FileMenuAction::SaveAs)
            ) {
                self.web_save_name = Some(
                    document
                        .current_path()
                        .and_then(std::path::Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "untitled.claydash".into()),
                );
                file_action = None;
            }
            if let Some(action) = draw_web_save_dialog(viewport_ui.ctx(), &mut self.web_save_name) {
                file_action = Some(action);
            }
            file_action
        };
        self.draw_status_bar(viewport_ui, tree);
        let mut frames = std::mem::take(&mut self.frames);
        let mut layout = std::mem::take(&mut self.layout);
        let mut view = WorkspaceView {
            tree,
            animation: &mut self.animation,
        };
        let events = frames.show(viewport_ui, &mut layout, &mut view);
        for event in events {
            match event {
                FramesEvent::PaneCloseRequested(pane) => {
                    if layout.pane(pane) != Some(&EditorPane::Viewport) {
                        layout.close_pane(pane);
                    }
                }
                FramesEvent::NewTabRequested(frame) => {
                    layout.add_pane(frame, EditorPane::Object, None);
                }
                FramesEvent::TabDoubleClicked(_) => {}
            }
        }
        for frame_id in layout.frame_ids() {
            let active_is_editor_panel = layout
                .frame(frame_id)
                .and_then(egui_frames::Frame::active_pane)
                .and_then(|pane| layout.pane(pane))
                .is_some_and(|kind| *kind != EditorPane::Viewport);
            if active_is_editor_panel {
                if let Some(rect) = frames.frame_rect(frame_id) {
                    self.regions.push(rect);
                }
            }
        }
        self.layout = layout;
        self.frames = frames;
        sync_camera_view(tree, camera);
        self.viewport_rect = self
            .layout
            .find_pane(|pane| *pane == EditorPane::Viewport)
            .and_then(|(pane, _)| self.frames.pane_rect(pane));
        if let Some(rect) = self.viewport_rect {
            let scale = viewport_ui.ctx().pixels_per_point();
            camera.viewport_origin = Vec2::new(rect.left(), rect.top()) * scale;
            camera.viewport = Vec2::new(rect.width().max(1.0), rect.height().max(1.0)) * scale;
            self.draw_top_controls(viewport_ui.ctx(), tree, camera);
            self.draw_view_gizmo(viewport_ui.ctx(), tree, camera);
            viewport_ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.set_clip_rect(rect);
                self.regions.extend(camera_overlay::draw(ui, tree, camera));
                self.ghosts = boolean_overlay::draw(ui, tree, camera);
                // Resize handles must not intercept extrusion or operand-selection clicks.
                if !matches!(
                    tree.get_path("editor.state"),
                    crate::model::ClaydashValue::EditorState(crate::model::EditorState::Extruding)
                ) && scene_actions::pending_boolean(tree).is_none()
                {
                    let object_gizmo_blocker_count = self.regions.len();
                    self.draw_selection_tools(ui, tree, camera);
                    if !self.selection_tools.box_mode() && !self.selection_tools.active() {
                        self.draw_object_gizmos_avoiding(
                            ui,
                            tree,
                            camera,
                            object_gizmo_blocker_count,
                        );
                    }
                }
                if let Some(active) = self.active_guide.or(interaction_guide) {
                    draw_active_guides(ui, camera, active);
                }
            });
        }
        self.draw_insert_keyframe_menu(viewport_ui.ctx(), tree);

        let open_palette = viewport_ui.ctx().input(|input| {
            input.key_pressed(egui::Key::P) && input.modifiers.command && input.modifiers.shift
        });
        if open_palette {
            self.palette.open();
        }
        let palette_commands = command_map
            .commands
            .iter()
            .map(|(id, command)| PaletteCommand {
                id: id.clone(),
                title: command.title.clone(),
                description: command.docs.clone(),
                shortcut: command.shortcut.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(command) = self.palette.show(viewport_ui.ctx(), &palette_commands) {
            commands::execute(command_map, &command, tree);
        }
        if let Some(rect) = self.palette.rect() {
            self.regions.push(rect);
        }
        if let Some(rect) = draw_file_error(viewport_ui.ctx(), document) {
            self.regions.push(rect);
        }
        let commit_edit = viewport_ui.ctx().input(|input| {
            input.pointer.any_released()
                || input.key_pressed(egui::Key::Enter)
                || input.key_pressed(egui::Key::Tab)
        });
        if commit_edit {
            tree.make_undo_redo_snapshot();
        }
        file_action
    }

    pub fn reset_document_gestures(&mut self) {
        self.selection_tools = selection_tools::SelectionTools::default();
        self.ghosts = boolean_overlay::Ghosts::default();
        self.active_guide = None;
        self.resize_guide_drag = None;
        self.insert_keyframe_menu_position = None;
    }

    pub fn reset_animation(&mut self, tree: &DataTree) {
        self.animation.reset_for_document(tree);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn animation_frame(&self) -> f32 {
        self.animation.current_frame
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_animation_frame(&mut self, tree: &mut DataTree, frame: f32) {
        self.animation.playing = false;
        self.animation.set_frame(tree, frame);
    }

    pub fn contains_pointer(&self, physical_position: Vec2, pixels_per_point: f32) -> bool {
        let position = egui::pos2(
            physical_position.x / pixels_per_point,
            physical_position.y / pixels_per_point,
        );
        self.viewport_rect
            .is_none_or(|rect| !rect.contains(position))
            || self.regions.iter().any(|region| region.contains(position))
    }

    pub fn overlay_captures_pointer(&self, ctx: &egui::Context) -> bool {
        egui::Popup::is_any_open(ctx)
    }

    pub fn ghost_at(&self, position: Vec2, pixels_per_point: f32) -> Option<uuid::Uuid> {
        if self.contains_pointer(position, pixels_per_point) {
            return None;
        }
        self.ghosts.pick(egui::pos2(
            position.x / pixels_per_point,
            position.y / pixels_per_point,
        ))
    }
}

struct ResizeGuideDrag {
    object: uuid::Uuid,
    handle: ResizeHandleId,
    initial_transform: crate::model::Transform,
    initial_half_extent: f32,
    initial_face_center: Vec3,
    world_direction_per_unit: Vec3,
    projected_axis: egui::Vec2,
    raw_amount: f32,
    guides: Vec<crate::guides::FaceGuide>,
    active_guide: Option<crate::guides::ActiveGuideSet>,
}

struct PolygonCapDrag {
    object: uuid::Uuid,
    positive: bool,
    initial_transform: crate::model::Transform,
    initial_half_depth: f32,
    split_void: Option<(uuid::Uuid, crate::model::Transform, f32)>,
    projected_axis: egui::Vec2,
    raw_amount: f32,
}

fn draw_active_guides(ui: &mut egui::Ui, camera: &Camera, active: crate::guides::ActiveGuideSet) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let color = Color32::from_rgb(94, 203, 255);
    for guide in active.iter() {
        if let Some([start, end]) =
            crate::guides::screen_segment(camera, guide.guide, pixels_per_point, ui.clip_rect())
        {
            ui.painter()
                .line_segment([start, end], Stroke::new(1.75, color));
        }
        if let Some(anchor) = camera.project(guide.anchor, pixels_per_point) {
            ui.painter().circle_filled(anchor, 4.0, color);
            ui.painter()
                .circle_stroke(anchor, 6.0, Stroke::new(1.25, Color32::WHITE));
        }
    }
}

include!("ui/ui_tests.rs");
