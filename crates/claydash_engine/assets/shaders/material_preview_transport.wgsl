// Full optical transport for one sphere, including refraction and twelve interactions.
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let far = camera.inverse_view_projection * vec4(input.clip, 1.0, 1.0);
    let near = camera.inverse_view_projection * vec4(input.clip, 0.0, 1.0);
    let far_point = far.xyz / far.w;
    let near_point = near.xyz / near.w;
    let ray = select(normalize(far_point - camera.position.xyz), normalize(far_point - near_point), camera.count.w != 0u);
    let ray_origin = select(camera.position.xyz, near_point, camera.count.w != 0u);
    let primary = preview_trace(ray_origin, ray, false, 0.003);
    var hit = primary.y >= 0.0;
    var point = ray_origin + ray * primary.x;
    var index = 0u;
    if !hit { return vec4(0.0); }

    var direction = ray;
    var throughput = vec3(1.0);
    var radiance = vec3(0.0);
    var reflecting = false;
    var reflection_weight = vec3(0.0);
    var transmission_origin = vec3(0.0);
    var transmission_direction = vec3(0.0);
    var transmission_inside = false;
    var opaque = false;
    var bounce = 0;
    // Reflection and transmission share a single tracing call site to reduce
    // live shader state on integrated GPUs. Retain six surface interactions.
    for (var action = 0; action < 12; action++) {
        var next_origin = point;
        var next_direction = direction;
        var next_inside = false;
        if reflecting {
            var reflected_color = background(direction);
            if hit {
                let reflected_normal = preview_normal(point);
                reflected_color = surface_light(point, reflected_normal, -direction, objects[index],
                    material_surface(point, reflected_normal, -direction, objects[index]), false);
            }
            radiance += reflection_weight * reflected_color;
            reflecting = false;
            if opaque { break; }
            next_origin = transmission_origin;
            next_direction = transmission_direction;
            next_inside = transmission_inside;
        } else {
            if !hit {
                break;
            }
            // Retain the scene transport interaction cap for transparent materials.
            if bounce >= 12 { break; }
            let object = objects[index];
            bounce += 1;
            let outward = preview_normal(point);
            let entering = dot(direction, outward) < 0.0;
            let normal = select(-outward, outward, entering);

            var surface = material_surface(point, normal, -direction, object);
            let decal = material_stencil_color(point, normal, object);
            surface.color = mix(surface.color, decal.rgb, decal.a);
            if object.stencil_meta.w > 0.5 && object.stencil_meta.x > 0.5 && !metal_image_detail_enabled(object) {
                surface.opacity = decal.a;
                surface.reflectivity = 0.0;
                surface.ior = 1.0;
                surface.metallic = 0.0;
            }
            if HAS_METAL_MATERIAL && surface.metal_response {
                let opacity = clamp(surface.opacity, 0.0, 1.0);
                var metal_ao = 1.0;
                if bounce == 1 { metal_ao = ambient_occlusion(point, normal); }
                var metal_color = metal_light(point, -direction, surface, metal_ao);
                if bounce == 1 && opacity > 0.0 {
                    metal_color += preview_metal_reflection(point, -direction, surface, metal_ao);
                }
                radiance += throughput * max(metal_color, vec3(0.0)) * opacity;
                throughput *= 1.0 - opacity;
                opaque = opacity >= 0.999;
                if opaque || max(throughput.x, max(throughput.y, throughput.z)) < 0.01 { break; }
                let metal_origin = point + direction * 0.007;
                let next = preview_trace(metal_origin, direction, entering, 0.0015);
                hit = next.y >= 0.0;
                point = metal_origin + direction * next.x;
                index = u32(max(next.y, 0.0));
                continue;
            }
            let ior = max(surface.ior, 1.0);
            let f0 = pow((ior - 1.0) / (ior + 1.0), 2.0);
            // Matched refractive indices have zero interface reflectance at
            // every angle; Schlick's approximation alone misses this case.
            let fresnel = select(
                f0 + (1.0 - f0) * pow(1.0 - max(dot(-direction, normal), 0.0), 5.0),
                0.0, ior == 1.0);
            let reflection = reflect(direction, normal);
            let opacity = clamp(surface.opacity, 0.0, 1.0);
            let metallic = clamp(surface.metallic, 0.0, 1.0);
            let eta = select(ior, 1.0 / ior, entering);
            let transmitted = refract(direction, normal, eta);
            next_origin = point + normal * 0.007;
            next_direction = reflection;
            next_inside = !entering;
            if !(opacity < 0.999 && metallic < 0.999 && dot(transmitted, transmitted) < 0.001) {
                let roughness = clamp(surface.roughness, 0.0, 1.0);
                let weight = clamp(max(fresnel, surface.reflectivity), 0.0, 1.0);
                let tint = mix(vec3(1.0), object.color.rgb, metallic);
                reflection_weight = throughput * tint * weight * (1.0 - roughness * roughness);
                var lit_surface = vec3(0.0);
                if opacity > 0.0 {
                    lit_surface = surface_light(point, normal, -direction, object, surface, bounce == 1);
                }
                radiance += throughput * (vec3(0.22, 0.27, 0.35) * roughness * roughness * tint * weight
                    + lit_surface * opacity * (1.0 - weight));
                transmission_direction = transmitted;
                transmission_origin = point - normal * 0.007;
                transmission_inside = entering;
                throughput *= (1.0 - weight) * (1.0 - opacity) * mix(vec3(1.0), object.color.rgb, 0.12);
                opaque = opacity >= 0.999 || metallic >= 0.999;
                reflecting = true;
                // After the first glass interface, the reflected branch has
                // little energy. Keep Fresnel/environment lighting while the
                // transmitted branch continues to resolve scene geometry.
                if (bounce > 1 && !opaque) || max(reflection_weight.x, max(reflection_weight.y, reflection_weight.z)) == 0.0 {
                    radiance += reflection_weight * background(reflection);
                    reflecting = false;
                    if opaque { break; }
                    next_origin = transmission_origin;
                    next_direction = transmission_direction;
                    next_inside = transmission_inside;
                }
            }
        }
        let next = preview_trace(next_origin, next_direction, next_inside, 0.0015);
        hit = next.y >= 0.0;
        point = next_origin + next_direction * next.x;
        direction = next_direction;
        index = u32(max(next.y, 0.0));
    }
    // The final transmission miss contributes the environment even at the
    // bounce cap, just as it does after each earlier transmitted ray.
    if !reflecting && !hit && !opaque { radiance += throughput * background(direction); }
    let alpha = clamp(1.0 - dot(throughput, vec3(0.33333334)), 0.0, 1.0);
    return vec4(radiance * camera.position.w, alpha);
}
