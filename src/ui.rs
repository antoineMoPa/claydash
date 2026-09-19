mod boolean_overlay;
pub(crate) mod scene_actions;
mod selection_tools;

use egui::{Color32, CornerRadius, RichText, Stroke};
use egui_command_palette::{Command as PaletteCommand, CommandPalette};
use egui_frames::{DropSide, Frames, FramesEvent, FramesStyle, Layout, PaneId, PaneView, Tab};
use glam::{EulerRot, Vec2, Vec3, Vec4};

use crate::{
    camera::{Camera, ViewAngle},
    commands::{self, Commands},
    document::{DocumentState, FileMenuAction},
    model::{
        objects, selected, set_objects, set_selected, BooleanOperation, DataTree, Material,
        MaterialKind, PrimitiveKind, SdfObject, SdfParams,
    },
    undo_redo,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum EditorPane {
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
    frames: Frames,
    layout: Layout<EditorPane>,
    palette: CommandPalette,
    regions: Vec<egui::Rect>,
    viewport_rect: Option<egui::Rect>,
    ghosts: boolean_overlay::Ghosts,
    selection_tools: selection_tools::SelectionTools,
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
            frames: Frames::new().with_style(style),
            layout,
            palette: CommandPalette::default(),
            regions: Vec::new(),
            viewport_rect: None,
            ghosts: boolean_overlay::Ghosts::default(),
            selection_tools: selection_tools::SelectionTools::default(),
        }
    }
}

impl UiState {
    pub fn draw(
        &mut self,
        viewport_ui: &mut egui::Ui,
        tree: &mut DataTree,
        command_map: &mut Commands,
        camera: &mut Camera,
        document: &mut DocumentState,
    ) -> Option<FileMenuAction> {
        self.regions.clear();
        self.ghosts = boolean_overlay::Ghosts::default();
        egui_extras::install_image_loaders(viewport_ui.ctx());
        let file_action = draw_file_menu(viewport_ui, document);
        let mut frames = std::mem::take(&mut self.frames);
        let mut layout = std::mem::take(&mut self.layout);
        let mut view = WorkspaceView { tree };
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
        self.viewport_rect = self
            .layout
            .find_pane(|pane| *pane == EditorPane::Viewport)
            .and_then(|(pane, _)| self.frames.pane_rect(pane));
        if let Some(rect) = self.viewport_rect {
            let scale = viewport_ui.ctx().pixels_per_point();
            camera.viewport_origin = Vec2::new(rect.left(), rect.top()) * scale;
            camera.viewport = Vec2::new(rect.width().max(1.0), rect.height().max(1.0)) * scale;
            self.draw_top_controls(viewport_ui.ctx(), tree, camera);
            self.draw_view_gizmo(viewport_ui.ctx(), camera);
            viewport_ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.set_clip_rect(rect);
                self.ghosts = boolean_overlay::draw(ui, tree, camera);
                if let Some(pick) = scene_actions::pending_boolean(tree) {
                    // Resize handles must not intercept the operand-selection click.
                    ui.painter().text(
                        rect.center_bottom() - egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!(
                            "{}: click another object · Esc cancels",
                            pick.operation.label()
                        ),
                        egui::FontId::proportional(13.0),
                        Color32::WHITE,
                    );
                } else {
                    self.draw_selection_tools(ui, tree, camera);
                    if !self.selection_tools.box_mode() && !self.selection_tools.active() {
                        self.draw_object_gizmos(ui, tree, camera);
                    }
                }
            });
        }

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
        file_action
    }

    pub fn reset_document_gestures(&mut self) {
        self.selection_tools = selection_tools::SelectionTools::default();
        self.ghosts = boolean_overlay::Ghosts::default();
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

    fn draw_top_controls(&mut self, ctx: &egui::Context, tree: &mut DataTree, camera: &mut Camera) {
        let viewport = self.viewport_rect.unwrap();
        let left = egui::Area::new("top-left-tools".into())
            .order(egui::Order::Foreground)
            .fixed_pos(self.viewport_rect.unwrap().left_top() + egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.set_max_width((viewport.width() - 12.0).max(80.0));
                egui::Frame::NONE.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    ui.horizontal_wrapped(|ui| {
                        if viewport.width() >= 400.0 {
                            for kind in PrimitiveKind::ALL {
                                if view_button(
                                    ui,
                                    primitive_icon_source(kind),
                                    &format!("Add {}", kind.label()),
                                )
                                .clicked()
                                {
                                    commands::spawn(tree, kind.object_type());
                                }
                            }
                        } else {
                            ui.scope(|ui| {
                                style_view_buttons(ui);
                                egui::containers::menu::MenuButton::from_button(view_icon_button(
                                    egui::include_image!("../assets/icons/lucide/plus.svg"),
                                    false,
                                ))
                                .ui(ui, |ui| {
                                    for kind in PrimitiveKind::ALL {
                                        if ui
                                            .add(egui::Button::image_and_text(
                                                icon_image(
                                                    primitive_icon_source(kind),
                                                    ui.visuals().text_color(),
                                                ),
                                                kind.label(),
                                            ))
                                            .clicked()
                                        {
                                            commands::spawn(tree, kind.object_type());
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                                .on_hover_text("Add object");
                            });
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../assets/icons/lucide/command.svg"),
                            "Command palette (Cmd/Ctrl+Shift+P)",
                        )
                        .clicked()
                        {
                            self.palette.open();
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../assets/icons/lucide/undo-2.svg"),
                            "Undo",
                        )
                        .clicked()
                        {
                            undo_redo::undo(tree);
                        }
                        if view_button(
                            ui,
                            egui::include_image!("../assets/icons/lucide/redo-2.svg"),
                            "Redo",
                        )
                        .clicked()
                        {
                            undo_redo::redo(tree);
                        }
                    });
                });
            });
        self.regions.push(left.response.rect);

        // Move the right-hand group below the tools when space is tight.
        let projection_y = if viewport.width() < left.response.rect.width() + 72.0 {
            left.response.rect.height() + 10.0
        } else {
            6.0
        };
        let right = egui::Area::new("top-right-tools".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(viewport.right_top() + egui::vec2(-6.0, projection_y))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                ui.horizontal(|ui| {
                    ui.set_height(26.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (projection_icon, next_mode) = match camera.projection_mode {
                        crate::camera::ProjectionMode::Perspective => (
                            egui::include_image!("../assets/icons/lucide/box.svg"),
                            "orthographic",
                        ),
                        crate::camera::ProjectionMode::Orthographic => (
                            egui::include_image!("../assets/icons/lucide/square.svg"),
                            "perspective",
                        ),
                    };
                    let tooltip =
                        format!("{} — click for {next_mode}", camera.projection_mode.label());
                    if view_button(ui, projection_icon, &tooltip).clicked() {
                        camera.toggle_projection();
                    }
                    if view_button(
                        ui,
                        egui::include_image!("../assets/icons/lucide/rotate-3d.svg"),
                        "Snap to isometric view",
                    )
                    .clicked()
                    {
                        camera.snap(ViewAngle::Isometric);
                    }
                });
            });
        self.regions.push(right.response.rect);
        self.draw_selection_toolbar(ctx, viewport);
    }

    fn draw_view_gizmo(&mut self, ctx: &egui::Context, camera: &mut Camera) {
        let viewport = self.viewport_rect.unwrap();
        if viewport.height() < 250.0 || viewport.width() < 150.0 {
            return;
        }
        let offset = camera.position - camera.target;
        let yaw = offset.x.atan2(offset.z).to_degrees();
        let pitch = (offset.y / offset.length().max(0.001)).asin().to_degrees();
        let area = egui::Area::new("view-gizmo".into())
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(self.viewport_rect.unwrap().left_bottom() + egui::vec2(10.0, -10.0))
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(7))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{yaw:.0}°  {pitch:.0}°"))
                                .small()
                                .color(Color32::LIGHT_GRAY),
                        );
                        orientation_axes(ui, camera);
                    });
            });
        self.regions.push(area.response.rect);
    }

    fn draw_object_gizmos(&mut self, ui: &mut egui::Ui, tree: &mut DataTree, camera: &Camera) {
        let selection = selected(tree);
        if selection.len() != 1 {
            return;
        }
        let mut scene = objects(tree);
        let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
            return;
        };
        let handles = resize_handles(object, camera);
        let mut changed = false;
        let mut finished = false;
        for (index, handle) in handles.iter().enumerate() {
            let Some(center) = camera.project(handle.world, ui.ctx().pixels_per_point()) else {
                continue;
            };
            if !ui.clip_rect().contains(center)
                || self.regions.iter().any(|rect| rect.contains(center))
            {
                continue;
            }
            let Some(ahead) = camera.project(
                handle.world + handle.direction * 0.1,
                ui.ctx().pixels_per_point(),
            ) else {
                continue;
            };
            let projected_axis = (ahead - center) * 10.0;
            if projected_axis.length_sq() < 4.0 {
                continue;
            }
            let axis = projected_axis / projected_axis.length();
            let tip = center + axis * 28.0;
            let mut hit = egui::Rect::from_center_size(center, egui::vec2(22.0, 22.0))
                .union(egui::Rect::from_center_size(tip, egui::vec2(14.0, 14.0)));
            let patch: Vec<_> = handle
                .patch
                .iter()
                .filter_map(|point| camera.project(*point, ui.ctx().pixels_per_point()))
                .collect();
            if patch.len() == 4 {
                for point in &patch {
                    hit.extend_with(*point);
                }
            }
            hit = hit.intersect(ui.clip_rect());
            self.regions.push(hit);
            let response = ui
                .interact(
                    hit,
                    egui::Id::new(("resize", object.uuid, index)),
                    egui::Sense::drag(),
                )
                .on_hover_text(format!(
                    "Drag to resize {}. Release to finish.",
                    handle.label
                ));
            let stroke = Stroke::new(
                if response.hovered() || response.dragged() {
                    2.5
                } else {
                    1.5
                },
                handle.color,
            );
            if patch.len() == 4 {
                ui.painter().add(egui::Shape::convex_polygon(
                    patch,
                    handle.color.gamma_multiply(0.18),
                    stroke,
                ));
            } else if let Some(origin) =
                camera.project(handle.guide_origin, ui.ctx().pixels_per_point())
            {
                let delta = center - origin;
                for segment in 0..12 {
                    let t = segment as f32 / 12.0;
                    ui.painter()
                        .line_segment([origin + delta * t, origin + delta * (t + 0.045)], stroke);
                }
                ui.painter().circle_stroke(center, 5.0, stroke);
            }
            ui.painter().arrow(center, tip - center, stroke);
            if response.hovered() || response.dragged() {
                let text = format!("{} {:.2}", handle.label, handle.value);
                let galley = ui.painter().layout_no_wrap(
                    text,
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
                if let Some(label_rect) =
                    gizmo_label_rect(ui.clip_rect(), tip, galley.size(), &self.regions)
                {
                    ui.painter()
                        .rect_filled(label_rect, 4.0, Color32::from_black_alpha(210));
                    ui.painter().galley(
                        label_rect.min + egui::vec2(4.0, 4.0),
                        galley,
                        Color32::WHITE,
                    );
                    self.regions.push(label_rect);
                }
            }
            if response.dragged() {
                let delta = ui.input(|input| input.pointer.delta());
                let amount = delta.dot(projected_axis) / projected_axis.length_sq();
                match &mut object.params {
                    SdfParams::BoxParams(params) => {
                        params.box_q[handle.parameter] =
                            (params.box_q[handle.parameter] + amount).max(0.01)
                    }
                    SdfParams::SphereParams(params) => {
                        // Keep the base radius and other axes unchanged. The
                        // existing local scale represents the ellipsoid radii.
                        let axis = handle.parameter;
                        let sign = if object.transform.scale[axis] < 0.0 {
                            -1.0
                        } else {
                            1.0
                        };
                        object.transform.scale[axis] =
                            sign * (handle.value + amount).max(0.01) / params.radius.max(0.0001);
                    }
                    SdfParams::CylinderParams {
                        radius,
                        half_height,
                    } => {
                        let value = if handle.parameter == 0 {
                            radius
                        } else {
                            half_height
                        };
                        *value = (*value + amount).max(0.01);
                    }
                    SdfParams::TorusParams {
                        major_radius,
                        minor_radius,
                    } => {
                        let value = if handle.parameter == 0 {
                            major_radius
                        } else {
                            minor_radius
                        };
                        *value = (*value + amount).max(0.01);
                    }
                }
                changed = true;
            }
            finished |= response.drag_stopped();
        }
        if changed {
            set_objects(tree, scene);
        }
        if finished {
            tree.make_undo_redo_snapshot();
        }
    }
}

