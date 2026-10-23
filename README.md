# 🐥 Claydash

Claydash is an experimental 3D modeler built with Rust, winit, wgpu, and egui. It supports SDF shapes, Boolean modeling, materials, Gaussian splats, meshes, animation, and rendering.

[Try the web app](https://app.claydash.com/) (the live version may differ from `main`).

## Get started

Run the native editor:

```sh
cargo run
```

For the web build, install the WebAssembly target and the project's `wasm-bindgen-cli` version, then run `make build` and `make serve`. See [web setup](docs.md#running-web-version) for the commands.

## Documentation

- [Features, workflows, development, and roadmap](docs.md)
- [User guide](docs/guide/index.html)
- [Agent and MCP integration](docs/agent-integration.md)
- [Rendering performance](docs/rendering-performance.md)
- [Engine crate](crates/claydash_engine/README.md)
