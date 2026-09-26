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
4. Call `set_view` if the camera needs to move, then `capture_viewport` to get a refined PNG image.
   Repeat the edit and capture steps until the design looks right.
5. Call `save` with a `.claydash` path, or use the current document path.

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
field, including materials, repetition, mirrors, lattice, and path extrusion. `SetWorld`,
`SetMaterials`, `SetCameras`, and `SetAnimation` replace the corresponding typed scene values.
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
