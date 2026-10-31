# Local agents and Claydash

Claydash exposes scene editing and rendered captures through a CLI and an MCP server on macOS
and Linux. Connect to the desktop app, run a window-free server, or let an MCP client own a
private headless session. The browser build does not start this local bridge.

## Start and connect

Use `claydash --help` (or `cargo run --release -- --help`) for command usage. Unknown
commands and options exit with an error instead of opening the desktop app.

Build the native binary and start the window:

```sh
cargo build --release
./target/release/claydash
```

For a server with no app window or desktop event loop:

```sh
./target/release/claydash serve
# Optional initial document and capture dimensions (default: 640x480):
./target/release/claydash serve --scene duck.claydash --size 1024x768
```

`serve` stays in the foreground until stopped (Ctrl-C). It uses the existing offscreen wgpu
renderer, so captures and background optimization jobs still need a supported GPU adapter,
but no display server is needed. `--headless` and `--agent-headless` are aliases for `serve`;
They do not create a hidden window. Without `--scene`, headless sessions start with the same
empty document and studio lighting as File → New.
Edits stay in memory until explicitly saved with the `save` tool or `agent Save`.

Use the **absolute path** to the binary in your agent configuration. The desktop app and
`serve` create `~/.claydash-agent/agent.sock` with owner-only permissions. One instance owns
a socket at a time. `claydash mcp` and `claydash agent` connect to that existing instance.
To use another socket, set `CLAYDASH_AGENT_SOCKET` to the same absolute socket path in the
server and clients. Use a private directory; custom parent-directory permissions are preserved.
A stale socket left after termination is recovered on the next start.
Unix socket paths have a platform length limit. If the path is too long, Claydash reports its
length and asks for a shorter `CLAYDASH_AGENT_SOCKET` path.

For a private session started and stopped by the MCP client, configure the command with:

```sh
/absolute/path/to/claydash mcp --headless
```

This uses the same MCP tools directly, without binding or connecting to the shared socket.
It accepts `--scene` and `--size` too, and exits when stdin closes. Each process owns an
independent scene. Stdout is reserved for MCP JSON-RPC; diagnostics go to stderr. In the
configuration examples below, use `["mcp", "--headless"]` instead of `["mcp"]` for this mode.

