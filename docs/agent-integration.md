# Local agents and Claydash

The native Claydash window exposes its live scene and viewport through a local Unix socket. The
same operations are available through a CLI and an MCP server, so Codex, Claude Code, and OpenCode
can edit the scene you are looking at and inspect the rendered viewport. The browser build does not
start this local bridge.

## Start and connect

Build the native binary and start the window:

```sh
cargo build --release
./target/release/claydash
```

For an agent-only session without a visible UI, start `claydash --agent-headless` instead. It
keeps a hidden native GPU window so `capture_viewport` still renders images.

Use the **absolute path** to that binary in your agent configuration. Claydash creates
`~/.claydash-agent/agent.sock` with owner-only permissions when the window starts. One native
window owns the socket at a time. The MCP server and CLI connect to that window; they do not start
another renderer.

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

## Agent workflow

1. Call `get_schema` for primitive examples and the action names.
2. Call `get_state` for the live objects, world, materials, cameras, animation, selection, full
   `.claydash` document, and `revision`.
3. Call `apply` with one or more typed actions. Pass `expected_revision` from `get_state` so an
   intervening scene edit causes a clear error. Actions in one call form one undo step. A failed
   call leaves the live scene untouched.
   Try grouping related objects into named Boolean unions for better organization and to move,
   rotate, or scale the whole model at once. For example, keep a sword's blade, guard, grip,
   pommel, and decorations in one union named `Sword`. Use an existing component as the root,
   rename it with `SetObjectName`, and attach the other components with `SetBoolean`, setting
   `parent` to the root UUID, `operation` to `Union`, and `softness` to `0` to preserve sharp
   edges. Components retain their materials and remain individually editable. Nested unions
   can organize larger assemblies. To transform the complete union through MCP, update the
   root's `group_transform` with `PutObject`.
4. Call `set_view` if the camera needs to move, then `capture_viewport` to get a PNG image.
   The capture renders offscreen even when the native window is unfocused or on another macOS
   desktop. Pass `object_ids` to show only specific objects or Boolean groups and their surface
   inlays in that PNG. Captures use `mode: "simple_shading"` by default; use
   `mode: "full_material"` for close material inspection or `mode: "outline"` to see primitive
   wire guides for every captured object, including hidden Boolean operands. Outline PNGs contain
   the guides on a dark background, without editor controls or gizmos. The scene, selection, and
   live viewport mode stay unchanged. Legacy `refine: true` selects full material rendering and
   `refine: false` selects simple shading; an explicit `mode` takes precedence over `refine`.
   Repeat the edit and capture steps until the design looks right.
5. Call `capture_orthographic` for one PNG with X (side), Y (top), and Z (front) orthographic views
   aimed at the origin. Each panel is 256 px square by default. It accepts the same `object_ids`
   and `mode`/`refine` options, plus optional `panel_size` and camera `distance`.
6. Call `save` with a `.claydash` path, or use the current document path.

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
claydash agent Save '{"path":"/tmp/design.claydash"}'
```

Use the absolute binary path if `claydash` is not on `PATH`. CLI errors go to stderr and exit with
a nonzero status. Without `--output`, `CaptureViewport` returns JSON containing PNG bytes encoded
as base64.

The bridge only accepts connections on the local socket, never a network port. File operations run
with the same local user permissions as the native Claydash process.