fn draw_file_menu(viewport_ui: &mut egui::Ui, document: &DocumentState) -> Option<FileMenuAction> {
    let mut action = None;
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
    action
}

fn draw_file_error(ctx: &egui::Context, document: &mut DocumentState) -> Option<egui::Rect> {
    let message = document.error().map(str::to_owned)?;
    let mut open = true;
    let mut dismiss = false;
    let response = egui::Window::new("File error")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(message);
            dismiss = ui.button("Dismiss").clicked();
        });
    if dismiss || !open {
        document.clear_error();
    }
    response.map(|response| response.response.rect)
}

struct WorkspaceView<'a> {
    tree: &'a mut DataTree,
}

impl PaneView<EditorPane> for WorkspaceView<'_> {
    fn tab(&mut self, _id: PaneId, pane: &EditorPane) -> Tab {
        Tab::new(pane.title()).closable(*pane != EditorPane::Viewport)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, id: PaneId, pane: &EditorPane) {
        if *pane == EditorPane::Viewport {
            return;
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Color32::from_rgb(25, 26, 29));
        egui::ScrollArea::vertical()
            .id_salt(("editor-pane-scroll", id))
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().text_edit_width = ui.available_width();
                        ui.spacing_mut().slider_width = ui
                            .spacing()
                            .slider_width
                            .min((ui.available_width() - 100.0).max(24.0));
                        match pane {
                            EditorPane::Scene => scene_panel(ui, self.tree),
                            EditorPane::Object => object_panel(ui, self.tree),
                            EditorPane::Materials => materials_panel(ui, self.tree),
                            EditorPane::Repetition => repetition_panel(ui, self.tree),
                            EditorPane::Operand => operand_panel(ui, self.tree),
                            EditorPane::Viewport => {}
                        }
                    });
            });
    }
}

fn scene_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    let scene = objects(tree);
    if let Some(pick) = scene_actions::pending_boolean(tree) {
        let target = scene
            .iter()
            .find(|object| object.uuid == pick.target)
            .unwrap();
        ui.colored_label(
            Color32::LIGHT_BLUE,
            format!("{}: {}", pick.operation.label(), target.display_name()),
        );
        ui.label("Click an operand in the scene or tree. Escape cancels.")
            .on_hover_text("The target and its ancestors cannot be used as operands. Choose another object or group.");
        if ui.button("Cancel operation").clicked() {
            scene_actions::cancel_boolean_pick(tree);
        }
        ui.separator();
    }
    if selection.len() >= 2 {
        if let Some(target) = scene.iter().find(|object| object.uuid == selection[0]) {
            ui.label(RichText::new(format!("→ {}", target.display_name())).strong());
        }
        ui.horizontal_wrapped(|ui| {
            for (label, operation) in [
                ("Union", BooleanOperation::Union),
                ("Subtract", BooleanOperation::Subtract),
                ("Intersect", BooleanOperation::Intersect),
            ] {
                if ui
                    .add_enabled(selection.len() >= 2, egui::Button::new(label))
                    .on_hover_text(match operation {
                        BooleanOperation::Union => "Add the selected shapes to the target.",
                        BooleanOperation::Subtract => {
                            "Cut all selected operands out of the first selected target."
                        }
                        BooleanOperation::Intersect => {
                            "Keep only the volume shared by target and operands."
                        }
                    })
                    .clicked()
                {
                    apply_boolean(tree, operation);
                }
            }
        });
        ui.separator();
    }
    for object in scene
        .iter()
        .filter(|object| object.boolean_parent.is_none())
    {
        subtree_rows(ui, tree, &scene, object, &selection, 0);
    }
}

fn subtree_rows(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    scene: &[SdfObject],
    object: &SdfObject,
    selection: &[uuid::Uuid],
    depth: usize,
) {
    if depth >= scene.len() {
        return;
    }
    object_row(ui, tree, object, selection, depth);
    for child in scene
        .iter()
        .filter(|child| child.boolean_parent == Some(object.uuid))
    {
        subtree_rows(ui, tree, scene, child, selection, depth + 1);
    }
}

