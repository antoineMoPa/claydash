use serde::{Deserialize, Serialize};

use super::{ClaydashValue, DataTree};

/// A full-frame WGSL effect. `wgsl` is the body of `effect`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostProcessPass {
    pub uuid: uuid::Uuid,
    pub name: String,
    pub enabled: bool,
    pub wgsl: String,
}

impl PostProcessPass {
    pub const EXAMPLE: &'static str =
        "let tint = vec3<f32>(1.0, 0.85, 0.7);\nreturn vec4<f32>(color.rgb * tint, color.a);";

    pub fn new(name: String, wgsl: String) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name,
            enabled: true,
            wgsl,
        }
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
