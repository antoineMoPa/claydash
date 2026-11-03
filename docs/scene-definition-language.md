# Scene definition language

Claydash's scene DSL describes a whole scene as editable native objects. Store recipes in `.claydef` files and render them directly from the command line, or use them through MCP. Applying replaces the whole scene; it is one undoable edit. The recipe is saved in the `.claydash` document. Editor changes do not rewrite it: `get_scene_dsl` reports `in_sync: false`, and reapplying restores the recipe's scene.

## Standalone rendering

```sh
claydash render examples/table.claydef
claydash render examples/curves-and-sheet.claydef --output shapes.png --size 1024x768
claydash render examples/table.claydef --view top --output table-top.png
```

The command reads and compiles the file, then renders an orthographic view fitted to the scene bounds with a 10-pixel margin on the limiting dimension. Deferred rendering is the default; `--pipeline exact` selects exact ray shading. `--view` accepts `isometric` (the default three-quarter view), `top`, `left`, or `right`. Top looks down from +Y with −Z upward; left looks from −X and right from +X, with +Y upward in both side views. The default output uses the input path with a `.png` extension, at 1024 × 768 pixels. `--output` (or `-o`) and `--size WIDTHxHEIGHT` override these defaults. A local GPU is required; no desktop window, running Claydash instance, MCP connection, or agent session is needed.

The parser, evaluator, deterministic part IDs and geometry bounds live in the public `claydash_engine::scene_language` module. `compile_scene(source)` returns native parts plus seams and connections. `compile_with_assets(source, loader)` additionally resolves declared triangle assets through a caller-supplied loader. The legacy `compile(source)` returns geometry only and rejects construction metadata; `bounds(objects)` supplies world-space bounds. Both APIs are usable independently of the application and agent transport, including WebAssembly builds.

## Recipes

```text
object table({ width: number, height: number }):
    top = box(size: [width, 0.15, 1], at: [0, height, 0], color: "#a66b42");
    leg = box(size: [0.12, height, 0.12], color: "#70452e");
    place(leg, at: [-width / 2 + 0.1, height / 2, -0.4]);
    place(leg, at: [ width / 2 - 0.1, height / 2, -0.4]);
    place(leg, at: [-width / 2 + 0.1, height / 2,  0.4]);
    place(leg, at: [ width / 2 - 0.1, height / 2,  0.4]);
end
place(table(width: 2, height: 1), at: [0, 0, 0]);
```

Assignments name a primitive or a numeric expression. A part used in `place` is a reusable template; otherwise it appears once in its enclosing object. Definitions have named numeric parameters and can call other definitions. Arithmetic, parentheses, vectors, strings and `#` line comments are supported. A placement has optional `at`, `rotation` (XYZ Euler degrees), and positive `scale` vectors. Transforms apply in local coordinates; nonuniform assembly scaling that would create shear is rejected. Each emitted part gets a deterministic path and ID. Give placements a stable `name: "vehicle"` to preserve paths when adding unrelated statements; unnamed placements use their statement position. Primitive assignment paths end in `/`; connection names accept either spelling.

Primitive calls use named arguments. All support `at`, `rotation`, `scale`, `color` as `"#rrggbb"`, `opacity` (0–1), optional `mirror: ["X", "Z"]`, and `lattice: lattice(...)`. `box` accepts full dimensions `size: [x,y,z]` and `corner_radius`; `sphere` accepts `radius`; `cylinder` accepts `radius` and full `height`. `torus` accepts `major_radius` and `minor_radius`; `polygon_prism` accepts 2D `vertices` and full `depth`; `text` accepts `text`, numeric `size`, full `depth`, `tracking`, and `line_spacing`. `bezier_curve` and `sheet_panel` can also be created with native defaults. Other shape-specific parameters are not yet exposed.

A cubic Bézier curve accepts a list of control points: four points for the first segment, then three per additional segment, up to 25. `closed` defaults to false. Curves have a visible round extrusion by default; use `radius` to set its radius and `profile: "square"` for a square profile:

```text
rail = bezier_curve(points: [[0,0,0], [1,0,0], [1,1,0], [2,1,0]], radius: 0.08, closed: false);
```

A sheet uses its local XY material plane. Dimensions are full widths, and angles are signed degrees; bends are evaluated in order by the native sheet implementation:

