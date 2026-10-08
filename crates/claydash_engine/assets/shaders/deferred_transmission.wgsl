// Refraction uses only the shared front-surface buffer. Occluded geometry is
// unavailable in the single geometry pass; misses use the world environment.
fn transmission_color(center: vec3<f32>, normal: vec3<f32>, view: vec3<f32>, ior: f32, component: f32) -> vec4<f32> {
    let entering = dot(normal, view) >= 0.0;
    let facing = select(-normal, normal, entering);
    let eta = select(max(ior, 1.0), 1.0 / max(ior, 1.0), entering);
    let direction = refract(-view, facing, eta);
    if dot(direction, direction) < 0.0001 { return vec4(0.0); }
    let screen = screen_ray_color(center, direction, 0.0, component);
    let environment = background(direction);
    let alpha = select(1.0, screen.a, camera.world_mode.x == 3u);
    return vec4(mix(environment, screen.rgb, screen.a), alpha);
}