### Codex

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.claydash]
command = "/absolute/path/to/claydash"
args = ["mcp"]
```

### Claude Code

```sh
claude mcp add --transport stdio claydash -- /absolute/path/to/claydash mcp
```

### OpenCode

```sh
opencode mcp add claydash -- /absolute/path/to/claydash mcp
```

The configuration follows each client's current local MCP documentation:
[Codex](https://learn.chatgpt.com/docs/extend/mcp?surface=cli),
[Claude Code](https://code.claude.com/docs/en/mcp), and
[OpenCode](https://opencode.ai/v2/docs/mcp-servers).

## Workflow skills over MCP

Claydash embeds two concise skills with the server binary:

- `claydash-basics`: choosing primitives, materials, and render modes.
- `claydash-modeling`: building one subpart at a time, checking four to six views,
  adding appearance, and confirming alignment with adjacent parts before continuing.

Clients supporting the [MCP Skills extension](https://modelcontextprotocol.io/extensions/skills/overview)
discover `io.modelcontextprotocol/skills` through `server/discover`, then use `skills/list`
or `skills/get` with `{"uri":"skill://claydash-modeling/SKILL.md"}`. Read instructions
with `resources/read` using the same URI. Entries include SHA-256 digests and byte sizes
of the exact embedded Markdown; discovery returns metadata without loading instructions.
The catalog is fixed for each binary build, so editing `skills/*/SKILL.md` requires a rebuild.

The adapter supports revision `2026-07-28` per-request metadata alongside the existing
`2025-11-25` initialization handshake. Clients without Skills support can discover and
read the same files through standard `resources/list` and `resources/read`; their host
decides whether and how to load them as skills. Directory reading is not advertised.
Both socket-backed `mcp` and private `mcp --headless` sessions serve the same catalog.

## Editable mesh shading

`CreateMesh` accepts optional `shading`: `Auto` (default, 60° creases), `Flat`, or
`Smooth`. `SetMeshShading` changes it later with `{ "id": "…", "shading": "Smooth" }`.
Smooth shading averages face normals at shared vertex indices, so adjacent faces must use
the same vertex indices to blend. The Object settings panel has the same Shading control.
`GetMesh` returns the shading mode and one normal per triangle corner; edits regenerate
normals, and saved scenes and GLB export retain the result.

## GLB export

The `export_glb` MCP tool starts a background export without a file dialog, in desktop,
`serve`, and `mcp --headless` sessions. For example:

```json
{"path":"/absolute/path/model.glb","geometry":"voxels","voxel_resolution":32}
```

`geometry` accepts `current_representation` (default), `direct`, `smooth`, or `voxels`:

- `current_representation` keeps voxel objects at their saved resolution; other objects become
  their saved direct representation or smooth Poisson meshes.
- `direct` tessellates native geometry. Optional `direct_resolution` (8–256, default 64)
  controls field fallbacks; optional positive `direct_error` sets the adaptive surface-error
  target in world-space scene units (default `0.032 / direct_resolution`). For meter-based
  scenes, `0.0005` targets 0.5 mm. Rounded primitives, panels and lattice refinement adapt
  to curvature and world scale; flat faces stay coarse. Solid materials use shared indexed
  vertices and native metallic/roughness/emissive PBR, while procedural, image and blended
  materials retain texture baking. The export dialog exposes the same tolerance in mm.
  Panel triangulation uses constrained Delaunay connections and interior support points
  to avoid long triangle fans around arcs while preserving cut boundaries and holes.
- `smooth` exports Poisson meshes with baked material textures.
- `voxels` uses the existing six-face capture, flat cube faces, and sampled vertex colors.
  Only this mode accepts `voxel_resolution`, from 8 to 128 (default 32).

An optional nonempty `object_ids` array selects objects or Boolean subtrees. When omitted,
export uses the current selection, or the whole scene if nothing is selected. The export uses
a snapshot of scene geometry, materials, and view taken when the job starts. It does not change
selection or viewport representation. Only one export runs at a time.

The output path is absolute **on the machine running Claydash**, and its parent directory must
exist. Existing files are preserved unless `overwrite: true` is supplied. The completed file is
published atomically.

The response includes `id` and `status`. Call `get_glb_export_status` with `{"id":"<job UUID>"}`
until status is `completed`, `failed` (with `error`), or `cancelled`. While `running`, the response
includes `stage`, `object_name`, `index`, `total`, and optional per-stage `percent`. Call
`cancel_glb_export` with the same id to cancel. If publication already finished, cancellation
returns `completed`; a job reported as `cancelled` cannot later publish a file. Opening or
replacing the scene also cancels pending exports. The most recent 32 terminal job results remain
queryable until the Claydash process exits; keep a private MCP session open until its export finishes.

The CLI exposes the same operations:

```sh
claydash agent ExportGlb '{"path":"/absolute/path/model.glb","geometry":"voxels","voxel_resolution":32}'
claydash agent GetGlbExportStatus '{"id":"<job UUID>"}'
claydash agent CancelGlbExport '{"id":"<job UUID>"}'
```

## Agent workflow

1. Call `get_scene_summary` for the revision, object IDs, names, transforms, selection, camera,
   and imported Gaussian counts and bounds. It omits embedded splat bytes. Call `get_state` when
   you need the complete typed scene and `.claydash` document for a round trip.
2. Call `get_action_schema` with `{"action":"CreateObject","primitive":"SheetPanel"}` to
   inspect a single tagged action, panel params, and bend fields. `apply` actions use a `type`
   field, such as `{"type":"CreateObject","kind":"SheetPanel"}`. `get_schema` remains available
   for all primitive examples and action names.
3. Call `apply` with one or more typed actions. Pass `expected_revision` from `get_scene_summary` so an
   intervening scene edit causes a clear error. Actions in one call form one undo step. A failed
   call leaves the live scene untouched.
   Model with independent parts, native sheet cuts/bends/crowns and primitive corner radii.
   Do not use Boolean unions just to organize parts. Shared vector variables with `Position`
   bindings and per-part offsets let an assembly translate together; shared rigid frame bindings
   let independent parts translate and rotate together without a Boolean tree.
   Adding Boolean geometry is disabled by default, including hard unions, subtraction and
   intersection. Only call `enable_boolean_operations` (`EnableBooleanOperations` in the CLI)
   when the user explicitly requests Boolean geometry. New or enlarged positive Boolean
   softness separately requires `enable_slow_smooth_blends`; enabling either permission does
   not grant the other. Both permissions are saved with the scene and undoable; New resets
   them, and `ReplaceScene` cannot grant them. Existing imported operations remain editable
   and can be detached or removed without opt-in. `PutObject`, `ReplaceScene` and command
   execution enforce the same policy as `SetBoolean`. Desktop modeling is unchanged.
4. Call `set_view` if the camera needs to move, then `capture_viewport` to get a PNG image.
   The capture renders offscreen even when the native window is unfocused or on another macOS
   desktop. Pass `object_ids` to show specific Boolean groups and their surface inlays.
   To inspect individual primitives, pass `object_ids` with `isolation: "parts"`; this draws
   only the named parts at their world positions, including hidden subtractive operands as
   standalone solids. `isolation: "groups"` is the default. A surface inlay requires its host,
   so capture it in groups mode. Parts whose parent transforms create shear cannot be detached
   without changing their shape; the capture returns an error for those parts. Captures use
   `mode: "simple_shading"` by default; use
   `mode: "full_material"` for close material inspection or `mode: "outline"` to see primitive
   wire guides for every captured object, including hidden Boolean operands. Captures omit editor
   controls and gizmos, and include a view title, horizontal and vertical world-axis directions,
   and a scale bar measured at the camera target depth. Outline PNGs contain the guides on a
   dark background. The scene, selection, and
   live viewport mode stay unchanged. Legacy `refine: true` selects full material rendering and
   `refine: false` selects simple shading; an explicit `mode` takes precedence over `refine`.
   Repeat the edit and capture steps until the design looks right.
5. Call `capture_orthographic` for one PNG with X (side), Y (top), and Z (front) orthographic views
   aimed at the origin. Each panel is 256 px square by default. `panel_size` accepts 96–512 px
   and must fit inside the viewport; `distance` accepts 0.1–1000. It accepts the same `object_ids`,
   `isolation`, and `mode`/`refine` options.
6. Call `save` with a `.claydash` path, or use the current document path.

For a mesh reference, call `import_glb` with an absolute local `path`. It creates one
undoable, selected object per triangle primitive and returns object IDs, world-space bounds,
vertex and triangle counts, and the scene revision. The default (or first) scene is used;
node transforms are baked into positions, including reflected winding. Geometry must use the
embedded GLB buffer. Non-triangle primitives are skipped. Base color factors are retained;
textures, UVs, other material properties, skinning and animation are not imported. The mesh
is embedded in saved projects. Import does not lock or mark reference objects: keep their IDs
separate and pass only candidate IDs to `export_glb` to exclude them.

For a basic comparison, capture reference and candidate IDs separately with the same camera,
mode and viewport settings. `capture_orthographic` gives consistently framed origin-centered
panels; `capture_viewport` uses the agent camera. These are separate images, not transparent
overlays, and reference objects remain editable in the scene.

For an imported Gaussian object, `get_gaussian_cloud` with `{"id":"<object UUID>","space":"world"}`
returns exact center-position bounds, per-axis 2nd/50th/98th percentiles, and up to 256
deterministic position/color/opacity samples (32 by default). Use `"space":"object"` for local
coordinates. Bounds and percentiles describe Gaussian centers, not the full ellipsoid support.
The tool does not return the embedded asset bytes.

The viewport's top-right aperture button toggles full material rendering; the scan button or `Z`
toggles outlines. These live editor modes are mutually exclusive. Export renders always use
full resolution.

For an outline sheet of a Boolean group, call `capture_orthographic` with:

```json
{"mode":"outline","object_ids":["<group-root-id>"],"panel_size":256}
```

For example, the `apply` arguments below create two shapes and make the sphere subtract from the
box after a subsequent call supplies their generated IDs:

```json
{
  "expected_revision": 42,
  "actions": [
    {"type": "CreateObject", "kind": "Box", "name": "Body", "position": [0, 0, 0]},
    {"type": "CreateObject", "kind": "Sphere", "name": "Cutout", "position": [0.2, 0.3, 0]}
  ]
}
```

`apply` returns `created_ids` in action order. A follow-up edit can use
`{"type":"SetBoolean","id":"<cutout-id>","parent":"<body-id>","operation":"Subtract"}`.

`PutObject` replaces a complete object by UUID. It gives agents access to every serialized object
field, including materials, repetition, mirrors, lattice, path extrusion, and surface inlays.
The `lattice` field is either `null` or an object with `resolution` (an integer from 2 to 9),
`min` and `max` (three-number vectors in the lattice owner's local space), and `offsets` (an
array of `resolution³` three-number displacement vectors). Index a control as
`x + resolution * (y + resolution * z)`, with X varying fastest. To create a three-point cage,
start with 27 `[0,0,0]` offsets, then edit the entries for the surface controls. For example,
the upper far corner is entry 26. Include `shape_keys: []`, `current_shape_key: 0`, and
`shape_position: 0.0` when constructing a complete lattice value. Each shape key has a `name`
and its own full `offsets` array; `current_shape_key` is zero for the undeformed base pose or a
one-based shape-key index. All dimensions and offsets must be finite, and each `max` component
must exceed its `min` component.

Interior controls are derived when the cage is evaluated. For each interior point, Claydash
linearly interpolates the two opposing offsets along X, Y, and Z at that point, then averages
those three vectors. Its stored interior offset is ignored. Surface controls, including face,
edge, and corner controls, use their stored offsets directly. The resulting control field is
trilinearly interpolated between neighboring points. Thus, set the surrounding surface
controls to shape the interior; changing an interior array entry alone does not deform the
object. Shape keys use the same offset layout.

`CreateObject` also accepts `render_representation`, and `SetRenderRepresentation` changes that
choice on an existing object, including a Boolean group root or leaf. The choices are `exact_sdf`, `box_depth_atlas`, `sphere_depth_atlas`, and `gaussian_splats`. The capture modes record
depth and the hit material from box faces or radial sphere rays; Gaussian splats project radial hits into soft opacity along each view ray.
For example, after joining objects into a Boolean group, set the choice on its root:

```json
{"actions":[{"type":"SetRenderRepresentation","id":"<group-root-id>","render_representation":"box_depth_atlas"}]}
```

To compose groups, use `SetBoolean` with a child **group root** as `id`, the outer group's root
as `parent`, and `"operation":"Union"`. Repeat this at any depth. The group hierarchy remains
editable. Hard nested unions use the exact renderer's flat-union acceleration when eligible;
set `softness` to `0` on every group root for that path. Older saved choices that have been removed
load as `exact_sdf`.

`BoxParams` accepts `corner_radius` in addition to `box_q`. `LoftParams` accepts 2–16 ordered
sections with `x`, `center_y`, `center_z`, `half_height`, and `half_width` fields. Each section
can also have a `profile` array of 3–32 `[Y,Z]` points in unit ellipse coordinates. The points
form a closed curve and scale by half height and half width. All custom profiles in a loft must
have the same point count and matching point order; sections without a profile remain elliptical.
For a surface inlay, put `surface_inlay: {"host":"<uuid>","offset":0.018,"thickness":0.012}`
on a separate mask object whose volume crosses the host surface. The host may be a smooth Boolean
group. The mask and host group cannot have spatial modifiers, and the mask cannot be a curve.
`SetWorld`, `SetMaterials`, `SetCameras`, and `SetAnimation` replace the corresponding typed scene values.
Custom WGSL materials have three focused actions:

```json
{"actions":[{"type":"CreateCustomMaterial","name":"Blue bands","wgsl":"var surface = base; surface.color = vec3<f32>(0.1, 0.4, 0.9); return surface;"}]}
```

The response includes `created_material_ids`. Use one of those IDs in
`{"type":"AssignMaterial","id":"<material-id>","object_ids":["<object-id>"]}`. To edit it,
send `{"type":"UpdateCustomMaterial","id":"<material-id>","wgsl":"..."}`; `name` may be
sent in the same action. `get_state` includes the WGSL on each custom material asset. The code
is the body of a WGSL function with `point`, `normal`, `view`, and `base` inputs and must return
`Surface`. Its fields are `color`, `normal`, `roughness`, `metallic`, `reflectivity`, `opacity`,
`ior`, `coat`, `sheen`, `fiber`, and `figure`. For example, start with `var surface = base;`,
change fields, and finish with `return surface;`. The editor accepts up to 8192 bytes per material
and 16 custom materials per scene. Invalid WGSL makes the entire `apply` call fail without
changing the live scene. The Material panel offers the same editor and a sphere preview.

`ReplaceScene` accepts the complete `document` value returned by `get_state` and must be its only
action. `list_commands` and `execute_command` expose the existing command palette; some palette
commands initiate a mouse gesture, so typed actions are the reliable way to model geometry.

## CLI

The CLI accepts the same operation names and JSON arguments as MCP:

```sh
claydash agent GetSchema
claydash agent GetState
claydash agent Apply '{"actions":[{"type":"CreateObject","kind":"Box","position":[0,0,0]}]}'
claydash agent CaptureViewport --output /tmp/claydash-view.png
claydash agent CaptureViewport '{"mode":"outline"}' --output /tmp/claydash-outline.png
claydash agent CaptureOrthographic --output /tmp/claydash-views.png '{"panel_size":256}'
claydash agent Save '{"path":"/tmp/design.claydash"}'
```

Use the absolute binary path if `claydash` is not on `PATH`. CLI errors go to stderr and exit with
a nonzero status. Without `--output`, `CaptureViewport` returns JSON containing PNG bytes encoded
as base64.
`--output` may go before or after the JSON capture options and works for both capture operations.
`GetSceneSummary` accepts no arguments, including `{}`. Its `view.viewport_size` gives the
agent capture size in pixels; `view.fov_y_degrees` is 45 for perspective and null for orthographic.
`SelectBox` and `SelectCircle` use coordinates local to that capture: `[0,0]` is its top-left,
with X right and Y down. Desktop window position and display scale do not affect these values.

The bridge only accepts connections on the local socket, never a network port. File operations run
with the same local user permissions as the native Claydash process.

## Import Gaussian PLY assets

Use **File → Import → Gaussian PLY…** on desktop or in the browser. For MCP, call
`import_gaussian_ply` with `{"path":"/absolute/path/scan.ply"}`. The CLI equivalent is:

```sh
claydash agent ImportGaussianPly '{"path":"/absolute/path/scan.ply"}'
```

Import adds and selects one asset, returns its `id` and scene `revision`, and supports
Undo/Redo. Use the regular transform tools or `SetObjectTransform` to position it.
Saving automatically packs the Gaussian floats into a versioned binary payload inside
`.claydash`; the source PLY is never needed to reopen the project.

Supported inputs are standard Graphdeco Gaussian PLY files, in ASCII, binary little-endian,
or binary big-endian form, with up to 2,000,000 Gaussians per asset. Required properties are
`x/y/z`, `scale_0..2` (log scale), `rot_0..3` (w/x/y/z), `opacity` (logit), and `f_dc_0..2`.
Other scalar properties are accepted; degree 1–3 `f_rest_*` coefficients are retained.
Ordinary mesh/point-cloud PLY files and compressed vendor-specific layouts are rejected.

The viewport renders oriented anisotropic Gaussians with their baked base color and opacity.
Higher-order view-dependent color coefficients are preserved, but are not currently evaluated.
Assets support splat selection and transforms. They do not support solid Boolean operations,
material edits, or GLB mesh export. Save a `.claydash` project to retain the splats.

For MCP color edits, select splats with `select_box` or `select_circle` using
`cloud_object_id`, then call `adjust_gaussian_colors` with the same ID. The default scope
edits only selected splats; `"scope":"all"` edits the whole cloud without a selection.
Each call is undoable and accepts `expected_revision` from `get_scene_summary`.

```json
{"cloud_object_id":"OBJECT_UUID","operation":"desaturate","amount":1.0}
```

Operations are `desaturate` (blend toward grayscale, 0–1), `saturation` (factor 0–4),
`contrast` (factor 0–4 around 0.5), and `brightness` (add -1–1). RGB values are floored
at zero, with no highlight clipping. Opacity and higher-order coefficients stay intact.

Property mapping follows the [Graphdeco Gaussian model format](https://github.com/graphdeco-inria/gaussian-splatting/blob/main/scene/gaussian_model.py).