```text
panel = sheet_panel(size: [2, 1], thickness: 0.04,
    bends: [bend(start: 0.2, radius: 0.3, angle: 45, direction: 0)]);
```

Sheet dimensions, cuts, surface controls, relief and bends use the same validation and geometry as native sheet recipes. The additional operations below remain editable in saved scenes.

## Sheet cuts, crowns and embosses

All positions below use the sheet's local XY material plane, before bending or object transforms. Cuts remove material; embosses displace the remaining sheet. They do not enable Boolean operations or create extra objects.

```text
fender = sheet_panel(size: [2,1], thickness: 0.02,
    cuts: [cut_circle(center: [-0.6,-0.4], radius: 0.25),
           cut_line(from: [0.9,-0.5], to: [0.9,0.5])],
    embosses: [emboss_circle(center: [-0.6,-0.4], radius: 0.25,
                           depth: 0.03, transition: 0.12)],
    surface: sheet_surface(heights: [
        0,0,0,0, 0,0.15,0.15,0, 0,0.15,0.15,0, 0,0,0,0]),
    path_embosses: [path_emboss(stations: [
        emboss_station(point: [0,-0.25], width: 0.08, depth: 0.01),
        emboss_station(point: [0,0.25], width: 0.1, depth: 0.015, profile: "Crease")],
        taper_start: 0.1, taper_end: 0.1)],
    bends: [bend(start: 0.6, radius: 0.4, angle: 35)],
    stage: "Formed");
```

- `cut_circle(center: [x,y], radius: r)` removes the disk. A center near or outside the blank edge makes an open notch/wheel opening. Radius must be positive.
- `cut_line(from: [x,y], to: [x,y])` retains the **left** side of the directed line. Reversing its endpoints retains the other side. Endpoints must differ; multiple cuts intersect their retained regions.
- `emboss_circle(center: [x,y], radius: r, depth: d, transition: t)` offsets an inner disk by signed depth, with a smooth radial return over positive transition width. Positive depth raises local Z; negative depth recesses it. Depth must be nonzero. Combine with a circular cut to form the surrounding lip; embossing alone makes no opening.
- `sheet_surface(heights: [...])` supplies exactly 16 cubic Bézier control heights, in four rows with X varying fastest. These are control values, not interpolated sample heights. Use this for smooth compound crown; XY remains a rectangle and the height graph has no overhangs.
- `path_emboss(stations: [...], taper_start: length, taper_end: length)` sweeps variable-width relief along 2–8 stations. Each `emboss_station` needs `point`, positive half-width `width`, signed nonzero `depth`, and optional `profile: "Rounded"` (default) or `"Crease"`. Consecutive points must differ; positive end-taper lengths must sum to no more than the path length.
- `stage: "Blank" | "Cut" | "Formed"` defaults to Formed. Blank shows the uncut flat stock. Cut applies cuts without crown/relief/bends. Formed applies the complete recipe. Switching stage retains every operation in the editable recipe.

Native limits: 32 cuts, 8 circular embosses, 4 path embosses, and 8 parallel bends (4 directional bends). Native validation rejects steep relief, insufficient clearance inside a bend, and invalid operations even when inspecting an earlier stage. Increase transition width/bend radius or reduce depth when necessary. Source seam connections still require a surviving full material edge; cutting through a bound edge is rejected atomically. Circular and straight cuts are the currently supported native cut types.

See `examples/sheet-relief.claydef` for the executable recipe. Render it directly:

```sh
claydash render examples/sheet-relief.claydef --output fender.png
```

## MCP workflow

1. Call `get_scene_dsl_guide` to retrieve this guide, then call `validate_scene_dsl` with `{ "source": "..." }` to parse, expand and validate without editing. It returns the scene revision and a list of generated paths and IDs.
2. Call `apply_scene_dsl` with `source` and that `expected_revision`. Failure leaves the scene unchanged. Existing Boolean and smooth-blend permissions are retained; the DSL does not enable either.
3. Call `capture_scene_dsl` for a PNG containing front, side, top and three-quarter orthographic views. Optionally pass `path` as a generated path or assembly path prefix and `panel_size` (96–2048, default 256). Framing uses native geometry bounds; very large scenes can exceed the camera clipping range.
4. Revise the text and repeat. `get_scene_dsl` returns the saved source and synchronization status. `save` writes both the resulting scene and recipe.

