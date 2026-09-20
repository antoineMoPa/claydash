use std::path::{Path, PathBuf};

use observable_key_value_tree::ObservableKVTree;

use crate::model::{ClaydashValue, DataTree};

#[cfg(not(target_arch = "wasm32"))]
const PROJECT_EXTENSION: &str = "claydash";
#[cfg(not(target_arch = "wasm32"))]
const RECENT_PROJECT_LIMIT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileMenuAction {
    Open,
    OpenRecent(PathBuf),
    Save,
    SaveAs,
}

pub struct DocumentState {
    current_path: Option<PathBuf>,
    recent_paths: Vec<PathBuf>,
    error: Option<String>,
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            current_path: None,
            recent_paths: load_recent_paths(),
            error: None,
        }
    }
}

impl DocumentState {
    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    pub fn recent_paths(&self) -> &[PathBuf] {
        &self.recent_paths
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

pub fn serialize_scene(tree: &DataTree) -> Result<Vec<u8>, String> {
    let scene = tree
        .get_tree("scene")
        .ok_or_else(|| "the document has no scene".to_string())?;
    serde_json::to_vec_pretty(&scene).map_err(|error| error.to_string())
}

pub fn deserialize_scene(bytes: &[u8]) -> Result<ObservableKVTree<ClaydashValue>, String> {
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
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

#[cfg(target_arch = "wasm32")]
fn load_recent_paths() -> Vec<PathBuf> {
    Vec::new()
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

        let bytes = serialize_scene(&tree).unwrap();
        let scene = deserialize_scene(&bytes).unwrap();
        let mut restored = DataTree::default();
        restored.set_tree("scene", scene);

        assert_eq!(objects(&restored)[0].uuid, object.uuid);
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

    #[test]
    fn malformed_project_is_rejected() {
        assert!(deserialize_scene(b"not json").is_err());
    }

    #[test]
    fn bundled_default_scene_loads() {
        let scene = deserialize_scene(crate::duck::DEFAULT_DUCK.as_bytes()).unwrap();
        let mut tree = DataTree::default();
        tree.set_tree("scene", scene);

        let objects = objects(&tree);
        assert!(!objects.is_empty());
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
