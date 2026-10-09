use super::*;

const SHARED: &str = r#"
struct TrainParams {
    world: mat4x4<f32>, cube: vec4<f32>, control: vec4<u32>,
    options: vec4<f32>, component: vec4<u32>, distance_target: vec4<f32>,
}
struct TrainSample { point: vec4<f32>, owner: vec4<u32>, ray: vec4<f32>, distance_vector: vec4<f32> }
"#;

pub(super) fn source_shader(capacity: u32) -> String {
    let source = crate::renderer::scene_bounds::specialized_neural_shader_source(
        &crate::renderer::scene_bounds::specialized_shader_source(
            &crate::renderer::material_gpu::shader_source(),
            capacity,
        ),
        4,
    );
    // Training reads editable primitives, never captured proxies. Reuse the
    // native branches of the renderer evaluator without binding derived atlases.
    let start = source.find("fn primitive_distance(").unwrap();
    let end = source[start..]
        .find("    } else if HAS_NEURAL_SDF")
        .unwrap()
        + start;
    let native = format!(
        "{}    }}\n    return distance;\n}}\n",
        source[start..end].replace(
            "fn primitive_distance(",
            "fn train_native_primitive_distance("
        )
    );
    let source = source.replace(
        "brick_geometry_visible(object)",
        "train_brick_geometry_enabled(object)",
    );
    let source = source.replace(
        "fabric_geometry_visible(object)",
        "train_fabric_geometry_enabled(object)",
    );
    let native = format!("{native}\nfn train_brick_geometry_enabled(object: Object) -> bool {{\n    let header = material_headers[object.component.w];\n    return header.kind == MATERIAL_BRICK && material_params[header.offset + BRICK_RELIEF].x >= 0.001;\n}}\n");
    let native = format!("{native}\nfn train_fabric_geometry_enabled(object: Object) -> bool {{\n    if object.state.y != 1 && object.state.y != 2 {{ return false; }}\n    let header = material_headers[object.component.w];\n    return header.kind == MATERIAL_FABRIC && material_params[header.offset + FABRIC_PITCH].z >= 0.0005;\n}}\n");
    // Route every primitive query in the base evaluator through the editable
    // source, including the finite radial-copy loop. Derived capture buffers
    // must remain unreachable from the portable training entry point.
    let base_start = source.find("fn base_object_distance_at(").unwrap();
    let base_end = base_start + source[base_start..].find("fn combine_operand(").unwrap();
    let source = format!(
        "{}{}{}",
        &source[..base_start],
        source[base_start..base_end].replace(
            "primitive_distance(",
            "train_native_primitive_distance("
        ),
        &source[base_end..],
    );
    format!(
        "{source}\n{native}\n{SHARED}\n{}",
        include_str!("../../../../assets/shaders/neural_sampling.wgsl")
    )
}
pub(super) fn model_shader(settings: NeuralTrainingSettings) -> String {
    format!("{SHARED}\nconst TRAIN_WIDTH: u32 = {}u;\nconst TRAIN_LAYERS: u32 = {}u;\nconst TRAIN_ACTIVATION: u32 = {}u;\nconst TRAIN_PARAMETERS: u32 = {}u;\n{}",
        settings.width, settings.layers, settings.activation.shader_id(), settings.parameter_count().unwrap(),
        include_str!("../../../../assets/shaders/neural_training.wgsl"))
}