The DSL does not encode animation, material assets, Boolean operations, or every native shape parameter. Syntax and semantic errors identify the statement; constraint errors identify the named connections.

## Construction geometry and declared references

Author the complete scene in a `.claydef` file. Keep the reference in that recipe so reapplying preserves surface attachments. Asset paths resolve relative to the recipe file for standalone rendering or `agent --file`; MCP source requests accept `base_dir` (otherwise the server working directory). A GLB primitive is selected by its zero-based `mesh` index, default 0. Loading uses the ordinary GLB decoder; `get_scene_summary` reports triangle counts and world bounds without vertex arrays.

```text
reference = mesh_asset(path: "reference.glb", mesh: 0);

object body({}):
    hood = sheet_panel(size: [2,1], thickness: 0.02,
        lattice: lattice(resolution: 3, min: [-1,-0.5,-0.15], max: [1,0.5,0.15]));
    flange = sheet_panel(size: [2,1], at: [0,2,0],
        bends: [bend(start: 0, radius: 0.3, angle: 45)]);
end
place(body(), name: "vehicle");

hood_join = seam(points: [[-1,-0.5,0], [0,-0.5,0.04], [1,-0.5,0]]);
bind(object: "vehicle/hood", seam: "hood_join", target: sheet_edge("MinY"));
```

This join uses world points. To attach a point to a reference mesh, replace a vector with `snap(source: "reference", at: [x,y,z])` for the closest surface point, or `surface_point(source: "reference", triangle: 12, barycentric: [0.2,0.3,0.5])`. Snapping occurs during compilation; saved seams retain triangle attachments. Seam points are in world coordinates, including seams connected to assembly members. Seams and `bind(...)` statements belong at the scene root. `closed: true` adds the last-to-first segment.

A lattice accepts resolution 2–9 (default 3), optional explicit `min` and `max` in the object's local cage frame, and sparse controls:

```text
panel = box(size: [2,1,0.1], lattice: lattice(resolution: 3,
    controls: [control(index: [0,2,2], offset: [0,0,0.06])]));
edge = seam(points: [[1,-0.5,-0.15], [1,0,-0.15], [1,0.5,-0.15]]);
```

Omitting bounds derives a cage from native geometry and gives thin axes enough thickness to edit. Control indices are `[i,j,k]`, zero-based, distinct surface nodes; offsets are local displacements. Dense `offsets` may replace `controls` and must contain resolution³ vectors in native order (i fastest, then j, then k). Saved cages remain the ordinary native lattice modifier.

Connections use the existing seam solver; no panels are generated. `sheet_edge("MinX"|"MaxX"|"MinY"|"MaxY")` targets the real sheet material boundary after bends, while `lattice_edge(axis: "X"|"Y"|"Z", sides: [false,true])` targets an existing cage edge. The two `sides` select min/maximum along the other axes in XYZ order. For example, axis Y uses X then Z. Optional `span: [start,end]` selects seam point endpoints; closed spans may wrap. `reverse: true` reverses traversal; positive `tolerance` defaults to 0.001 world units. Multiple parts can connect to one seam. Unsupported fits fail atomically with a named diagnostic; revise placement, bend or cage instead of increasing tolerance to hide a gap.

## File-based authoring and inspection

```sh
claydash agent ValidateSceneDsl --file model.claydef
claydash agent ApplySceneDsl --file model.claydef '{"expected_revision":42}'
claydash agent CaptureSceneDsl '{"path":"vehicle","panel_size":384}' --output vehicle.png
claydash agent CaptureSceneDsl '{"path":"vehicle"}' --show-seams --output joins.png
claydash render model.claydef --part vehicle --show-seams --output joins.png
claydash render model.claydef --part vehicle --output final.png
```

Validation/application return named part and seam IDs, not geometry trees. Use those IDs with `capture_comparison` to compare candidate and reference in common framing; use `capture_scene_dsl` path prefixes for isolated parts and assemblies. Review multiple views after each meaningful part, correct contacts in assembly context, and inspect the final regenerated images. Red seam guides require `show_seams: true` or `--show-seams`; final captures omit them by default. `save` stores an editable native scene, source recipe, asset base directory and named part map. Opening the saved document and capturing its named parts does not require rereading the original GLB.
