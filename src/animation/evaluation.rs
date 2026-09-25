use super::editing::cubic_bezier;
use super::*;

pub fn evaluate(tree: &mut DataTree, frame: f32) -> bool {
    migrate_legacy_lattice_tracks(tree);
    let data = animation_data(tree);
    if data.tracks.is_empty() && data.lattice_tracks.is_empty() {
        return false;
    }
    let mut scene = objects(tree);
    let mut cameras = scene_cameras(tree);
    let mut applied = false;
    let mut linked_materials = Vec::new();
    for track in &data.tracks {
        let Some(value) = sample(track, frame) else {
            continue;
        };
        if let Some(object) = scene
            .iter_mut()
            .find(|object| object.uuid == track.binding.object)
        {
            if track
                .binding
                .property
                .value(object)
                .is_some_and(|current| current.to_bits() != value.to_bits())
            {
                track.binding.property.apply(object, value);
                if track.binding.property.is_material() {
                    if let Some(material_id) = object.material_id {
                        linked_materials.push((material_id, object.material));
                    }
                }
                applied = true;
            }
        } else if let Some(camera) = cameras
            .iter_mut()
            .find(|camera| camera.uuid == track.binding.object)
        {
            if track
                .binding
                .property
                .camera_value(camera)
                .is_some_and(|current| current.to_bits() != value.to_bits())
            {
                track.binding.property.apply_camera(camera, value);
                applied = true;
            }
        }
    }
    for (material_id, material) in linked_materials {
        for object in &mut scene {
            if object.material_id == Some(material_id) {
                object.material = material;
                object.color = material.color;
            }
        }
    }
    if applied {
        set_objects_transient(tree, scene);
        tree.set_transient_path("scene.cameras", ClaydashValue::VecCamera(cameras));
    }
    applied
}

pub fn sample(track: &AnimationTrack, frame: f32) -> Option<f32> {
    let first = track.keyframes.first()?;
    if frame <= first.frame as f32 {
        return Some(first.value);
    }
    let last = track.keyframes.last()?;
    if frame >= last.frame as f32 {
        return Some(last.value);
    }
    for pair in track.keyframes.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if frame > right.frame as f32 {
            continue;
        }
        return Some(match left.interpolation {
            KeyframeInterpolation::Constant => left.value,
            KeyframeInterpolation::Linear => {
                let span = (right.frame - left.frame) as f32;
                let amount = (frame - left.frame as f32) / span;
                left.value + (right.value - left.value) * amount
            }
            KeyframeInterpolation::Bezier => {
                let [start, outgoing, incoming, end] = bezier_control_points(left, right);
                let mut low = 0.0;
                let mut high = 1.0;
                for _ in 0..24 {
                    let amount = (low + high) * 0.5;
                    let sampled_frame =
                        cubic_bezier(start.0, outgoing.0, incoming.0, end.0, amount);
                    if sampled_frame < frame {
                        low = amount;
                    } else {
                        high = amount;
                    }
                }
                let amount = (low + high) * 0.5;
                bezier_point([start, outgoing, incoming, end], amount).1
            }
        });
    }
    Some(last.value)
}
