use glam::{Vec2, Vec3, Vec4};
use observable_key_value_tree::{CanBeNone, ObservableKVTree};
use serde::{Deserialize, Serialize};

use super::{
    AnimationData, BooleanOperation, BoxFaceSelection, CurvePointSelection, EditorState, Material,
    MaterialAsset, ModelingFaceSelection, PostProcessPass, SdfObject, Transform, World,
};
use crate::camera::SceneCamera;

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    SceneVariables(super::SceneVariables),
    Animation(AnimationData),
    Material(Material),
    VecMaterialAsset(Vec<MaterialAsset>),
    VecPostProcessPass(Vec<PostProcessPass>),
    VecCamera(Vec<SceneCamera>),
    BooleanPick(BooleanPick),
    BoxFaceSelection(BoxFaceSelection),
    ModelingFaceSelection(ModelingFaceSelection),
    CurvePointSelection(CurvePointSelection),
    Uuid(uuid::Uuid),
    VecUuid(Vec<uuid::Uuid>),
    F32(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    RotationPivot(RotationPivot),
    Transform(Transform),
    VecSDFObject(Vec<SdfObject>),
    EditorState(EditorState),
    SelectionScope(SelectionScope),
    ViewportMode(ViewportMode),
    Bool(bool),
    World(super::World),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue>)),
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewportMode {
    #[default]
    FullMaterial,
    SimpleShading,
    Outline,
}

