struct Progress { size_and_tiles: vec4<u32> }
fn refinement_phase(pixel: vec2<u32>) -> u32 {
    var phase = 0u;
    for (var bit = 0u; bit < 4u; bit++) {
        let pair = ((pixel.x >> bit) & 1u) + (((pixel.y >> bit) & 1u) << 1u);
        let digit = array<u32, 4>(0u, 2u, 3u, 1u)[pair];
        phase |= digit << (6u - 2u * bit);
    }
    return phase;
}
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
    if tile_size == 0u {
        let completed = progress.size_and_tiles.w;
        if completed == 0u { return vec4(0.0, 0.0, 0.0, 1.0); }
        if completed == 256u || refinement_phase(pixel % 16u) < completed {
            return textureLoad(refined, vec2<i32>(pixel), 0);
        }
        let grid = select(select(16u, 8u, completed >= 4u),
            select(4u, 2u, completed >= 64u), completed >= 16u);
        let block = pixel / grid * grid;
        var best = block;
        var best_distance = 0xffffffffu;
        for (var index = 0u; index < 4u; index++) {
            let candidate = block + vec2(index & 1u, index >> 1u) * (grid / 2u);
            if any(candidate >= size) || refinement_phase(candidate % 16u) >= completed { continue; }
            let offset = vec2<i32>(candidate) - vec2<i32>(pixel);
            let distance_squared = u32(dot(offset, offset));
            if distance_squared < best_distance {
                best_distance = distance_squared;
                best = candidate;
            }
        }
        return textureLoad(refined, vec2<i32>(best), 0);
    }
    let coarse_color = textureSample(preview, filtering, input.uv);
    let tile = pixel / tile_size;
    let columns = (size.x + tile_size - 1u) / tile_size;
    let complete = tile.y * columns + tile.x < progress.size_and_tiles.w;
    // Exact texel fetch avoids blending across unfinished tile boundaries.
    let fine_color = textureLoad(refined, vec2<i32>(pixel), 0);
    return select(coarse_color, fine_color, complete);
}
