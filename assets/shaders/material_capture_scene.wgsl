struct MaterialCapture { color: vec3<f32>, index: u32 }
fn material_capture(point: vec3<f32>, view: vec3<f32>, object: Object) -> MaterialCapture {
    var material_object = object;
    var captured_color = object.color.rgb;
    if object.state.y == 12 {
        let capture = neural_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 8 {
        let capture = box_depth_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 9 {
        let capture = sphere_depth_surface_sample(point, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    } else if object.state.y == 10 && object.box_depth_max.w >= 0.0 {
        let capture = gaussian_splat_surface_sample(point, -view, object);
        captured_color = capture.capture.yzw;
        material_object.component.w = capture.material_index;
    }
    return MaterialCapture(captured_color, material_object.component.w);
}
