use glam::{Vec2, Vec4};
use observable_key_value_tree::{CanBeNone, ObservableKVTree};
use serde::{Deserialize, Serialize};

use super::{
    AnimationData, BooleanOperation, BoxFaceSelection, EditorState, Material, MaterialAsset,
    SdfObject, Transform,
};
use crate::camera::SceneCamera;

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    Animation(AnimationData),
    Material(Material),
    VecMaterialAsset(Vec<MaterialAsset>),
    VecCamera(Vec<SceneCamera>),
    BooleanPick(BooleanPick),
    BoxFaceSelection(BoxFaceSelection),
    Uuid(uuid::Uuid),
    VecUuid(Vec<uuid::Uuid>),
    F32(f32),
    Vec2(Vec2),
    Vec4(Vec4),
    Transform(Transform),
    VecSDFObject(Vec<SdfObject>),
    EditorState(EditorState),
    SelectionScope(SelectionScope),
    Bool(bool),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue>)),
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionScope {
    #[default]
    Group,
    Exact,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct BooleanPick {
    pub target: uuid::Uuid,
    pub operation: BooleanOperation,
}

impl Default for ClaydashValue {
    fn default() -> Self {
        Self::None
    }
}

impl CanBeNone<ClaydashValue> for ClaydashValue {
    fn none() -> Self {
        Self::None
    }
}

pub type DataTree = ObservableKVTree<ClaydashValue>;

pub fn picked_material(tree: &DataTree) -> Material {
    match tree.get_path("editor.material") {
        ClaydashValue::Material(material) => material,
        _ => {
            let mut material = Material::default();
            if let ClaydashValue::Vec4(color) = tree.get_path("editor.color") {
                material.color = color;
            }
            material
        }
    }
}

pub fn picked_material_id(tree: &DataTree) -> Option<uuid::Uuid> {
    match tree.get_path("editor.material_id") {
        ClaydashValue::Uuid(id) => Some(id),
        _ => None,
    }
}

pub fn material_assets(tree: &DataTree) -> Vec<MaterialAsset> {
    match tree.get_path("scene.materials") {
        ClaydashValue::VecMaterialAsset(assets) => assets,
        _ => Vec::new(),
    }
}

pub fn set_material_assets(tree: &mut DataTree, assets: Vec<MaterialAsset>) {
    tree.set_path("scene.materials", ClaydashValue::VecMaterialAsset(assets));
}

pub fn ensure_material_asset(tree: &mut DataTree, material: Material) -> uuid::Uuid {
    let mut assets = material_assets(tree);
    if let Some(asset) = assets.iter().find(|asset| asset.material == material) {
        return asset.uuid;
    }
    let asset = MaterialAsset::new(material);
    let id = asset.uuid;
    assets.push(asset);
    set_material_assets(tree, assets);
    id
}

pub fn update_material_asset(tree: &mut DataTree, id: uuid::Uuid, material: Material) {
    let mut assets = material_assets(tree);
    if let Some(asset) = assets.iter_mut().find(|asset| asset.uuid == id) {
        asset.material = material;
    } else {
        assets.push(MaterialAsset {
            uuid: id,
            name: material.kind.label().to_string(),
            material,
        });
    }
    set_material_assets(tree, assets);
}

pub fn rename_material_asset(tree: &mut DataTree, id: uuid::Uuid, name: String) -> bool {
    let mut assets = material_assets(tree);
    let Some(asset) = assets.iter_mut().find(|asset| asset.uuid == id) else {
        return false;
    };
    if asset.name == name {
        return false;
    }
    asset.name = name;
    set_material_assets(tree, assets);
    true
}

pub fn create_unlinked_material_asset(
    tree: &mut DataTree,
    source_id: Option<uuid::Uuid>,
    material: Material,
) -> uuid::Uuid {
    let mut assets = material_assets(tree);
    let source_name = source_id
        .and_then(|id| {
            assets
                .iter()
                .find(|asset| asset.uuid == id)
                .map(|asset| asset.name.clone())
        })
        .unwrap_or_else(|| material.kind.label().to_owned());
    let mut asset = MaterialAsset::new(material);
    asset.name = format!("{source_name} copy");
    let id = asset.uuid;
    assets.push(asset);
    set_material_assets(tree, assets);
    id
}

pub fn scene_cameras(tree: &DataTree) -> Vec<SceneCamera> {
    match tree.get_path("scene.cameras") {
        ClaydashValue::VecCamera(cameras) => cameras,
        _ => Vec::new(),
    }
}

pub fn set_scene_cameras(tree: &mut DataTree, cameras: Vec<SceneCamera>) {
    tree.set_path("scene.cameras", ClaydashValue::VecCamera(cameras));
}

pub fn active_camera_id(tree: &DataTree) -> Option<uuid::Uuid> {
    match tree.get_path("scene.active_camera") {
        ClaydashValue::Uuid(id) => Some(id),
        _ => None,
    }
}

pub fn boolean_distance(a: f32, b: f32, operation: BooleanOperation, softness: f32) -> f32 {
    let b = if operation == BooleanOperation::Subtract {
        -b
    } else {
        b
    };
    let hard = if operation == BooleanOperation::Union {
        a.min(b)
    } else {
        a.max(b)
    };
    if softness <= 0.0 {
        return hard;
    }
    let h = (softness - (a - b).abs()).max(0.0) / softness;
    let blend = softness * h * h * 0.25;
    if operation == BooleanOperation::Union {
        hard - blend
    } else {
        hard + blend
    }
}

pub fn objects(tree: &DataTree) -> Vec<SdfObject> {
    match tree.get_path("scene.sdf_objects") {
        ClaydashValue::VecSDFObject(value) => value,
        _ => vec![],
    }
}

pub fn selected(tree: &DataTree) -> Vec<uuid::Uuid> {
    match tree.get_path("scene.selected_uuids") {
        ClaydashValue::VecUuid(value) => value,
        _ => vec![],
    }
}

pub fn objects_ref(tree: &DataTree) -> &[SdfObject] {
    match tree.get_path_ref("scene.sdf_objects") {
        Some(ClaydashValue::VecSDFObject(value)) => value,
        _ => &[],
    }
}

pub fn selected_ref(tree: &DataTree) -> &[uuid::Uuid] {
    match tree.get_path_ref("scene.selected_uuids") {
        Some(ClaydashValue::VecUuid(value)) => value,
        _ => &[],
    }
}

pub fn selection_scope(tree: &DataTree) -> SelectionScope {
    match tree.get_path("scene.selection_scope") {
        ClaydashValue::SelectionScope(scope) => scope,
        _ => SelectionScope::Group,
    }
}

pub fn selected_box_face(tree: &DataTree) -> Option<BoxFaceSelection> {
    match tree.get_path("editor.selected_box_face") {
        ClaydashValue::BoxFaceSelection(face) => Some(face),
        _ => None,
    }
}

pub fn set_selected_box_face(tree: &mut DataTree, face: Option<BoxFaceSelection>) {
    tree.set_transient_path(
        "editor.selected_box_face",
        face.map_or(ClaydashValue::None, ClaydashValue::BoxFaceSelection),
    );
}

pub fn set_objects(tree: &mut DataTree, value: Vec<SdfObject>) {
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_objects_transient(tree: &mut DataTree, value: Vec<SdfObject>) {
    tree.set_transient_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_selected(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    set_selected_box_face(tree, None);
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Group),
    );
    tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
}

pub fn set_selected_exact(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    set_selected_box_face(tree, None);
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Exact),
    );
    tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
}