pub fn viewport_mode(tree: &DataTree) -> ViewportMode {
    match tree.get_path("editor.viewport_mode") {
        ClaydashValue::ViewportMode(mode) => mode,
        _ => ViewportMode::default(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionScope {
    #[default]
    Group,
    Exact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationPivot {
    #[default]
    ObjectCenter,
    Cursor,
}

pub fn rotation_pivot(tree: &DataTree) -> RotationPivot {
    match tree.get_path("editor.rotation_pivot") {
        ClaydashValue::RotationPivot(pivot) => pivot,
        _ => RotationPivot::ObjectCenter,
    }
}

pub fn cursor_position(tree: &DataTree) -> Vec3 {
    match tree.get_path("scene.cursor_position") {
        ClaydashValue::Vec3(position) => position,
        _ => Vec3::ZERO,
    }
}

pub fn snapping_enabled(tree: &DataTree, ctrl_down: bool, alt_down: bool) -> bool {
    !alt_down
        && (ctrl_down
            || matches!(
                tree.get_path("editor.persistent_snapping"),
                ClaydashValue::Bool(true)
            ))
}

#[cfg(test)]
mod snapping_tests {
    use super::*;

    #[test]
    fn guides_snap_on_demand_or_when_toolbar_toggle_is_enabled() {
        let mut tree = DataTree::default();
        assert!(!snapping_enabled(&tree, false, false));
        assert!(snapping_enabled(&tree, true, false));
        tree.set_transient_path("editor.persistent_snapping", ClaydashValue::Bool(true));
        assert!(snapping_enabled(&tree, false, false));
        assert!(!snapping_enabled(&tree, true, true));
    }
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
            name: material.display_name().to_string(),
            material,
            wgsl: None,
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
        .unwrap_or_else(|| material.display_name().to_owned());
    let mut asset = MaterialAsset::new(material);
    asset.name = format!("{source_name} copy");
    asset.wgsl = source_id
        .and_then(|id| assets.iter().find(|source| source.uuid == id))
        .and_then(|source| source.wgsl.clone());
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

pub fn selected_modeling_face(tree: &DataTree) -> Option<ModelingFaceSelection> {
    match tree.get_path("editor.selected_modeling_face") {
        ClaydashValue::ModelingFaceSelection(face) => Some(face),
        _ => selected_box_face(tree).map(ModelingFaceSelection::Box),
    }
}

pub fn selected_curve_point(tree: &DataTree) -> Option<CurvePointSelection> {
    match tree.get_path("editor.selected_curve_point") {
        ClaydashValue::CurvePointSelection(point) => Some(point),
        _ => None,
    }
}

pub fn set_selected_curve_point(tree: &mut DataTree, point: Option<CurvePointSelection>) {
    tree.set_transient_path(
        "editor.selected_curve_point",
        point.map_or(ClaydashValue::None, ClaydashValue::CurvePointSelection),
    );
}

pub fn set_selected_modeling_face(tree: &mut DataTree, face: Option<ModelingFaceSelection>) {
    tree.set_transient_path(
        "editor.selected_modeling_face",
        face.map_or(ClaydashValue::None, ClaydashValue::ModelingFaceSelection),
    );
    tree.set_transient_path(
        "editor.selected_box_face",
        match face {
            Some(ModelingFaceSelection::Box(face)) => ClaydashValue::BoxFaceSelection(face),
            _ => ClaydashValue::None,
        },
    );
}

pub fn set_selected_box_face(tree: &mut DataTree, face: Option<BoxFaceSelection>) {
    set_selected_modeling_face(tree, face.map(ModelingFaceSelection::Box));
}

pub fn set_objects(tree: &mut DataTree, mut value: Vec<SdfObject>) {
    let mut variables = super::scene_variables(tree);
    super::evaluate_derived_variables(&mut variables);
    let previous_count = variables.bindings.len() + variables.rigid_bindings.len();
    variables.bindings.retain(|binding| {
        !objects_ref(tree)
            .iter()
            .any(|object| object.uuid == binding.object)
            || value.iter().any(|object| object.uuid == binding.object)
    });
    variables.rigid_bindings.retain(|binding| {
        !objects_ref(tree)
            .iter()
            .any(|object| object.uuid == binding.object)
            || value.iter().any(|object| object.uuid == binding.object)
    });
    if variables.bindings.len() + variables.rigid_bindings.len() != previous_count
        || variables != super::scene_variables(tree)
    {
        tree.set_path(
            "scene.variables",
            ClaydashValue::SceneVariables(variables.clone()),
        );
    }
    super::apply_vector_bindings(&variables, &mut value);
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_objects_transient(tree: &mut DataTree, mut value: Vec<SdfObject>) {
    let mut variables = super::scene_variables(tree);
    super::evaluate_derived_variables(&mut variables);
    tree.set_transient_path(
        "scene.variables",
        ClaydashValue::SceneVariables(variables.clone()),
    );
    super::apply_vector_bindings(&variables, &mut value);
    tree.set_transient_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_selected(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    super::set_selected_variable(tree, None);
    set_selected_modeling_face(tree, None);
    set_selected_curve_point(tree, None);
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Group),
    );
    if selected_ref(tree) != value {
        tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
    }
}

pub fn set_selected_exact(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    super::set_selected_variable(tree, None);
    set_selected_modeling_face(tree, None);
    set_selected_curve_point(tree, None);
    tree.set_transient_path(
        "scene.selection_scope",
        ClaydashValue::SelectionScope(SelectionScope::Exact),
    );
    if selected_ref(tree) != value {
        tree.set_transient_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
    }
}

pub fn post_processing(tree: &DataTree) -> Vec<PostProcessPass> {
    match tree.get_path("scene.post_processing") {
        ClaydashValue::VecPostProcessPass(passes) => passes,
        _ => Vec::new(),
    }
}

pub fn post_processing_ref(tree: &DataTree) -> &[PostProcessPass] {
    match tree.get_path_ref("scene.post_processing") {
        Some(ClaydashValue::VecPostProcessPass(passes)) => passes,
        _ => &[],
    }
}

pub fn set_post_processing(tree: &mut DataTree, passes: Vec<PostProcessPass>) {
    tree.set_path(
        "scene.post_processing",
        ClaydashValue::VecPostProcessPass(passes),
    );
}

pub fn world(tree: &DataTree) -> World {
    match tree.get_path("scene.world") {
        ClaydashValue::World(world) => world,
        _ => World::default(),
    }
}