fn operation_style(operation: BooleanOperation) -> (&'static str, Color32) {
    match operation {
        BooleanOperation::Union => ("+", Color32::from_rgb(140, 210, 175)),
        BooleanOperation::Subtract => ("−", Color32::from_rgb(255, 192, 115)),
        BooleanOperation::Intersect => ("∩", Color32::from_rgb(159, 219, 255)),
    }
}

fn operand_shape_menu(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    target: uuid::Uuid,
    operation: BooleanOperation,
) {
    for kind in PrimitiveKind::ALL {
        if ui
            .add(egui::Button::image_and_text(
                icon_image(primitive_icon_source(kind), ui.visuals().text_color()),
                kind.label(),
            ))
            .clicked()
        {
            scene_actions::add_operand(tree, target, kind, operation);
            ui.close();
        }
    }
}

fn operand_operation_menu(ui: &mut egui::Ui, tree: &mut DataTree, object: &SdfObject) {
    for operation in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ] {
        if ui
            .selectable_label(object.operation == operation, operation.label())
            .clicked()
        {
            edit_boolean_operand(tree, object.uuid, Some(operation));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Detach").clicked() {
        edit_boolean_operand(tree, object.uuid, None);
        ui.close();
    }
}

fn object_row(
    ui: &mut egui::Ui,
    tree: &mut DataTree,
    object: &SdfObject,
    selection: &[uuid::Uuid],
    depth: usize,
) {
    ui.push_id(object.uuid, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("...", |ui| {
                    if ui.button("Duplicate").clicked() {
                        commands::duplicate_object(tree, object.uuid);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Add cutter", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Subtract)
                    });
                    ui.menu_button("Add union", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Union)
                    });
                    ui.menu_button("Add intersection", |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Intersect)
                    });
                    let operands: Vec<_> = selection
                        .iter()
                        .copied()
                        .filter(|id| *id != object.uuid)
                        .collect();
                    if scene_actions::can_attach(&objects(tree), object.uuid, &operands) {
                        ui.separator();
                        for (label, operation) in [
                            ("Subtract selected", BooleanOperation::Subtract),
                            ("Union selected", BooleanOperation::Union),
                            ("Intersect selected", BooleanOperation::Intersect),
                        ] {
                            if ui.button(label).clicked() {
                                scene_actions::attach(tree, object.uuid, &operands, operation);
                                ui.close();
                            }
                        }
                    }
                    if object.boolean_parent.is_some() {
                        ui.separator();
                        operand_operation_menu(ui, tree, object);
                    }
                })
                .response
                .on_hover_text("Object actions");
                ui.menu_button(
                    RichText::new("−")
                        .color(operation_style(BooleanOperation::Subtract).1)
                        .strong(),
                    |ui| {
                        operand_shape_menu(ui, tree, object.uuid, BooleanOperation::Subtract);
                    },
                )
                .response
                .on_hover_text("Add a subtractive shape");

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space((depth as f32 * 12.0).min(ui.available_width() * 0.3));
                        if depth > 0 {
                            let (symbol, color) = operation_style(object.operation);
                            let badge = if object.operation == BooleanOperation::Intersect {
                                egui::Button::image(
                                    egui::Image::new(egui::include_image!(
                                        "../assets/icons/intersect.svg"
                                    ))
                                    .fit_to_exact_size(egui::vec2(12.0, 12.0))
                                    .tint(color),
                                )
                            } else {
                                egui::Button::new(RichText::new(symbol).color(color).strong())
                            };
                            egui::containers::menu::MenuButton::from_button(badge)
                                .ui(ui, |ui| {
                                    operand_operation_menu(ui, tree, object);
                                })
                                .0
                                .on_hover_text(object.operation.label());
                        }
                        primitive_icon(ui, PrimitiveKind::from_object_type(object.object_type));
                        let selected_now = selection.contains(&object.uuid);
                        let rename_id = ui.id().with("rename");
                        let rename_focus_id = ui.id().with("rename-focus");
                        let response = if let Some(mut draft) =
                            ui.ctx().data(|data| data.get_temp::<String>(rename_id))
                        {
                            let response = ui.add_sized(
                                egui::vec2(ui.available_width().max(1.0), 24.0),
                                egui::TextEdit::singleline(&mut draft),
                            );
                            let needs_focus = ui.ctx().data(|data| {
                                !data.get_temp::<bool>(rename_focus_id).unwrap_or(false)
                            });
                            if needs_focus {
                                response.request_focus();
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(rename_focus_id, true);
                                });
                            }
                            let cancel = response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Escape));
                            let commit = response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(rename_id, draft.clone());
                            });
                            if cancel || commit || response.lost_focus() {
                                ui.ctx().data_mut(|data| {
                                    data.remove_temp::<String>(rename_id);
                                    data.remove_temp::<bool>(rename_focus_id);
                                });
                                if !cancel {
                                    rename_object(tree, object.uuid, draft);
                                }
                            }
                            None
                        } else {
                            Some(
                                ui.add_sized(
                                    egui::vec2(ui.available_width().max(1.0), 24.0),
                                    egui::Button::new(object.display_name())
                                        .selected(selected_now)
                                        .frame(false)
                                        .truncate()
                                        .sense(egui::Sense::click_and_drag()),
                                )
                                .on_hover_text(format!(
                                "{}\nDouble-click to rename · Drag onto another object to combine",
                                object.display_name()
                            )),
                            )
                        };
                        let Some(response) = response else {
                            return;
                        };
                        if response.double_clicked() {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(rename_id, object.display_name());
                                data.insert_temp(rename_focus_id, false);
                            });
                        }
                        if response.clicked() && !response.double_clicked() {
                            response.surrender_focus();
                            if !scene_actions::apply_boolean_pick(tree, object.uuid) {
                                let extend = ui.input(|input| {
                                    input.modifiers.shift || input.modifiers.command
                                });
                                let mut next = if extend {
                                    selection.to_vec()
                                } else {
                                    Vec::new()
                                };
                                if extend && selected_now {
                                    next.retain(|id| *id != object.uuid);
                                } else {
                                    next.push(object.uuid);
                                }
                                set_selected(tree, next);
                            }
                        }
                        response.dnd_set_drag_payload(scene_actions::DragObjects(
                            if selected_now {
                                selection.to_vec()
                            } else {
                                vec![object.uuid]
                            },
                        ));
                        let popup_id = ui.id().with("drop-operation");
                        if let Some(payload) =
                            response.dnd_hover_payload::<scene_actions::DragObjects>()
                        {
                            let valid =
                                scene_actions::can_attach(&objects(tree), object.uuid, &payload.0);
                            ui.ctx().set_cursor_icon(if valid {
                                egui::CursorIcon::Copy
                            } else {
                                egui::CursorIcon::NotAllowed
                            });
                            if valid {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    4,
                                    Stroke::new(2.0, Color32::from_rgb(255, 192, 115)),
                                    egui::StrokeKind::Inside,
                                );
                                if let Some(payload) =
                                    response.dnd_release_payload::<scene_actions::DragObjects>()
                                {
                                    ui.ctx().data_mut(|data| {
                                        data.insert_temp(popup_id, (*payload).clone())
                                    });
                                    egui::Popup::open_id(ui.ctx(), popup_id);
                                }
                            }
                        }
                        egui::Popup::from_response(&response)
                            .id(popup_id)
                            .open_memory(None)
                            .show(|ui| {
                                ui.strong(object.display_name());
                                for operation in [
                                    BooleanOperation::Union,
                                    BooleanOperation::Subtract,
                                    BooleanOperation::Intersect,
                                ] {
                                    let (symbol, color) = operation_style(operation);
                                    if ui
                                        .button(
                                            RichText::new(format!(
                                                "{}  {}",
                                                if operation == BooleanOperation::Intersect {
                                                    "&"
                                                } else {
                                                    symbol
                                                },
                                                operation.label()
                                            ))
                                            .color(color),
                                        )
                                        .clicked()
                                    {
                                        if let Some(payload) = ui.ctx().data_mut(|data| {
                                            data.remove_temp::<scene_actions::DragObjects>(popup_id)
                                        }) {
                                            scene_actions::attach(
                                                tree,
                                                object.uuid,
                                                &payload.0,
                                                operation,
                                            );
                                        }
                                        ui.close();
                                    }
                                }
                            });
                        response.context_menu(|ui| {
                            if ui.button("Duplicate").clicked() {
                                commands::duplicate_object(tree, object.uuid);
                                ui.close();
                            }
                            ui.separator();
                            ui.menu_button("Add cutter", |ui| {
                                operand_shape_menu(
                                    ui,
                                    tree,
                                    object.uuid,
                                    BooleanOperation::Subtract,
                                )
                            });
                            if object.boolean_parent.is_some() {
                                ui.separator();
                                operand_operation_menu(ui, tree, object);
                            }
                        });
                    },
                );
            });
        });
    });
}

