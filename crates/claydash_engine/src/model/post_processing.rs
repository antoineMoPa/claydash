use serde::{Deserialize, Serialize};


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

