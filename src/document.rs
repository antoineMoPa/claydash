use std::path::{Path, PathBuf};

use observable_key_value_tree::ObservableKVTree;

use crate::model::{ClaydashValue, DataTree};

#[cfg(not(target_arch = "wasm32"))]
const PROJECT_EXTENSION: &str = "claydash";
#[cfg(not(target_arch = "wasm32"))]
const RECENT_PROJECT_LIMIT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileMenuAction {
    New,
    Clear,
    Open,
    OpenRecent(PathBuf),
    OpenExample(crate::examples::Example),
    Guide,
    Save,
    SaveAs,
    #[cfg(target_arch = "wasm32")]
    SaveNamed(String),
    Render(RenderFormat),
    ExportGlb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderFormat {
    WebP,
    Mp4,
}

impl RenderFormat {
    pub const ALL: [Self; 2] = [Self::WebP, Self::Mp4];

    pub fn label(self) -> &'static str {
        match self {
            Self::WebP => "WebP image",
            Self::Mp4 => "MP4 video",
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn extension(self) -> &'static str {
        match self {
            Self::WebP => "webp",
            Self::Mp4 => "mp4",
        }
    }
}

pub struct DocumentState {
    current_path: Option<PathBuf>,
    saved_scene: Option<Vec<u8>>,
    observed_authored_version: u64,
    dirty: bool,
    recent_paths: Vec<PathBuf>,
    ui_preferences: UiPreferences,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct UiPreferences {
    #[serde(default)]
    animation_timeline_open: bool,
    #[serde(default)]
    color_theme: ColorTheme,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ColorTheme {
    #[default]
    Dark,
    Light,
}

impl ColorTheme {
    pub fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            current_path: None,
            saved_scene: None,
            observed_authored_version: 0,
            dirty: false,
            recent_paths: load_recent_paths(),
            ui_preferences: load_ui_preferences(),
            error: None,
        }
    }
}

impl DocumentState {
    pub fn start_new(&mut self) {
        self.current_path = None;
        self.error = None;
        self.saved_scene = None;
        self.observed_authored_version = 0;
        self.dirty = false;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_scene_clean(&mut self, tree: &DataTree) {
        self.saved_scene = serialize_scene(tree).ok();
        self.observed_authored_version = tree.authored_version();
        self.dirty = false;
    }

    pub fn refresh_dirty(&mut self, tree: &DataTree) {
        let version = tree.authored_version();
        if version == self.observed_authored_version {
            return;
        }
        self.observed_authored_version = version;
        let current = serialize_scene(tree).ok();
        if self.saved_scene.is_none() {
            self.saved_scene = current;
            return;
        }
        self.dirty = current != self.saved_scene;
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    pub fn recent_paths(&self) -> &[PathBuf] {
        &self.recent_paths
    }

    pub fn animation_timeline_open(&self) -> bool {
        self.ui_preferences.animation_timeline_open
    }

    pub fn color_theme(&self) -> ColorTheme {
        self.ui_preferences.color_theme
    }

    pub fn set_color_theme(&mut self, theme: ColorTheme) {
        if self.ui_preferences.color_theme == theme {
            return;
        }
        self.ui_preferences.color_theme = theme;
        save_ui_preferences(self.ui_preferences);
    }

    pub fn set_animation_timeline_open(&mut self, open: bool) {
        if self.ui_preferences.animation_timeline_open == open {
            return;
        }
        self.ui_preferences.animation_timeline_open = open;
        save_ui_preferences(self.ui_preferences);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn set_error(&mut self, operation: &str, error: impl std::fmt::Display) {
        self.error = Some(format!("Could not {operation}: {error}"));
    }

    pub fn mark_opened(&mut self, path: PathBuf) {
        self.current_path = Some(path.clone());
        self.remember(path);
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.current_path = Some(path.clone());
        self.remember(path);
    }

    fn remember(&mut self, path: PathBuf) {
        #[cfg(target_arch = "wasm32")]
        let _ = path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recent_paths.retain(|recent| recent != &path);
            self.recent_paths.insert(0, path);
            self.recent_paths.truncate(RECENT_PROJECT_LIMIT);
            save_recent_paths(&self.recent_paths);
        }
    }
}

#[cfg(test)]
mod new_document_tests {
    use super::*;

    #[test]
    fn starting_new_document_clears_save_target_and_preserves_recent_projects() {
        let previous = PathBuf::from("previous.claydash");
        let mut document = DocumentState::default();
        document.current_path = Some(previous.clone());
        document.recent_paths = vec![previous.clone()];
        document.set_error("save the project", "disk full");

        document.start_new();

        assert!(document.current_path().is_none());
        assert_eq!(document.recent_paths(), &[previous]);
        assert!(document.error().is_none());
    }

    #[test]
    fn dirty_state_follows_scene_edits_and_undo_to_saved_content() {
        let mut tree = DataTree::default();
        tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(Vec::new()));
        tree.set_path(
            "scene.cursor_position",
            ClaydashValue::Vec3(glam::Vec3::ZERO),
        );
        let mut document = DocumentState::default();
        document.mark_scene_clean(&tree);
        tree.set_transient_path("scene.cursor_position", ClaydashValue::Vec3(glam::Vec3::Y));
        document.refresh_dirty(&tree);
        assert!(!document.is_dirty());
        tree.set_path("scene.cursor_position", ClaydashValue::Vec3(glam::Vec3::X));
        document.refresh_dirty(&tree);
        assert!(document.is_dirty());
        tree.set_path(
            "scene.cursor_position",
            ClaydashValue::Vec3(glam::Vec3::ZERO),
        );
        document.refresh_dirty(&tree);
        assert!(!document.is_dirty());
    }
}

pub fn serialize_scene(tree: &DataTree) -> Result<Vec<u8>, String> {
    let scene = tree
        .get_tree("scene")
        .ok_or_else(|| "the document has no scene".to_string())?;
    serde_json::to_vec_pretty(&scene).map_err(|error| error.to_string())
}

pub fn deserialize_scene(bytes: &[u8]) -> Result<ObservableKVTree<ClaydashValue>, String> {
    let mut scene: ObservableKVTree<ClaydashValue> =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if let ClaydashValue::VecSDFObject(mut objects) = scene.get_path("sdf_objects") {
        let ids: std::collections::HashSet<_> = objects.iter().map(|object| object.uuid).collect();
        let mut repaired = false;
        for object in &mut objects {
            if object
                .boolean_parent
                .is_some_and(|parent| !ids.contains(&parent))
            {
                object.boolean_parent = None;
                object.operation = crate::model::BooleanOperation::Union;
                object.name = format!("Recovered {}", object.name);
                repaired = true;
            }
        }
        if repaired {
            scene.set_path("sdf_objects", ClaydashValue::VecSDFObject(objects));
        }
    }
    Ok(scene)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function revokeDownloadUrlLater(url) {
    setTimeout(() => URL.revokeObjectURL(url), 60000);
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = revokeDownloadUrlLater)]
    fn revoke_download_url_later(url: &str);
}

#[cfg(target_arch = "wasm32")]
pub fn download_bytes(file_name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let byte_array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&byte_array.buffer());
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|error| format!("could not create download: {error:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|error| format!("could not create download URL: {error:?}"))?;
    let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "browser document is unavailable".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|error| format!("could not create download link: {error:?}"))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not prepare download link".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(file_name);
    anchor
        .style()
        .set_property("display", "none")
        .map_err(|error| format!("could not hide download link: {error:?}"))?;
    let body = document
        .body()
        .ok_or_else(|| "browser document body is unavailable".to_string())?;
    body.append_child(&anchor)
        .map_err(|error| format!("could not attach download link: {error:?}"))?;
    anchor.click();
    let _ = body.remove_child(&anchor);
    revoke_download_url_later(&url);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_scene(path: &Path) -> Result<ObservableKVTree<ClaydashValue>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    deserialize_scene(&bytes)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_scene(path: &Path, tree: &DataTree) -> Result<(), String> {
    let bytes = serialize_scene(tree)?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Claydash project", &[PROJECT_EXTENSION])
        .pick_file()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Claydash project", &[PROJECT_EXTENSION])
        .set_file_name(format!("untitled.{PROJECT_EXTENSION}"))
        .save_file()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn render_dialog(format: RenderFormat) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(format.label(), &[format.extension()])
        .set_file_name(format!("render.{}", format.extension()))
        .save_file()
}

#[cfg(not(target_arch = "wasm32"))]
fn recent_projects_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home).join("Library/Application Support/claydash/recent-projects.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("claydash/recent-projects.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
            Some(PathBuf::from(path).join("claydash/recent-projects.json"))
        } else {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".config/claydash/recent-projects.json"))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ui_preferences_path() -> Option<PathBuf> {
    recent_projects_path().map(|path| path.with_file_name("ui-preferences.json"))
}

