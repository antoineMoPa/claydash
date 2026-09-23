// A small procedural material used to validate dispatch and packed parameters.
fn evaluate_diagnostic(point: vec3<f32>, object: Object, base: Surface, offset: u32) -> Surface {
    let settings = material_params[offset + 2u];
    let local = vec3(dot(object.inverse_rows[0], vec4(point, 1.0)),
        dot(object.inverse_rows[1], vec4(point, 1.0)),
        dot(object.inverse_rows[2], vec4(point, 1.0)));
    let checker = (i32(floor(local.x * settings.x)) + i32(floor(local.z * settings.x))) & 1;
    return Surface(mix(base.color, settings.yzw, f32(checker) * 0.65), base.normal,
        base.roughness, base.metallic, base.reflectivity, base.opacity, base.ior,
        base.coat, base.sheen, base.fiber, base.figure);
}