fn edit_boolean_operand(tree: &mut DataTree, id: uuid::Uuid, operation: Option<BooleanOperation>) {
    let mut scene = objects(tree);
    if let Some(object) = scene.iter_mut().find(|object| object.uuid == id) {
        object.operation = operation.unwrap_or(BooleanOperation::Union);
        if operation.is_none() {
            object.boolean_parent = None;
        }
        set_objects(tree, scene);
        tree.make_undo_redo_snapshot();
    }
}

fn rename_object(tree: &mut DataTree, id: uuid::Uuid, name: String) {
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == id) else {
        return;
    };
    if object.name == name {
        return;
    }
    object.name = name;
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

fn apply_boolean(tree: &mut DataTree, operation: BooleanOperation) {
    let selection = selected(tree);
    if selection.len() < 2 {
        return;
    }
    scene_actions::attach(tree, selection[0], &selection[1..], operation);
}

fn object_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    if selection.len() != 1 {
        ui.label("Select one object to edit it.");
        return;
    }
    let mut scene = objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == selection[0]) else {
        return;
    };
    let mut changed = false;
    ui.label(RichText::new("Object settings").strong());
    changed |= ui.text_edit_singleline(&mut object.name).changed();
    ui.separator();
    let reset_position = ui
        .horizontal(|ui| {
            ui.label("Position");
            ui.add_enabled(
                object.transform.translation != Vec3::ZERO,
                egui::Button::new("Reset"),
            )
            .on_hover_text("Reset position to the origin")
            .clicked()
        })
        .inner;
    if reset_position {
        tree.make_undo_redo_snapshot();
        object.transform.translation = Vec3::ZERO;
        changed = true;
    }
    changed |= vec3_editor(ui, &mut object.transform.translation, 0.01);
    let reset_rotation = ui
        .horizontal(|ui| {
            ui.label("Rotation");
            ui.add_enabled(
                object.transform.rotation != glam::Quat::IDENTITY,
                egui::Button::new("Reset"),
            )
            .on_hover_text("Reset rotation to zero on all axes")
            .clicked()
        })
        .inner;
    if reset_rotation {
        tree.make_undo_redo_snapshot();
        object.transform.rotation = glam::Quat::IDENTITY;
        changed = true;
    }
    let (x, y, z) = object.transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    if vec3_editor(ui, &mut degrees, 1.0) {
        object.transform.rotation = glam::Quat::from_euler(
            EulerRot::XYZ,
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        changed = true;
    }
    ui.label("Scale");
    changed |= vec3_editor(ui, &mut object.transform.scale, 0.01);
    ui.separator();
    changed |= params_editor(ui, &mut object.params);
    if changed {
        set_objects(tree, scene);
        if reset_position || reset_rotation {
            tree.make_undo_redo_snapshot();
        }
    }
}

fn params_editor(ui: &mut egui::Ui, params: &mut SdfParams) -> bool {
    match params {
        SdfParams::SphereParams(value) => ui
            .add(egui::Slider::new(&mut value.radius, 0.01..=4.0).text("Radius"))
            .changed(),
        SdfParams::BoxParams(value) => {
            ui.label("Half extents");
            vec3_editor(ui, &mut value.box_q, 0.01)
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            ui.add(egui::Slider::new(radius, 0.01..=4.0).text("Radius"))
                .changed()
                | ui.add(egui::Slider::new(half_height, 0.01..=4.0).text("Half height"))
                    .changed()
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            ui.add(egui::Slider::new(major_radius, 0.02..=4.0).text("Major radius"))
                .changed()
                | ui.add(egui::Slider::new(minor_radius, 0.01..=2.0).text("Tube radius"))
                    .changed()
        }
    }
}

fn materials_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    ui.label(RichText::new("Material library").strong());
    ui.horizontal_wrapped(|ui| {
        for kind in MaterialKind::ALL {
            let preset = Material::preset(kind);
            let response = material_preview(ui, kind, preset);
            if response.clicked() {
                apply_material(tree, preset);
            }
        }
    });
    ui.separator();
    let selection = selected(tree);
    if selection.is_empty() {
        ui.label("Pick a material for new objects, or select objects to edit their material.");
        return;
    }
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        return;
    };
    let mut material = first.material;
    material.color = first.color;
    let mut rgba = material.color.to_array();
    let mut changed = ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed();
    material.color = Vec4::from_array(rgba);
    for (label, value, range) in [
        ("Roughness", &mut material.roughness, 0.0..=1.0),
        ("Metallic", &mut material.metallic, 0.0..=1.0),
        ("Reflectivity", &mut material.reflectivity, 0.0..=1.0),
        (
            "Refractive index",
            &mut material.refractive_index,
            1.0..=2.5,
        ),
        ("Opacity", &mut material.opacity, 0.02..=1.0),
    ] {
        let label = ui.label(label);
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(24.0);
        changed |= ui
            .add(egui::Slider::new(value, range))
            .labelled_by(label.id)
            .changed();
    }
    if changed {
        tree.set_path(
            "editor.material",
            crate::model::ClaydashValue::Material(material),
        );
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                object.material = material;
                object.color = material.color;
            }
        }
        set_objects(tree, scene);
    }
}

fn apply_material(tree: &mut DataTree, material: Material) {
    tree.set_path(
        "editor.material",
        crate::model::ClaydashValue::Material(material),
    );
    let selection = selected(tree);
    let mut scene = objects(tree);
    for object in &mut scene {
        if selection.contains(&object.uuid) {
            object.material = material;
            object.color = material.color;
        }
    }
    set_objects(tree, scene);
    tree.make_undo_redo_snapshot();
}

fn operand_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        ui.label("Select an object to edit its operand properties.");
        return;
    };
    ui.label(RichText::new("Operand properties").strong());
    if let Some(parent) = first
        .boolean_parent
        .and_then(|id| scene.iter().find(|o| o.uuid == id))
    {
        ui.label(format!("Target: {}", parent.display_name()));
    } else {
        ui.label("These settings apply when this object is used as an operand.");
    }
    let mut operation = first.operation;
    let mut softness = first.softness;
    let mut changed = false;
    egui::ComboBox::from_label("Operation")
        .selected_text(operation.label())
        .show_ui(ui, |ui| {
            for value in [
                BooleanOperation::Union,
                BooleanOperation::Subtract,
                BooleanOperation::Intersect,
            ] {
                changed |= ui
                    .selectable_value(&mut operation, value, value.label())
                    .changed();
            }
        });
    let response = ui.add(egui::Slider::new(&mut softness, 0.0..=0.5).text("Softness"));
    let operation_changed = changed;
    changed |= response.changed();
    ui.label("0 = sharp edges. Softness is measured in world units.");
    if response.drag_started() || operation_changed || (changed && !response.dragged()) {
        tree.make_undo_redo_snapshot();
    }
    if changed {
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                if operation_changed {
                    object.operation = operation;
                }
                if response.changed() {
                    object.softness = softness;
                }
            }
        }
        set_objects(tree, scene);
    }
    if response.drag_stopped() || operation_changed || (changed && !response.dragged()) {
        tree.make_undo_redo_snapshot();
    }
}