#[cfg(target_arch = "wasm32")]
fn load_recent_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_arch = "wasm32")]
fn load_ui_preferences() -> UiPreferences {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("claydash-ui-preferences").ok().flatten())
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn load_ui_preferences() -> UiPreferences {
    let Some(path) = ui_preferences_path() else {
        return UiPreferences::default();
    };
    read_ui_preferences(&path)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_ui_preferences(path: &Path) -> UiPreferences {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn save_ui_preferences(preferences: UiPreferences) {
    if let (Some(storage), Ok(value)) = (
        web_sys::window().and_then(|window| window.local_storage().ok().flatten()),
        serde_json::to_string(&preferences),
    ) {
        let _ = storage.set_item("claydash-ui-preferences", &value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_ui_preferences(preferences: UiPreferences) {
    let Some(path) = ui_preferences_path() else {
        return;
    };
    let _ = write_ui_preferences(&path, preferences);
}

#[cfg(not(target_arch = "wasm32"))]
fn write_ui_preferences(path: &Path, preferences: UiPreferences) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("preferences path has no parent directory".into());
    };
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn load_recent_paths() -> Vec<PathBuf> {
    let Some(path) = recent_projects_path() else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<PathBuf>>(&bytes).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn save_recent_paths(paths: &[PathBuf]) {
    let Some(path) = recent_projects_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(paths) {
        let _ = std::fs::write(path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        animation,
        model::{
            objects, set_objects, AnimatableProperty, AnimationBinding, SdfObject, VectorAxis,
        },
    };
    use sdf_consts::TYPE_SPHERE;

    #[test]
    fn scene_round_trips_without_editor_state() {
        let mut tree = DataTree::default();
        let object = SdfObject::create(TYPE_SPHERE);
        set_objects(&mut tree, vec![object.clone()]);
        tree.set_path("editor.color", ClaydashValue::F32(0.25));
        tree.set_path(
            "scene.cursor_position",
            ClaydashValue::Vec3(glam::Vec3::new(1.0, 2.0, 3.0)),
        );

        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);

        assert_eq!(objects(&restored)[0].uuid, object.uuid);
        assert_eq!(
            crate::model::cursor_position(&restored),
            glam::Vec3::new(1.0, 2.0, 3.0)
        );
        assert!(matches!(
            restored.get_path("editor.color"),
            ClaydashValue::None
        ));
    }

    #[test]
    fn scene_round_trip_keeps_animation_tracks() {
        let mut tree = DataTree::default();
        let object = SdfObject::create(TYPE_SPHERE);
        let binding = AnimationBinding {
            object: object.uuid,
            property: AnimatableProperty::Position(VectorAxis::X),
        };
        set_objects(&mut tree, vec![object]);
        animation::insert_keyframe(&mut tree, binding, 0, -1.0);
        animation::insert_keyframe(&mut tree, binding, 12, 2.0);

        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);

        let tracks = animation::animation_data(&restored).tracks;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].binding, binding);
        assert_eq!(tracks[0].keyframes.len(), 2);
    }

    #[test]
    fn scene_round_trip_keeps_shared_material_assets() {
        let mut tree = DataTree::default();
        let material = crate::model::Material::preset(crate::model::MaterialKind::Wood);
        let id = crate::model::ensure_material_asset(&mut tree, material);
        let mut first = SdfObject::create(TYPE_SPHERE);
        first.material = material;
        first.material_id = Some(id);
        let mut second = SdfObject::create(TYPE_SPHERE);
        second.material = material;
        second.material_id = Some(id);
        set_objects(&mut tree, vec![first, second]);

        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        assert_eq!(crate::model::material_assets(&restored).len(), 1);
        assert!(objects(&restored)
            .iter()
            .all(|object| object.material_id == Some(id)));
    }

    #[test]
    fn scene_round_trip_keeps_fabric_settings_and_link() {
        let mut tree = DataTree::default();
        let mut material =
            crate::model::Material::fabric_preset(crate::model::FabricPreset::HeatherJersey);
        material.fabric.pitch_x = 0.027;
        material.fabric.light_yarn_fraction = 0.47;
        let id = crate::model::ensure_material_asset(&mut tree, material);
        let mut object = SdfObject::create(TYPE_SPHERE);
        object.material = material;
        object.material_id = Some(id);
        object.color = material.color;
        set_objects(&mut tree, vec![object]);
        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        let restored_object = &objects(&restored)[0];
        assert_eq!(restored_object.material, material);
        assert_eq!(restored_object.material_id, Some(id));
    }

    #[test]
    fn scene_round_trip_keeps_metal_layers_tint_and_object_height_image() {
        let mut tree = DataTree::default();
        let mut material = crate::model::MetalStudy::ChippedPaint.material();
        material.metal.image_relief = 0.72;
        material.metal.brush_angle = -0.61;
        material.metal.texture_scale = 7.2;
        let id = crate::model::ensure_material_asset(&mut tree, material);
        let mut object = SdfObject::create(TYPE_SPHERE);
        object.material = material;
        object.material_id = Some(id);
        object.color = glam::Vec4::new(0.8, 0.6, 0.9, 1.0);
        let mut stencil =
            crate::model::ImageStencil::new("height.png".into(), vec![1, 2, 3], false);
        stencil.rotation = 42.0;
        stencil.offset = glam::Vec2::new(0.25, -0.11);
        object.image_stencil = Some(stencil);
        set_objects(&mut tree, vec![object.clone()]);
        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        let loaded = &objects(&restored)[0];
        assert_eq!(loaded.material, material);
        assert_eq!(loaded.material_id, Some(id));
        assert_eq!(loaded.color, object.color);
        let loaded_image = loaded.image_stencil.as_ref().unwrap();
        let original_image = object.image_stencil.as_ref().unwrap();
        assert_eq!(loaded_image.image, original_image.image);
        assert_eq!(loaded_image.offset, original_image.offset);
        assert_eq!(loaded_image.rotation, original_image.rotation);
        assert_eq!(
            crate::model::material_assets(&restored)[0].material,
            material
        );
    }

    #[test]
    fn scene_round_trip_keeps_custom_material_source() {
        let mut tree = DataTree::default();
        let asset = crate::model::MaterialAsset::custom("Ocean".into());
        let id = asset.uuid;
        let source = asset.wgsl.clone();
        crate::model::set_material_assets(&mut tree, vec![asset]);
        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        let assets = crate::model::material_assets(&restored);
        assert_eq!(assets[0].uuid, id);
        assert_eq!(assets[0].wgsl, source);
    }

    #[test]
    fn scene_round_trip_keeps_camera_objects_and_active_camera() {
        let mut tree = DataTree::default();
        let camera =
            crate::camera::SceneCamera::from_view("Main shot", &crate::camera::Camera::new());
        let id = camera.uuid;
        crate::model::set_scene_cameras(&mut tree, vec![camera]);
        tree.set_path("scene.active_camera", ClaydashValue::Uuid(id));

        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);
        assert_eq!(crate::model::scene_cameras(&restored)[0].uuid, id);
        assert_eq!(crate::model::active_camera_id(&restored), Some(id));
    }

    #[test]
    fn recent_projects_are_most_recent_first_and_bounded() {
        let mut paths = Vec::new();
        for index in 0..12 {
            remember_path(
                &mut paths,
                PathBuf::from(format!("project-{index}.claydash")),
            );
        }
        remember_path(&mut paths, PathBuf::from("project-5.claydash"));

        assert_eq!(paths.len(), RECENT_PROJECT_LIMIT);
        assert_eq!(paths[0], PathBuf::from("project-5.claydash"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_os_str() == "project-5.claydash")
                .count(),
            1
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn animation_timeline_preference_round_trips_between_sessions() {
        let directory = std::env::temp_dir().join(format!(
            "claydash-ui-preferences-test-{}",
            uuid::Uuid::new_v4()
        ));
        let path = directory.join("ui-preferences.json");
        let preferences = UiPreferences {
            animation_timeline_open: true,
            color_theme: ColorTheme::Light,
        };

        write_ui_preferences(&path, preferences).unwrap();

        assert_eq!(read_ui_preferences(&path), preferences);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn older_ui_preferences_default_to_dark_mode() {
        let preferences: UiPreferences =
            serde_json::from_str(r#"{"animation_timeline_open":true}"#).unwrap();
        assert!(preferences.animation_timeline_open);
        assert_eq!(preferences.color_theme, ColorTheme::Dark);
    }

    #[test]
    fn malformed_project_is_rejected() {
        assert!(deserialize_scene(b"not json").is_err());
    }

    #[test]
    fn opening_a_project_recovers_a_split_void_whose_parent_was_deleted() {
        let mut tree = DataTree::default();
        let mut split_void =
            crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::PolygonPrism);
        split_void.name = "Face split void".into();
        split_void.boolean_parent = Some(uuid::Uuid::new_v4());
        split_void.operation = crate::model::BooleanOperation::Subtract;
        crate::model::set_objects(&mut tree, vec![split_void.clone()]);
        let bytes = serialize_scene(&tree).unwrap();

        let mut restored = DataTree::default();
        restored.set_tree("scene", deserialize_scene(&bytes).unwrap());

        let objects = crate::model::objects(&restored);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].boolean_parent, None);
        assert_eq!(objects[0].operation, crate::model::BooleanOperation::Union);
        assert_eq!(objects[0].name, "Recovered Face split void");
    }

    #[test]
    fn bundled_default_scene_loads() {
        let scene = deserialize_scene(crate::duck::DEFAULT_DUCK.as_bytes()).unwrap();
        let mut tree = DataTree::default();
        tree.set_tree("scene", scene);

        let objects = objects(&tree);
        assert!(!objects.is_empty());
        assert!(objects.iter().all(|object| !object.name.is_empty()));
        assert!(objects
            .iter()
            .all(|object| object.color == object.material.color));
        let group = objects
            .iter()
            .find(|candidate| {
                objects
                    .iter()
                    .any(|object| object.boolean_parent == Some(candidate.uuid))
            })
            .expect("default document includes its Boolean group");
        assert_ne!(group.group_transform, crate::model::Transform::default());
    }

    fn remember_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
        paths.retain(|recent| recent != &path);
        paths.insert(0, path);
        paths.truncate(RECENT_PROJECT_LIMIT);
    }
}
