struct Progress { size_and_tiles: vec4<u32>, refinement_rect: vec4<u32> }
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
// Interpolate only completed samples. Missing corners retain the complete
// navigation preview; stale texels from an earlier camera are never read.
fn completed_sample(local: vec2<u32>, origin: vec2<u32>, columns: u32,
    phase_pixels: u32, coverage: u32, samples: u32, fallback: vec4<f32>) -> vec4<f32> {
    let cell = (local.y / 16u) * columns + local.x / 16u;
    let phase = refinement_phase(local % 16u);
    if phase * phase_pixels + cell < samples || (phase == 0u && cell < coverage) {
        return textureLoad(refined, vec2<i32>(origin + local), 0);
    }
    return fallback;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = progress.size_and_tiles.xy;
    let pixel = min(vec2<u32>(input.uv * vec2<f32>(size)), size - vec2(1u));
    let tile_size = progress.size_and_tiles.z;
    if tile_size == 0u {
        let rect = progress.refinement_rect;
        let origin = rect.xy;
        let extent = select(size, rect.zw, all(rect.zw > vec2(0u)));
        if any(pixel < origin) || any(pixel >= origin + extent) {
            return textureLoad(refined, vec2<i32>(pixel), 0);
        }
        let local = pixel - origin;
        let columns = (extent.x + 15u) / 16u;
        let phase_pixels = columns * ((extent.y + 15u) / 16u);
        let coverage = min(progress.size_and_tiles.w, phase_pixels);
        let samples = min(progress.size_and_tiles.w - coverage, 256u * phase_pixels);
        let sample_index = (local.y / 16u) * columns + local.x / 16u;
        let phase = refinement_phase(local % 16u);
        if phase * phase_pixels + sample_index < samples {
            return textureLoad(refined, vec2<i32>(pixel), 0);
        }
        if phase == 0u && sample_index < coverage {
            return textureLoad(refined, vec2<i32>(pixel), 0);
        }
        let coarse = textureSampleLevel(preview, filtering, input.uv, 0.0);
        if coverage == 0u { return coarse; }
        // Completed phase groups form progressively finer regular grids.
        // Reconstruct a smooth background while exact scattered native pixels
        // above stay untouched. This reuses work already done by the renderer.
        var spacing = 16u;
        if samples >= 64u * phase_pixels { spacing = 2u; }
        else if samples >= 16u * phase_pixels { spacing = 4u; }
        else if samples >= 4u * phase_pixels { spacing = 8u; }
        let last = ((extent - vec2(1u)) / spacing) * spacing;
        let lo = min((local / spacing) * spacing, last);
        let hi = min(lo + vec2(spacing), last);
        let blend = clamp(vec2<f32>(local - lo) / f32(spacing), vec2(0.0), vec2(1.0));
        let a = completed_sample(lo, origin, columns, phase_pixels, coverage, samples, coarse);
        let b = completed_sample(vec2(hi.x, lo.y), origin, columns, phase_pixels, coverage, samples, coarse);
        let c = completed_sample(vec2(lo.x, hi.y), origin, columns, phase_pixels, coverage, samples, coarse);
        let d = completed_sample(hi, origin, columns, phase_pixels, coverage, samples, coarse);
        let top = mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), blend.x);
        let bottom = mix(vec4(c.rgb * c.a, c.a), vec4(d.rgb * d.a, d.a), blend.x);
        let color = mix(top, bottom, blend.y);
        return vec4(color.rgb / max(color.a, 0.00001), color.a);
    }
    // Earlier per-pixel returns make implicit derivatives non-uniform. The
    // preview has one mip level, so explicitly sampling level zero is equivalent.
    let coarse_color = textureSampleLevel(preview, filtering, input.uv, 0.0);
    let tile = pixel / tile_size;
    let columns = (size.x + tile_size - 1u) / tile_size;
    let complete = tile.y * columns + tile.x < progress.size_and_tiles.w;
    // Exact texel fetch avoids blending across unfinished tile boundaries.
    let fine_color = textureLoad(refined, vec2<i32>(pixel), 0);
    return select(coarse_color, fine_color, complete);
}