fn repetition_panel(ui: &mut egui::Ui, tree: &mut DataTree) {
    let selection = selected(tree);
    if selection.is_empty() {
        ui.label("Select objects to repeat.");
        return;
    }
    let mut scene = objects(tree);
    let Some(first) = scene.iter().find(|object| selection.contains(&object.uuid)) else {
        return;
    };
    let mut repetition = first.repetition;
    let mut changed = ui
        .checkbox(&mut repetition.enabled, "Enable domain repetition")
        .changed();
    ui.label("Axes");
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut repetition.axes[0], "X").changed();
        changed |= ui.checkbox(&mut repetition.axes[1], "Y").changed();
        changed |= ui.checkbox(&mut repetition.axes[2], "Z").changed();
    });
    for axis in 0..3 {
        ui.horizontal_wrapped(|ui| {
            ui.label(axis_label(axis));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut repetition.count[axis])
                        .range(1..=32)
                        .prefix("count "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut repetition.spacing[axis])
                        .speed(0.02)
                        .range(0.01..=20.0)
                        .prefix("spacing "),
                )
                .changed();
        });
    }
    if changed {
        for object in &mut scene {
            if selection.contains(&object.uuid) {
                object.repetition = repetition;
            }
        }
        set_objects(tree, scene);
    }
}

fn vec3_editor(ui: &mut egui::Ui, value: &mut Vec3, speed: f64) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for axis in 0..3 {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut value[axis])
                        .speed(speed)
                        .prefix(format!("{} ", axis_label(axis))),
                )
                .changed();
        }
    });
    changed
}

struct ResizeHandle {
    world: Vec3,
    guide_origin: Vec3,
    direction: Vec3,
    patch: Vec<Vec3>,
    label: String,
    color: Color32,
    parameter: usize,
    value: f32,
}

fn gizmo_label_rect(
    viewport: egui::Rect,
    tip: egui::Pos2,
    text_size: egui::Vec2,
    occupied: &[egui::Rect],
) -> Option<egui::Rect> {
    let size = text_size + egui::vec2(8.0, 8.0);
    let offsets = [
        egui::vec2(12.0, -size.y * 0.5),
        egui::vec2(-size.x - 12.0, -size.y * 0.5),
        egui::vec2(-size.x * 0.5, -size.y - 12.0),
        egui::vec2(-size.x * 0.5, 12.0),
    ];
    offsets
        .into_iter()
        .map(|offset| egui::Rect::from_min_size(tip + offset, size))
        .find(|rect| {
            viewport.contains_rect(*rect) && !occupied.iter().any(|other| rect.intersects(*other))
        })
}

fn resize_handles(object: &SdfObject, camera: &Camera) -> Vec<ResizeHandle> {
    let matrix = object.transform.matrix();
    let mut handles = Vec::new();
    match &object.params {
        SdfParams::BoxParams(params) => {
            for axis in 0..3 {
                for sign in [-1.0, 1.0] {
                    let mut local = Vec3::ZERO;
                    local[axis] = params.box_q[axis] * sign;
                    let mut direction = Vec3::ZERO;
                    direction[axis] = sign;
                    let world = matrix.transform_point3(local);
                    let direction = matrix.transform_vector3(direction);
                    if direction.dot(camera.position - world) <= 0.0 {
                        continue;
                    }
                    let u = (axis + 1) % 3;
                    let v = (axis + 2) % 3;
                    let patch = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                        .into_iter()
                        .map(|(a, b)| {
                            let mut point = local;
                            point[u] = params.box_q[u] * 0.3 * a;
                            point[v] = params.box_q[v] * 0.3 * b;
                            matrix.transform_point3(point)
                        })
                        .collect();
                    handles.push(ResizeHandle {
                        world,
                        guide_origin: object.transform.translation,
                        direction,
                        patch,
                        label: format!("Resize {}", axis_label(axis)),
                        color: axis_color(axis),
                        parameter: axis,
                        value: params.box_q[axis] * 2.0,
                    });
                }
            }
        }
        SdfParams::SphereParams(params) => {
            for (axis, local_axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                let sign = if object.transform.scale[axis] < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let axis_direction = object.transform.rotation * local_axis * sign;
                // Use the camera-facing end of each local axis so handles do
                // not bunch together on the far side of a small sphere.
                let side =
                    if axis_direction.dot(camera.position - object.transform.translation) < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                let direction = axis_direction * side;
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(local_axis * params.radius * side),
                    guide_origin: object.transform.translation,
                    direction,
                    patch: vec![],
                    label: format!("Radius {}", axis_label(axis)),
                    color: axis_color(axis),
                    parameter: axis,
                    value: params.radius * object.transform.scale[axis].abs(),
                });
            }
        }
        SdfParams::CylinderParams {
            radius,
            half_height,
        } => {
            for (axis, value, label) in [(0, *radius, "Radius"), (1, *half_height, "Height")] {
                let direction = if axis == 0 { Vec3::X } else { Vec3::Y };
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(direction * value),
                    guide_origin: object.transform.translation,
                    direction: matrix.transform_vector3(direction),
                    patch: vec![],
                    label: label.into(),
                    color: axis_color(axis),
                    parameter: axis,
                    value: if axis == 1 { value * 2.0 } else { value },
                });
            }
        }
        SdfParams::TorusParams {
            major_radius,
            minor_radius,
        } => {
            for (parameter, local, direction, label, value) in [
                (
                    0,
                    Vec3::X * *major_radius,
                    Vec3::X,
                    "Ring radius",
                    *major_radius,
                ),
                (
                    1,
                    Vec3::X * *major_radius + Vec3::Y * *minor_radius,
                    Vec3::Y,
                    "Tube radius",
                    *minor_radius,
                ),
            ] {
                handles.push(ResizeHandle {
                    world: matrix.transform_point3(local),
                    guide_origin: matrix.transform_point3(if parameter == 1 {
                        Vec3::X * *major_radius
                    } else {
                        Vec3::ZERO
                    }),
                    direction: matrix.transform_vector3(direction),
                    patch: vec![],
                    label: label.into(),
                    color: axis_color(parameter),
                    parameter,
                    value,
                });
            }
        }
    }
    handles
}

fn orientation_axes(ui: &mut egui::Ui, camera: &mut Camera) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 82.0), egui::Sense::hover());
    let center = rect.center();
    let view = camera.view();
    let axes = [
        (Vec3::X, "X", axis_color(0), ViewAngle::Right),
        (Vec3::NEG_X, "", axis_color(0), ViewAngle::Left),
        (Vec3::Y, "Y", axis_color(1), ViewAngle::Top),
        (Vec3::NEG_Y, "", axis_color(1), ViewAngle::Bottom),
        (Vec3::Z, "Z", axis_color(2), ViewAngle::Front),
        (Vec3::NEG_Z, "", axis_color(2), ViewAngle::Back),
    ];
    for (axis, label, color, angle) in axes {
        let direction = view.transform_vector3(axis);
        let end = center + egui::vec2(direction.x, -direction.y) * 31.0;
        let hit = egui::Rect::from_center_size(end, egui::vec2(18.0, 18.0));
        let response = ui.interact(
            hit,
            egui::Id::new(("view-axis", label, angle)),
            egui::Sense::click(),
        );
        let muted = color.gamma_multiply(if direction.z < 0.0 { 0.42 } else { 1.0 });
        ui.painter()
            .line_segment([center, end], Stroke::new(2.0, muted));
        ui.painter()
            .circle_filled(end, if response.hovered() { 7.0 } else { 5.0 }, muted);
        if !label.is_empty() {
            ui.painter().text(
                end,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(9.0),
                Color32::BLACK,
            );
        }
        if response.clicked() {
            camera.snap(angle);
        }
    }
}

fn primitive_icon(ui: &mut egui::Ui, kind: PrimitiveKind) {
    ui.add(icon_image(
        primitive_icon_source(kind),
        ui.visuals().text_color(),
    ));
}

fn primitive_icon_source(kind: PrimitiveKind) -> egui::ImageSource<'static> {
    match kind {
        PrimitiveKind::Sphere => egui::include_image!("../assets/icons/lucide/globe.svg"),
        PrimitiveKind::Box => egui::include_image!("../assets/icons/lucide/box.svg"),
        PrimitiveKind::Cylinder => egui::include_image!("../assets/icons/lucide/cylinder.svg"),
        PrimitiveKind::Torus => egui::include_image!("../assets/icons/lucide/torus.svg"),
    }
}

fn icon_image(source: egui::ImageSource<'static>, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(source)
        .fit_to_exact_size(egui::vec2(18.0, 18.0))
        .tint(tint)
}

fn view_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tooltip: &str,
) -> egui::Response {
    selectable_view_button(ui, source, tooltip, false)
}

fn selectable_view_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tooltip: &str,
    selected: bool,
) -> egui::Response {
    ui.scope(|ui| {
        style_view_buttons(ui);
        ui.add(view_icon_button(source, selected))
    })
    .inner
    .on_hover_text(tooltip)
}

