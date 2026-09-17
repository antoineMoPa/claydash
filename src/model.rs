use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use observable_key_value_tree::{CanBeNone, ObservableKVTree};
use sdf_consts::TYPE_BOX;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorState {
    Start,
    Grabbing,
    Scaling,
    Rotating,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BoxParams {
    pub box_q: Vec3,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SphereParams {
    pub radius: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SdfParams {
    BoxParams(BoxParams),
    SphereParams(SphereParams),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SdfObject {
    pub uuid: uuid::Uuid,
    pub transform: Transform,
    pub color: Vec4,
    pub object_type: i32,
    pub params: SdfParams,
}

impl SdfObject {
    pub fn create(object_type: i32) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            transform: Transform::default(),
            color: Vec4::new(0.8, 0.0, 0.3, 1.0),
            object_type,
            params: if object_type == TYPE_BOX {
                SdfParams::BoxParams(BoxParams {
                    box_q: Vec3::splat(0.3),
                })
            } else {
                SdfParams::SphereParams(SphereParams { radius: 0.2 })
            },
        }
    }

    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.uuid = uuid::Uuid::new_v4();
        copy
    }

    pub fn distance(&self, point: Vec3) -> f32 {
        let local = (self.transform.matrix().inverse() * point.extend(1.0)).truncate();
        let distance = match self.params {
            SdfParams::SphereParams(ref params) => local.length() - params.radius,
            SdfParams::BoxParams(ref params) => {
                let q = local.abs() - params.box_q;
                q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
            }
        };
        distance * self.transform.scale.abs().min_element()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ClaydashValue {
    Uuid(uuid::Uuid),
    VecUuid(Vec<uuid::Uuid>),
    F32(f32),
    Vec2(Vec2),
    Vec4(Vec4),
    Transform(Transform),
    VecSDFObject(Vec<SdfObject>),
    EditorState(EditorState),
    Bool(bool),
    #[serde(skip)]
    Fn(fn(&mut ObservableKVTree<ClaydashValue>)),
    None,
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

pub fn set_objects(tree: &mut DataTree, value: Vec<SdfObject>) {
    tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(value));
}

pub fn set_selected(tree: &mut DataTree, value: Vec<uuid::Uuid>) {
    tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(value));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_stress_scene() -> Vec<SdfObject> {
    use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

    let mut objects = Vec::with_capacity(256);
    for z in 0..4 {
        for y in 0..8 {
            for x in 0..8 {
                let object_type = if (x + y + z) % 2 == 0 {
                    TYPE_SPHERE
                } else {
                    TYPE_BOX
                };
                let mut object = SdfObject::create(object_type);
                object.transform.translation = Vec3::new(
                    (x as f32 - 3.5) * 0.42,
                    (y as f32 - 3.5) * 0.42,
                    (z as f32 - 1.5) * 0.42,
                );
                object.transform.rotation = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    x as f32 * 0.11,
                    y as f32 * 0.07,
                    z as f32 * 0.17,
                );
                object.transform.scale = Vec3::new(
                    0.8 + (x % 3) as f32 * 0.14,
                    0.8 + (y % 3) as f32 * 0.14,
                    0.8 + (z % 3) as f32 * 0.14,
                );
                object.color = Vec4::new(
                    0.25 + x as f32 * 0.07,
                    0.2 + y as f32 * 0.06,
                    0.35 + z as f32 * 0.14,
                    1.0,
                );
                objects.push(object);
            }
        }
    }
    objects
}
