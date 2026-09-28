# Post-processing WGSL

Add effects in the World panel or through the MCP `CreatePostProcessPass` action. Passes run in list order on the completed scene, after Exact SDF, Deferred, or hybrid splat rendering and before editor overlays. Viewport captures and WebP/MP4 exports include the effects.

Each `wgsl` value is the **body** of this function:

```wgsl
fn effect(uv: vec2<f32>, color: vec4<f32>, resolution: vec2<f32>, time: f32) -> vec4<f32> {
    // Your code here.
}
```

- `uv` runs from `(0, 0)` at the top left to `(1, 1)` at the bottom right of the scene viewport.
- `color` is the current pixel from the preceding pass in linear RGB with premultiplied alpha (RGB is already multiplied by alpha).
- `resolution` is the viewport size in pixels.
- `time` is elapsed wall-clock seconds since the renderer started. An effect using it animates during normal playback and export; it is not tied to timeline time.
- `sample_scene(uv)` reads the preceding pass at viewport-local UV. It clamps to pixel centers inside the viewport, so effects cannot pull in editor margins.

Return a `vec4<f32>` with the desired premultiplied RGB and alpha. Preserve `color.a` for effects that only change color. The source texture and sampler are also exposed as `scene_color` and `scene_sampler`, and `frame` contains the full-frame size and viewport origin when direct framebuffer sampling is needed. The helper is preferred for viewport-local effects.

Example: invert the scene's RGB channels while preserving transparency:

```wgsl
return vec4<f32>(vec3<f32>(color.a) - color.rgb, color.a);
```

Example: a horizontal blur, in viewport pixels:

```wgsl
let step = vec2<f32>(1.0 / resolution.x, 0.0);
let left = sample_scene(uv - step);
let right = sample_scene(uv + step);
return (left + color + right) / 3.0;
```

WGSL is validated before **Apply**. An invalid edit leaves the prior pass active and shows the error in the World panel. MCP `Apply` validates the complete transaction; any invalid pass rejects all actions in that call. Up to 16 passes and 8192 bytes per body are supported.
