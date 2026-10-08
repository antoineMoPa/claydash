// Rasterized Poisson triangles use the same material and image-stencil
// functions as the source scene. The object record is retained after its
// component leaves the SDF BVH.
struct PoissonVertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal_owner: vec4<f32>,
}
struct PoissonVertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) point: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) owner: u32,
}
@vertex fn vs_poisson(input: PoissonVertexInput) -> PoissonVertexOutput {
    var output: PoissonVertexOutput;
    output.clip = camera.view_projection * vec4(input.position.xyz, 1.0);
    output.point = input.position.xyz;
    output.normal = input.normal_owner.xyz;
    output.owner = u32(input.normal_owner.w + 0.5);
    return output;
}
struct PoissonSurface { object: Object, surface: Surface }
fn poisson_surface(point: vec3<f32>, normal: vec3<f32>, owner: u32) -> PoissonSurface {
    let view = normalize(camera.position.xyz - point);
    let source_owner = poisson_component_owner(point, owner);
    var object = objects[source_owner];
    // Gaussian/depth representations must use the original source material,
    // never captured sample colors.
    if object.state.y == 8 || object.state.y == 9 || object.state.y == 10 || object.state.y == 12 {
        object.state.y = 0;
    }
    var surface = material_surface(point, normal, view, object);
    let decal = material_stencil_color(point, normal, object);
    if object.stencil_meta.w > 0.5 && object.stencil_meta.x > 0.5 && !metal_image_detail_enabled(object) {
        surface.color = decal.rgb;
        surface.opacity = decal.a;
    } else if decal.a > 0.0 {
        surface.color = mix(surface.color, decal.rgb, decal.a);
    }
    return PoissonSurface(object, surface);
}
fn poisson_shade(point: vec3<f32>, normal: vec3<f32>, owner: u32) -> vec4<f32> {
    let sampled = poisson_surface(point, normal, owner);
    let view = normalize(camera.position.xyz - point);
    let color = surface_light(point, normal, view, sampled.object, sampled.surface, false);
    return vec4(max(color, vec3(0.0)) * camera.position.w, clamp(sampled.surface.opacity, 0.0, 1.0));
}
@fragment fn fs_poisson(input: PoissonVertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let normal = normalize(select(-input.normal, input.normal, front));
    return poisson_shade(input.point, normal, input.owner);
}
