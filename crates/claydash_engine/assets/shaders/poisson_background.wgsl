@group(0) @binding(0) var<uniform> camera: Camera;
struct VertexOutput { @builtin(position) position:vec4<f32>, @location(0) clip:vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index:u32)->VertexOutput {
    let p=array<vec2<f32>,3>(vec2(-1.0,-3.0),vec2(3.0,1.0),vec2(-1.0,1.0));
    return VertexOutput(vec4(p[index],0.0,1.0),p[index]);
}
@fragment fn fs_main(input:VertexOutput)->@location(0) vec4<f32> {
    if camera.world_mode.x==3u { return vec4(0.0); }
    let far=camera.inverse_view_projection*vec4(input.clip,1.0,1.0);
    let near=camera.inverse_view_projection*vec4(input.clip,0.0,1.0);
    let ray=select(normalize(far.xyz/far.w-camera.position.xyz),normalize(far.xyz/far.w-near.xyz/near.w),camera.count.w!=0u);
    return vec4(background(ray),1.0);
}
