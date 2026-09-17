# 🐥 Claydash

Claydash is an experimental 3D SDF modeler made in Rust with winit, wgpu, and egui.

https://app.claydash.com/ - note: live version does not always point to main branch.

# What we can do so far

* Add spheres and cubes via the command search tool.
* Various operations through shortcuts:
  * Grab: G
  * Scale: S
  * Rotate: R
  * Duplicate: Shift/⌘ + D

# MVP Roadmap: 

- **color picker** ✅
- **real-time-capable & serializable scene data structure** ✅
- **selecting individual sdfs (rust raymarch)** ✅
- **moving sdfs** ✅
- **scaling objects** ✅
- **multiple select** ✅
- **rotating objects** ✅
- **file menu** ✅
- boolean operations
- repetition ui
- object settings
- domain warping
- top bar buttons
- real time engine
- tree view
- perspective/ortho selection

# Running

```
cargo run
```

# Running (Web version)

Install the WebAssembly target and the `wasm-bindgen` CLI version used by the project:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128
```

Build the WebAssembly bundle and start a local server:

```sh
make build
make serve
open http://localhost:3001
```