// egui derives frame margins from theme strokes before applying Button::stroke.
// Keep those inputs fixed so hover/focus cannot change the allocated size.
fn style_view_buttons(ui: &mut egui::Ui) {
    ui.spacing_mut().button_padding = egui::vec2(3.0, 3.0);
    let widgets = &mut ui.style_mut().visuals.widgets;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.bg_stroke.width = 1.5;
        state.expansion = 0.0;
    }
}

fn view_icon_button(source: egui::ImageSource<'static>, selected: bool) -> egui::Button<'static> {
    egui::Button::image(icon_image(source, Color32::WHITE))
        .fill(if selected {
            Color32::from_rgba_unmultiplied(151, 73, 235, 155)
        } else {
            Color32::from_black_alpha(165)
        })
        .selected(selected)
        .stroke(Stroke::new(
            1.5,
            if selected {
                Color32::from_rgb(232, 183, 255)
            } else {
                Color32::TRANSPARENT
            },
        ))
        .corner_radius(CornerRadius::same(255))
        .min_size(egui::vec2(26.0, 26.0))
}

fn axis_label(axis: usize) -> &'static str {
    match axis {
        0 => "X",
        1 => "Y",
        _ => "Z",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_then_tree_click_combines_whole_groups_and_undoes() {
        use winit::keyboard::KeyCode;
        for (key, operation) in [
            (KeyCode::Equal, BooleanOperation::Union),
            (KeyCode::Minus, BooleanOperation::Subtract),
            (KeyCode::NumpadMultiply, BooleanOperation::Intersect),
        ] {
            let ctx = egui::Context::default();
            let mut tree = DataTree::default();
            let mut target = SdfObject::create_kind(PrimitiveKind::Box);
            target.name = "First target".into();
            let mut group = SdfObject::create_kind(PrimitiveKind::Sphere);
            group.name = "Other group".into();
            let mut child = SdfObject::create_kind(PrimitiveKind::Box);
            child.boolean_parent = Some(group.uuid);
            child.operation = BooleanOperation::Subtract;
            set_objects(
                &mut tree,
                vec![target.clone(), group.clone(), child.clone()],
            );
            tree.make_undo_redo_snapshot();
            click_scene_text(&ctx, &mut tree, "First target", egui::Modifiers::NONE);
            let mut interactions = crate::interactions::InteractionState::default();
            interactions.key_pressed(
                key,
                ctx.egui_wants_keyboard_input(),
                &Commands::new(),
                &mut tree,
            );
            assert_eq!(
                scene_actions::pending_boolean(&tree).unwrap().target,
                target.uuid
            );
            click_scene_text(&ctx, &mut tree, "Other group", egui::Modifiers::NONE);
            let scene = objects(&tree);
            assert_eq!(scene[1].boolean_parent, Some(target.uuid));
            assert_eq!(scene[1].operation, operation);
            assert_eq!(scene[2].boolean_parent, Some(group.uuid));
            assert_eq!(scene[2].operation, BooleanOperation::Subtract);
            assert!(scene_actions::pending_boolean(&tree).is_none());
            undo_redo::undo(&mut tree);
            assert!(objects(&tree)[1].boolean_parent.is_none());
            assert!(scene_actions::pending_boolean(&tree).is_none());
        }
    }

    #[test]
    fn inspector_resets_only_requested_transform_property_and_undoes() {
        for reset_index in 0..2 {
            let ctx = egui::Context::default();
            let mut tree = DataTree::default();
            let mut object = SdfObject::create_kind(PrimitiveKind::Box);
            object.transform.translation = Vec3::new(1.0, 2.0, 3.0);
            object.transform.rotation = glam::Quat::from_rotation_y(0.7);
            object.transform.scale = Vec3::new(2.0, 3.0, 4.0);
            set_selected(&mut tree, vec![object.uuid]);
            set_objects(&mut tree, vec![object.clone()]);
            tree.make_undo_redo_snapshot();
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| object_panel(ui, &mut tree),
                );
                output.textures_delta.clear();
                output.shapes
            };
            let shapes = frame(vec![]);
            let positions: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Reset" => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .collect();
            let position = positions[reset_index];
            for pressed in [true, false] {
                frame(vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            let result = objects(&tree)[0].transform;
            assert_eq!(
                result.translation,
                if reset_index == 0 {
                    Vec3::ZERO
                } else {
                    object.transform.translation
                }
            );
            assert_eq!(
                result.rotation,
                if reset_index == 1 {
                    glam::Quat::IDENTITY
                } else {
                    object.transform.rotation
                }
            );
            assert_eq!(result.scale, object.transform.scale);
            undo_redo::undo(&mut tree);
            let restored = objects(&tree)[0].transform;
            assert_eq!(restored.translation, object.transform.translation);
            assert_eq!(restored.rotation, object.transform.rotation);
        }
    }

    #[test]
    fn picking_wood_without_selection_sets_material_for_new_objects_and_cutters() {
        let mut tree = DataTree::default();
        let material = Material::preset(MaterialKind::Wood);
        apply_material(&mut tree, material);
        commands::spawn(&mut tree, sdf_consts::TYPE_BOX);
        let target = objects(&tree)[0].uuid;
        scene_actions::add_operand(
            &mut tree,
            target,
            PrimitiveKind::Sphere,
            BooleanOperation::Subtract,
        );
        for object in objects(&tree) {
            assert_eq!(object.material.kind, MaterialKind::Wood);
            assert_eq!(object.color, material.color);
        }
    }

    #[test]
    fn operand_softness_slider_changes_geometry_and_undoes() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let mut operand = SdfObject::create_kind(PrimitiveKind::Sphere);
        operand.boolean_parent = Some(target.uuid);
        set_selected(&mut tree, vec![operand.uuid]);
        set_objects(&mut tree, vec![target, operand]);
        tree.make_undo_redo_snapshot();
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(350.0);
                    operand_panel(ui, &mut tree);
                },
            );
            output.textures_delta.clear();
            output.shapes
        };
        let shapes = frame(vec![]);
        let label = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Softness" => Some(text.pos),
                _ => None,
            })
            .expect("softness control");
        // Slider track occupies the left side of the same row.
        let position = egui::pos2(65.0, label.y + 7.0);
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(objects(&tree)[1].softness > 0.1);
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree)[1].softness, 0.05);
    }

    #[test]
    fn inline_rename_updates_the_tree_and_undoes() {
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Box);
        let id = object.uuid;
        set_objects(&mut tree, vec![object]);
        tree.make_undo_redo_snapshot();

        rename_object(&mut tree, id, "Workbench".into());

        assert_eq!(objects(&tree)[0].display_name(), "Workbench");
        undo_redo::undo(&mut tree);
        assert_eq!(objects(&tree)[0].display_name(), "Box");
        undo_redo::redo(&mut tree);
        assert_eq!(objects(&tree)[0].display_name(), "Workbench");
    }

    fn scene_frame(
        ctx: &egui::Context,
        tree: &mut DataTree,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<egui::epaint::ClippedShape> {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_width(350.0);
                scene_panel(ui, tree);
            },
        );
        output.textures_delta.clear();
        output.shapes
    }

    fn click_scene_text(
        ctx: &egui::Context,
        tree: &mut DataTree,
        label: &str,
        modifiers: egui::Modifiers,
    ) {
        let shapes = scene_frame(ctx, tree, vec![], modifiers);
        let position = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing scene control: {label}"));
        for pressed in [true, false] {
            scene_frame(
                ctx,
                tree,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    },
                ],
                modifiers,
            );
        }
    }

    #[test]
    fn scene_clicks_apply_subtraction_to_first_selected_target_and_can_detach() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut target = SdfObject::create(sdf_consts::TYPE_BOX);
        target.name = "Target shape".into();
        let mut cutter = SdfObject::create(sdf_consts::TYPE_SPHERE);
        cutter.name = "Cutter shape".into();
        let target_id = target.uuid;
        let cutter_id = cutter.uuid;
        // Reverse storage order proves the selection order determines the target.
        set_objects(&mut tree, vec![cutter, target]);
        tree.make_undo_redo_snapshot();
        click_scene_text(&ctx, &mut tree, "Target shape", egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "Cutter shape", egui::Modifiers::SHIFT);
        assert_eq!(selected(&tree), vec![target_id, cutter_id]);
        click_scene_text(&ctx, &mut tree, "Subtract", egui::Modifiers::NONE);
        let scene = objects(&tree);
        let cutter = scene
            .iter()
            .find(|object| object.uuid == cutter_id)
            .unwrap();
        assert_eq!(cutter.boolean_parent, Some(target_id));
        assert_eq!(cutter.operation, BooleanOperation::Subtract);
        assert!(crate::model::scene_sample(Vec3::ZERO, &scene).unwrap().0 > 0.0);
        edit_boolean_operand(&mut tree, cutter_id, None);
        assert!(objects(&tree)
            .iter()
            .all(|object| object.boolean_parent.is_none()));
        undo_redo::undo(&mut tree);
        assert_eq!(
            objects(&tree)
                .iter()
                .find(|object| object.uuid == cutter_id)
                .unwrap()
                .boolean_parent,
            Some(target_id)
        );
    }

    #[test]
    fn tree_drag_opens_operation_picker_and_subtracts_on_click() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let mut target = SdfObject::create_kind(PrimitiveKind::Box);
        target.name = "Drop target".into();
        let mut source = SdfObject::create_kind(PrimitiveKind::Sphere);
        source.name = "Dragged cutter".into();
        let target_id = target.uuid;
        let source_id = source.uuid;
        set_objects(&mut tree, vec![target, source]);
        tree.make_undo_redo_snapshot();
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        let shapes = scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        let position = |label: &str| {
            shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let start = position("Dragged cutter");
        let end = position("Drop target");
        scene_frame(
            &ctx,
            &mut tree,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerMoved(start + egui::vec2(15.0, 0.0))],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerMoved(end)],
            egui::Modifiers::NONE,
        );
        scene_frame(
            &ctx,
            &mut tree,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        assert_eq!(
            objects(&tree)[1].boolean_parent,
            None,
            "dropping awaits an explicit operation"
        );
        click_scene_text(&ctx, &mut tree, "−  Subtract", egui::Modifiers::NONE);
        let scene = objects(&tree);
        let cutter = scene
            .iter()
            .find(|object| object.uuid == source_id)
            .unwrap();
        assert_eq!(cutter.boolean_parent, Some(target_id));
        assert_eq!(cutter.operation, BooleanOperation::Subtract);
    }

    #[test]
    fn minus_menu_creates_a_cutter_with_left_clicks() {
        let ctx = egui::Context::default();
        let mut tree = DataTree::default();
        let target = SdfObject::create_kind(PrimitiveKind::Box);
        let target_id = target.uuid;
        set_objects(&mut tree, vec![target]);
        tree.make_undo_redo_snapshot();
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "−", egui::Modifiers::NONE);
        scene_frame(&ctx, &mut tree, vec![], egui::Modifiers::NONE);
        click_scene_text(&ctx, &mut tree, "Sphere", egui::Modifiers::NONE);
        let scene = objects(&tree);
        assert_eq!(scene.len(), 2);
        assert_eq!(scene[1].boolean_parent, Some(target_id));
        assert_eq!(scene[1].operation, BooleanOperation::Subtract);
        assert_eq!(selected(&tree), vec![scene[1].uuid]);
    }

    #[test]
    fn labels_avoid_overlays_and_viewport_edges() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));
        let occupied = [egui::Rect::from_min_max(
            egui::pos2(150.0, 50.0),
            egui::pos2(300.0, 150.0),
        )];
        let label = gizmo_label_rect(
            viewport,
            egui::pos2(140.0, 100.0),
            egui::vec2(90.0, 20.0),
            &occupied,
        )
        .unwrap();
        assert!(viewport.contains_rect(label));
        assert!(!label.intersects(occupied[0]));
    }

    #[test]
    fn viewport_toolbars_do_not_overlap_when_narrow() {
        let ctx = egui::Context::default();
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let mut camera = Camera::new();
        for width in [500.0, 300.0, 200.0, 140.0] {
            state.viewport_rect = Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 40.0),
                egui::vec2(width, 500.0),
            ));
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        state.regions.clear();
                        state.draw_top_controls(ui.ctx(), &mut tree, &mut camera);
                    },
                );
                output.textures_delta.clear();
                assert_eq!(state.regions.len(), 3);
                assert!(
                    !state.regions[0].intersects(state.regions[1])
                        && !state.regions[0].intersects(state.regions[2])
                        && !state.regions[1].intersects(state.regions[2]),
                    "overlapping toolbars at width {width}: {:?}",
                    state.regions
                );
            }
        }
    }

    #[test]
    fn toolbar_button_size_stays_fixed_on_hover_press_and_selection() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut rect = egui::Rect::NOTHING;
        for pass in 0..10 {
            let events = match pass {
                2 | 3 => vec![egui::Event::PointerMoved(rect.center())],
                4 => vec![egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                5 => vec![egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                8 => vec![egui::Event::PointerMoved(egui::pos2(500.0, 500.0))],
                _ => vec![],
            };
            let previous = rect;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    rect = selectable_view_button(
                        ui,
                        primitive_icon_source(PrimitiveKind::Box),
                        "Box",
                        pass >= 6,
                    )
                    .rect;
                },
            );
            output.textures_delta.clear();
            assert_eq!(rect.size(), egui::vec2(26.0, 26.0), "pass {pass}");
            if pass > 0 {
                assert_eq!(rect, previous, "pass {pass}");
            }
        }
    }

    #[test]
    fn compact_toolbar_buttons_and_top_right_isometric_control() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = view_button(ui, primitive_icon_source(PrimitiveKind::Box), "Add Box");
            assert_eq!(response.rect.width(), 26.0);
            assert_eq!(response.rect.width(), response.rect.height());
        });
        output.textures_delta.clear();
        let mut state = UiState::default();
        state.viewport_rect = Some(egui::Rect::from_min_size(
            egui::pos2(100.0, 40.0),
            egui::vec2(600.0, 500.0),
        ));
        let mut tree = DataTree::default();
        let mut camera = Camera::new();
        for pass in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(pass as f64),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    state.regions.clear();
                    state.draw_top_controls(ui.ctx(), &mut tree, &mut camera);
                    state.draw_view_gizmo(ui.ctx(), &mut camera);
                },
            );
            output.textures_delta.clear();
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if ["Isometric", "Perspective", "Orthographic"]
                            .contains(&text.galley.text()) =>
                    {
                        Some(text.pos)
                    }
                    _ => None,
                })
                .collect();
            if pass == 2 {
                assert!(labels.is_empty(), "view controls should be icon-only");
                let circles: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == Color32::from_black_alpha(165)
                                && state.regions[1].contains_rect(rect.rect) =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(circles.len(), 2);
                for circle in &circles {
                    assert_eq!(circle.width(), circle.height());
                    assert!(state.regions[1].contains_rect(*circle));
                }
                assert!(
                    (circles[0].center().y - circles[1].center().y).abs() < 2.0,
                    "top-right controls should be side by side"
                );
                assert!(circles[0].right() < circles[1].left());
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(rect) if state.regions[2].contains_rect(rect.rect) && rect.rect.width() > 80.0 && rect.fill != Color32::TRANSPARENT
                )), "bottom-left gizmo should have no background");
            }
        }
    }

    #[test]
    fn dragging_each_primitive_handle_outward_increases_its_dimension() {
        for (kind, handle_index) in PrimitiveKind::ALL.into_iter().flat_map(|kind| {
            let count = if kind == PrimitiveKind::Sphere { 3 } else { 1 };
            (0..count).map(move |index| (kind, index))
        }) {
            let ctx = egui::Context::default();
            let viewport =
                egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
            let mut camera = Camera::new();
            camera.viewport_origin = Vec2::new(100.0, 40.0);
            camera.viewport = Vec2::new(600.0, 500.0);
            let mut state = UiState::default();
            let mut tree = DataTree::default();
            let object = SdfObject::create_kind(kind);
            let handles = resize_handles(&object, &camera);
            let handle = &handles[handle_index];
            let start = camera.project(handle.world, 1.0).unwrap();
            let end = camera
                .project(handle.world + handle.direction * 0.2, 1.0)
                .unwrap();
            let before = handle.value;
            let original = object.clone();
            set_selected(&mut tree, vec![object.uuid]);
            set_objects(&mut tree, vec![object]);
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 700.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        state.regions.clear();
                        ui.set_clip_rect(viewport);
                        state.draw_object_gizmos(ui, &mut tree, &camera);
                    },
                );
                output.textures_delta.clear();
            };
            frame(vec![]);
            frame(vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            frame(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
            frame(vec![egui::Event::PointerMoved(end)]);
            frame(vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let resized = &objects(&tree)[0];
            let after = resize_handles(resized, &camera)[handle_index].value;
            if kind == PrimitiveKind::Sphere {
                for axis in 0..3 {
                    if axis != handle_index {
                        assert_eq!(
                            resized.transform.scale[axis],
                            original.transform.scale[axis]
                        );
                    }
                }
                let (SdfParams::SphereParams(before), SdfParams::SphereParams(after)) =
                    (&original.params, &resized.params)
                else {
                    unreachable!()
                };
                assert_eq!(
                    before.radius, after.radius,
                    "axis resize must not change the shared base radius"
                );
                assert_eq!(resized.transform.rotation, original.transform.rotation);
                assert_eq!(
                    resized.transform.translation,
                    original.transform.translation
                );
            }
            assert!(
                after > before,
                "{kind:?}: outward drag should increase dimension ({before} -> {after})"
            );
        }
    }

    #[test]
    fn resize_labels_only_appear_while_hovering_a_handle() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(600.0, 500.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(100.0, 40.0);
        camera.viewport = Vec2::new(600.0, 500.0);
        let mut tree = DataTree::default();
        let object = SdfObject::create_kind(PrimitiveKind::Sphere);
        let handle = camera
            .project(resize_handles(&object, &camera)[0].world, 1.0)
            .unwrap();
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut state = UiState::default();
        for (position, visible) in [
            (egui::pos2(10.0, 10.0), false),
            (handle, true),
            (egui::pos2(10.0, 10.0), false),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 700.0),
                    )),
                    events: vec![egui::Event::PointerMoved(position)],
                    ..Default::default()
                },
                |ui| {
                    state.regions.clear();
                    ui.set_clip_rect(viewport);
                    state.draw_object_gizmos(ui, &mut tree, &camera);
                },
            );
            output.textures_delta.clear();
            let has_label = output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().starts_with("Radius ")
                )
            });
            assert_eq!(has_label, visible);
        }
    }

    #[test]
    fn sphere_has_three_local_axis_radii_in_every_camera_view() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Sphere);
        object.transform.scale = Vec3::new(1.5, 0.8, 1.1);
        object.transform.rotation = glam::Quat::from_rotation_z(0.4);
        let inverse = object.transform.matrix().inverse();
        let SdfParams::SphereParams(params) = &object.params else {
            unreachable!()
        };
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800.0, 600.0);
        for angle in [
            ViewAngle::Front,
            ViewAngle::Back,
            ViewAngle::Left,
            ViewAngle::Right,
            ViewAngle::Top,
            ViewAngle::Bottom,
            ViewAngle::Isometric,
        ] {
            camera.snap(angle);
            for mode in [
                crate::camera::ProjectionMode::Perspective,
                crate::camera::ProjectionMode::Orthographic,
            ] {
                camera.projection_mode = mode;
                let handles = resize_handles(&object, &camera);
                assert_eq!(handles.len(), 3);
                for (axis, handle) in handles.into_iter().enumerate() {
                    let local_axis = [Vec3::X, Vec3::Y, Vec3::Z][axis];
                    assert_eq!(handle.parameter, axis);
                    let local = inverse.transform_point3(handle.world);
                    assert!((local[axis].abs() - params.radius).abs() < 0.0001);
                    assert!((local - local_axis * local[axis]).length() < 0.0001);
                    assert!(
                        (handle
                            .direction
                            .dot(object.transform.rotation * local_axis)
                            .abs()
                            - 1.0)
                            .abs()
                            < 0.0001
                    );
                    assert!(
                        handle
                            .direction
                            .dot(camera.position - object.transform.translation)
                            >= -0.0001
                    );
                    assert_eq!(
                        handle.value,
                        params.radius * object.transform.scale[axis].abs()
                    );
                }
            }
        }
    }

    fn assert_no_id_warnings(shape: &egui::Shape) {
        match shape {
            egui::Shape::Text(text) => {
                let text = text.galley.text();
                for warning in ["First use of", "Second use of", "Double use of"] {
                    assert!(!text.contains(warning), "egui ID warning: {text}");
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    assert_no_id_warnings(shape);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn duplicate_inspectors_and_narrow_viewports_have_no_id_warnings() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.warn_on_id_clash = true);
        let mut state = UiState::default();
        state
            .layout
            .add_pane_against_edge(DropSide::Bottom, 0.25, EditorPane::Object);
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut commands = Commands::new();
        let mut camera = Camera::new();
        let mut document = DocumentState::default();
        for width in [1200.0, 800.0, 480.0] {
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        state.draw(ui, &mut tree, &mut commands, &mut camera, &mut document);
                    },
                );
                output.textures_delta.clear(); // Headless test has no GPU texture consumer.
                for shape in output.shapes {
                    assert_no_id_warnings(&shape.shape);
                }
            }
        }
    }

    #[test]
    fn resize_gizmo_shapes_are_clipped_to_the_viewport() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(300.0, 250.0));
        let mut camera = Camera::new();
        camera.viewport_origin = Vec2::new(viewport.left(), viewport.top());
        camera.viewport = Vec2::new(viewport.width(), viewport.height());
        let mut state = UiState::default();
        let mut tree = DataTree::default();
        let object = SdfObject::create(sdf_consts::TYPE_BOX);
        set_selected(&mut tree, vec![object.uuid]);
        set_objects(&mut tree, vec![object]);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_clip_rect(viewport);
            state.draw_object_gizmos(ui, &mut tree, &camera);
        });
        output.textures_delta.clear();
        let mut painted = 0;
        for shape in output.shapes {
            if !matches!(shape.shape, egui::Shape::Noop) {
                assert!(viewport.contains_rect(shape.clip_rect));
                painted += 1;
            }
        }
        assert!(painted > 0, "test must render actual handles");
    }
}

