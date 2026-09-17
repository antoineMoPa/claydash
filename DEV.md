## SDF performance benchmark

The native app includes a bounded GPU benchmark with 256 mixed spheres and boxes—far more than
the default scene. Each command warms up once, renders three offscreen frames at 384×216, reports
GPU-completion throughput, and exits. Each GPU wait has a 15-second timeout.

```sh
cargo run -- --stress-benchmark
cargo run -- --stress-benchmark-brute-force
```

