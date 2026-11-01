# Scene definition language

Claydash's scene DSL describes a whole scene as editable native objects. Store recipes in `.claydef` files and render them directly from the command line, or use them through MCP. Applying replaces the whole scene; it is one undoable edit. The recipe is saved in the `.claydash` document. Editor changes do not rewrite it: `get_scene_dsl` reports `in_sync: false`, and reapplying restores the recipe's scene.

## Standalone rendering

```sh
claydash render examples/table.claydef
claydash render examples/curves-and-sheet.claydef --output shapes.png --size 1024x768
claydash render examples/table.claydef --view top --output table-top.png
```

The command reads and compiles the file, then renders an automatically framed orthographic view. `--view` accepts `isometric` (the default three-quarter view), `top`, `left`, or `right`. Top looks down from +Y with −Z upward; left looks from −X and right from +X, with +Y upward in both side views. The default output uses the input path with a `.png` extension, at 1024 × 768 pixels. `--output` (or `-o`) and `--size WIDTHxHEIGHT` override these defaults. A local GPU is required; no desktop window, running Claydash instance, MCP connection, or agent session is needed.

The parser, evaluator, deterministic part IDs and geometry bounds live in the public `claydash_engine::scene_language` module. `compile(source)` returns generated part paths and native `SdfObject` values; `bounds(objects)` supplies world-space bounds. Both APIs are usable independently of the application and agent transport, including WebAssembly builds.

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

Assignments name a primitive or a numeric expression. A part used in `place` is a reusable template; otherwise it appears once in its enclosing object. Definitions have named numeric parameters and can call other definitions. Arithmetic, parentheses, vectors, strings and `#` line comments are supported. A placement has optional `at`, `rotation` (XYZ Euler degrees), and positive `scale` vectors. Transforms apply in local coordinates; nonuniform assembly scaling that would create shear is rejected. Each emitted part gets a deterministic path and ID; changing a placement's statement position can change its path and ID.

Primitive calls use named arguments. All support `at`, `rotation`, `scale`, and `color` as `"#rrggbb"`. `box` accepts full dimensions `size: [x,y,z]` and `corner_radius`; `sphere` accepts `radius`; `cylinder` accepts `radius` and full `height`. `torus` accepts `major_radius` and `minor_radius`; `polygon_prism` accepts 2D `vertices` and full `depth`; `text` accepts `text`, numeric `size`, full `depth`, `tracking`, and `line_spacing`. `bezier_curve` and `sheet_panel` can also be created with native defaults. Other shape-specific parameters are not yet exposed.

A cubic Bézier curve accepts a list of control points: four points for the first segment, then three per additional segment, up to 25. `closed` defaults to false. Curves have a visible round extrusion by default; use `radius` to set its radius and `profile: "square"` for a square profile:

```text
rail = bezier_curve(points: [[0,0,0], [1,0,0], [1,1,0], [2,1,0]], radius: 0.08, closed: false);
```

A sheet uses its local XY material plane. Dimensions are full widths, and angles are signed degrees; bends are evaluated in order by the native sheet implementation:

```text
panel = sheet_panel(size: [2, 1], thickness: 0.04,
    bends: [bend(start: 0.2, radius: 0.3, angle: 45, direction: 0)]);
```

Sheet dimensions and bends are checked using the same validation as native sheet recipes. More operations such as sheet cuts and embosses are not yet exposed.

## MCP workflow

1. Call `get_scene_dsl_guide` to retrieve this guide, then call `validate_scene_dsl` with `{ "source": "..." }` to parse, expand and validate without editing. It returns the scene revision and a list of generated paths and IDs.
2. Call `apply_scene_dsl` with `source` and that `expected_revision`. Failure leaves the scene unchanged. Existing Boolean and smooth-blend permissions are retained; the DSL does not enable either.
3. Call `capture_scene_dsl` for a PNG containing front, side, top and three-quarter orthographic views. Optionally pass `path` as a generated path or assembly path prefix and `panel_size` (96–2048, default 256). Framing uses native geometry bounds; very large scenes can exceed the camera clipping range.
4. Revise the text and repeat. `get_scene_dsl` returns the saved source and synchronization status. `save` writes both the resulting scene and recipe.

The DSL currently focuses on primitive assemblies. It does not encode animation, imports, materials beyond color, Boolean operations, or every parameter available through the existing MCP actions. Syntax and semantic errors include a line and column.