fn axis_color(axis: usize) -> Color32 {
    match axis {
        0 => Color32::from_rgb(244, 88, 91),
        1 => Color32::from_rgb(91, 218, 119),
        _ => Color32::from_rgb(86, 149, 255),
    }
}

fn color32(color: Vec4) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.x * 255.0) as u8,
        (color.y * 255.0) as u8,
        (color.z * 255.0) as u8,
        (color.w * 255.0) as u8,
    )
}

fn material_preview(ui: &mut egui::Ui, kind: MaterialKind, material: Material) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(82.0, 70.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect(
        rect,
        5.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let preview = egui::Rect::from_min_max(
        rect.min + egui::vec2(7.0, 6.0),
        egui::pos2(rect.max.x - 7.0, rect.max.y - 22.0),
    );
    if kind == MaterialKind::Transparent {
        let size = 8.0;
        for row in 0..5 {
            for column in 0..9 {
                let tile = egui::Rect::from_min_size(
                    preview.min + egui::vec2(column as f32 * size, row as f32 * size),
                    egui::vec2(size, size),
                )
                .intersect(preview);
                let fill = if (row + column) % 2 == 0 {
                    Color32::from_gray(75)
                } else {
                    Color32::from_gray(42)
                };
                ui.painter().rect_filled(tile, 0.0, fill);
            }
        }
    }
    let center = preview.center();
    let radius = preview.height().min(preview.width()) * 0.37;
    ui.painter()
        .circle_filled(center, radius, color32(material.color));
    if kind == MaterialKind::Wood {
        for offset in -5..=5 {
            let x = offset as f32 * radius / 6.0;
            let height = (radius * radius - x * x).sqrt();
            ui.painter().line_segment(
                [
                    center + egui::vec2(x, -height),
                    center + egui::vec2(x, height),
                ],
                Stroke::new(1.2, Color32::from_black_alpha(65)),
            );
        }
    }
    ui.painter().circle_filled(
        center - egui::vec2(radius * 0.28, radius * 0.32),
        radius * (0.18 + material.reflectivity * 0.12),
        Color32::from_white_alpha((100.0 + material.reflectivity * 155.0) as u8),
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.max.y - 10.0),
        egui::Align2::CENTER_CENTER,
        kind.label(),
        egui::FontId::proportional(11.0),
        visuals.text_color(),
    );
    response
}
