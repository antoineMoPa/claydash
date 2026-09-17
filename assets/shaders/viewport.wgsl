struct Progress { size_and_tiles: vec4<u32> }
@group(0) @binding(0) var preview: texture_2d<f32>;
@group(0) @binding(1) var refined: texture_2d<f32>;
@group(0) @binding(2) var filtering: sampler;
@group(0) @binding(3) var<uniform> progress: Progress;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(3.0, 1.0), vec2(-1.0, 1.0));
    var out: VertexOutput;
    out.position = vec4(positions[index], 0.0, 1.0);
    out.uv = positions[index] * vec2(0.5, -0.5) + vec2(0.5);
    return out;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = progress.size_and_tiles.xy;
    let pixel = min(vec2<u32>(input.uv * vec2<f32>(size)), size - vec2(1u));
    let tile_size = progress.size_and_tiles.z;
    let tile = pixel / tile_size;
    let columns = (size.x + tile_size - 1u) / tile_size;
    let complete = tile.y * columns + tile.x < progress.size_and_tiles.w;
    let coarse_color = textureSample(preview, filtering, input.uv);
    // Exact texel fetch avoids blending across unfinished tile boundaries.
    let fine_color = textureLoad(refined, vec2<i32>(pixel), 0);
    return select(coarse_color, fine_color, complete);
}
