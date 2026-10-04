// position.w is display exposure: 1.0 in the viewport, brighter in material previews.
// count: object count, BVH node count, viewport height, orthographic flag.
// world_mode.w: viewport width for sparse native-pixel refinement.
struct Camera {
    inverse_view_projection: mat4x4<f32>, position: vec4<f32>, count: vec4<u32>,
    world_mode: vec4<u32>, world_color: vec4<f32>, sun_direction: vec4<f32>, sky_params: vec4<f32>,
    night_params: vec4<f32>, night_color: vec4<f32>,
    lighting_params: vec4<f32>,
    view_projection: mat4x4<f32>,
}
struct Object {
    state: vec4<i32>,
    color: vec4<f32>,
    inverse_rows: array<vec4<f32>, 3>,
    group_inverse_rows: array<vec4<f32>, 3>,
    params: vec4<f32>,
    repeat_spacing: vec4<f32>,
    repeat_count: vec4<i32>,
    component: vec4<u32>,
    scale: vec4<f32>,
    modifier: vec4<u32>,
    mirror_axes: vec4<u32>,
    stencil_placement: vec4<f32>,
    stencil_meta: vec4<f32>,
    distance_bound: vec4<f32>,
    // xy: operand BVH range; z: captured subtree root + 1; w: refinement distance, f32 bits.
    operand_tree: vec4<u32>,
    box_depth_meta: vec4<u32>,
    box_depth_min: vec4<f32>,
    box_depth_max: vec4<f32>,
}
