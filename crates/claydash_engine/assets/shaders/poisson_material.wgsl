// Rasterized Poisson triangles use the same material and image-stencil
// functions as the source scene. The object record is retained after its
// component leaves the SDF BVH.
struct PoissonVertexInput {
    @location(0) position: vec4<f32>,
    @location(1) normal_owner: vec4<f32>,
    @location(2) captured_color: vec4<f32>,
}
@group(0) @binding(14) var mesh_color_atlas: texture_2d_array<f32>;
@group(0) @binding(15) var mesh_color_sampler: sampler;
struct PoissonVertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) point: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) owner: u32,
    @location(3) captured_color: vec4<f32>,
}
@vertex fn vs_poisson(input: PoissonVertexInput) -> PoissonVertexOutput {
    var output: PoissonVertexOutput;
    output.clip = camera.view_projection * vec4(input.position.xyz, 1.0);
    output.point = input.position.xyz;
    output.normal = input.normal_owner.xyz;
    output.owner = u32(input.normal_owner.w + 0.5);
    output.captured_color = input.captured_color;
    return output;
}
struct PoissonSurface { object: Object, surface: Surface }
fn poisson_surface(point: vec3<f32>, normal: vec3<f32>, owner: u32, imported: bool) -> PoissonSurface {
    let view = normalize(camera.position.xyz - point);
    // Imported triangles already carry their exact owner. Their placeholder
    // primitive must never participate in CSG material lookup.
    var source_owner = owner;
    if !imported { source_owner = poisson_component_owner(point, owner); }
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
var<private> bloom_emission: vec3<f32>;
fn poisson_shade(point: vec3<f32>, normal: vec3<f32>, owner: u32, imported: bool) -> vec4<f32> {
    let sampled = poisson_surface(point, normal, owner, imported);
    let view = normalize(camera.position.xyz - point);
    let color = surface_light(point, normal, view, sampled.object, sampled.surface, false);
    let emitted = material_emission(point, view, sampled.object);
    bloom_emission = emitted * clamp(sampled.surface.opacity, 0.0, 1.0) * camera.position.w;
    return vec4((max(color, vec3(0.0)) + emitted) * camera.position.w, clamp(sampled.surface.opacity, 0.0, 1.0));
}
@fragment fn fs_poisson(input: PoissonVertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let normal = normalize(select(-input.normal, input.normal, front));
    if input.captured_color.a > 0.0 {
        // Voxel faces retain their captured albedo and geometric cube normal.
        // No source SDF traversal or surface-normal query is needed here.
        let light = select(normalize(vec3(2.0, 3.0, 2.0)),
            normalize(camera.sun_direction.xyz), camera.world_mode.x == 1u);
        let diffuse = max(dot(normal, light), 0.0);
        // Authored image colors need enough studio fill to stay legible when
        // a textured mesh faces away from the fixed key light. The sparse
        // scan-color and voxel paths keep their existing lighting.
        let base_ambient = diffuse_ambient_fill(normal, 0.13);
        let ambient = select(base_ambient, max(base_ambient, 0.52),
            input.captured_color.a > 1.5);
        let daylight = smoothstep(-0.18, 0.16, camera.sun_direction.y);
        var direct = select(0.75, camera.sun_direction.w * daylight, camera.world_mode.x == 1u);
        if camera.world_mode.x == 4u { direct = 0.12; }
        let emitted = material_emission(input.point, normalize(camera.position.xyz - input.point), objects[input.owner]);
        bloom_emission = emitted * camera.position.w;
        var albedo = input.captured_color.rgb;
        if input.captured_color.a > 1.5 {
            albedo = textureSampleLevel(mesh_color_atlas, mesh_color_sampler,
                input.captured_color.xy, i32(input.captured_color.z + 0.5), 0.0).rgb;
        }
        return vec4(emitted * camera.position.w + max(albedo, vec3(0.0))
            * (ambient + direct * diffuse) * camera.position.w, 1.0);
    }
    return poisson_shade(input.point, normal, input.owner, input.captured_color.a < 0.0);
}
