use super::*;

// Preview modules share the scene's parameter ABI, evaluators, light and environment.
// Scene traversal, CSG, captured geometry, modifiers and deferred entrypoints are absent.
pub(super) fn preview_shader_source(
    features: BuiltinMaterialFeatures,
    custom_sources: &[(uuid::Uuid, String)],
) -> String {
    let mut common = include_str!("../../../assets/shaders/material_common.wgsl").to_owned();
    let mut modules = Vec::new();
    for (enabled, call, module) in [
        (
            features.wood,
            "evaluate_wood(point, normal, material_object, base, header.offset)",
            include_str!("../../../assets/shaders/material_wood.wgsl"),
        ),
        (
            features.brick,
            "evaluate_brick(point, normal, view, material_object, base, header.offset)",
            include_str!("../../../assets/shaders/material_brick.wgsl"),
        ),
        (
            features.fabric,
            "evaluate_fabric(point, normal, material_object, base, header.offset)",
            include_str!("../../../assets/shaders/material_fabric.wgsl"),
        ),
        (
            features.metal,
            "evaluate_metal(point, normal, material_object, base, header.offset)",
            include_str!("../../../assets/shaders/material_metal.wgsl"),
        ),
        (
            features.diagnostic,
            "evaluate_diagnostic(point, material_object, base, header.offset)",
            include_str!("../../../assets/shaders/material_diagnostic.wgsl"),
        ),
    ] {
        if enabled {
            modules.push(module);
        } else {
            common = common.replace(call, "base");
        }
    }
    if !features.fabric {
        common = common.replace("fabric_extra_light(surface, light, view)", "vec3(0.0)");
    }
    let mut cases = String::new();
    let mut functions = String::new();
    for (index, (_, body)) in custom_sources.iter().enumerate() {
        let index = index + 1;
        cases.push_str(&format!(
            "case {index}u: {{ return custom_material_{index}(point, normal, view, base); }}\n"
        ));
        functions.push_str(&format!("fn custom_material_{index}(point: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, base: Surface) -> Surface {{\n{body}\n}}\n"));
    }
    common = common.replace("// CUSTOM_MATERIAL_CASES", &cases);
    let mut sphere = include_str!("../../../assets/shaders/material_preview.wgsl").to_owned();
    if features.fabric {
        sphere = sphere
            .replace(
                "// PREVIEW_FABRIC_DISTANCE",
                r#"
    if fabric_geometry_visible(object) {
        let header = material_headers[object.component.w];
        let depth = material_params[header.offset + FABRIC_PITCH].z;
        if distance < max(depth * 2.0, 0.04) {
            distance += fabric_geometry_cut(point, object, header.offset);
        }
    }"#,
            )
            .replace(
                "// PREVIEW_FABRIC_TRACE",
                r#"
    if fabric_geometry_visible(objects[0]) {
        var travel = select(entry, 0.0, inside);
        for (var step = 0u; step < 128u; step++) {
            let distance = preview_distance(origin + direction * travel);
            if select(distance < epsilon, abs(distance) <= epsilon, inside) {
                return vec2(travel, 0.0);
            }
            travel += select(distance, max(abs(distance) * 0.8, 0.0008), inside);
            if travel > exit { break; }
        }
        return vec2(100.0, -1.0);
    }"#,
            );
    }
    if features.metal {
        // The shared metal environment integration is unchanged. Its scene correction
        // uses the same optical formula with this module's sphere-only hit query.
        let metal = include_str!("../../../assets/shaders/material_metal_light.wgsl");
        let split = metal.find("// One traced scene reflection").unwrap();
        modules.push(&metal[..split]);
        let correction = metal[split..]
            .replace("metal_scene_reflection", "preview_metal_reflection")
            .replace(
                "trace_objects(origin, ray, 0.003, empty_splat_exclusions(), HYBRID_SPLATS)",
                "preview_trace(origin, ray, false, 0.003)",
            )
            .replace(
                "scene_normal(reflected_point, u32(hit.y))",
                "preview_normal(reflected_point)",
            )
            .replace(
                "gbuffer_surface(reflected_point, normal, -ray, object)",
                "material_surface(reflected_point, normal, -ray, object)",
            );
        sphere = sphere.replace("// PREVIEW_METAL_REFLECTION", &correction);
    } else {
        modules.push(r#"
const METAL_PAINT: u32 = 4u;
fn metal_light(point: vec3<f32>, view: vec3<f32>, surface: Surface, ao: f32) -> vec3<f32> { return vec3(0.0); }
fn preview_metal_reflection(point: vec3<f32>, view: vec3<f32>, surface: Surface, ao: f32) -> vec3<f32> { return vec3(0.0); }
"#);
    }
    [
        include_str!("../../../assets/shaders/scene_abi.wgsl"),
        &sphere,
        include_str!("../../../assets/shaders/environment.wgsl"),
        &common,
        &modules.join("\n"),
        include_str!("../../../assets/shaders/ambient_occlusion.wgsl"),
        &functions,
        include_str!("../../../assets/shaders/material_preview_transport.wgsl"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_preview_custom_assets_do_not_add_unrelated_families() {
        let features =
            BuiltinMaterialFeatures::for_materials(&[Material::preset(MaterialKind::Brick)]);
        let source =
            preview_shader_source(features, &[(uuid::Uuid::nil(), "return base;".to_owned())]);
        assert!(source.contains("fn evaluate_brick("));
        for family in ["wood", "fabric", "metal", "diagnostic"] {
            assert!(!source.contains(&format!("fn evaluate_{family}(")));
        }
        assert!(source.contains("fn custom_material_1("));
        assert!(source.len() < material_gpu::shader_source().len() / 2);
    }

    #[test]
    fn material_preview_shader_families_are_valid() {
        for kind in [
            MaterialKind::Solid,
            MaterialKind::Transparent,
            MaterialKind::Metallic,
            MaterialKind::Wood,
            MaterialKind::Brick,
            MaterialKind::Fabric,
            MaterialKind::Metal,
            MaterialKind::Diagnostic,
            MaterialKind::Custom,
        ] {
            let features = BuiltinMaterialFeatures::for_materials(&[Material::preset(kind)]);
            let custom = if kind == MaterialKind::Custom {
                vec![(uuid::Uuid::nil(), "return base;".to_owned())]
            } else {
                vec![]
            };
            let source = preview_shader_source(features, &custom);
            assert!(!source.contains("fn trace_objects("));
            assert!(!source.contains("fn component_distance("));
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("{kind:?}: {}", error.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{kind:?}: {error:?}"));
        }
    }
}
