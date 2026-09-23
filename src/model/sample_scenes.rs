use glam::{Quat, Vec3, Vec4};
use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

use super::{
    boolean_distance, object_world_matrix, BooleanOperation, BoxParams, Material, MaterialKind,
    PrimitiveKind, SdfObject, SdfParams, SphereParams, WoodSpecies,
};

/// Deterministic visual QA scene: materials above, boolean operations below.
#[cfg(not(target_arch = "wasm32"))]
pub fn ui_preview_scene() -> Vec<SdfObject> {
    let mut scene = Vec::new();
    for (index, kind) in [
        MaterialKind::Solid,
        MaterialKind::Metallic,
        MaterialKind::Transparent,
    ]
    .into_iter()
    .enumerate()
    {
        let mut sphere = SdfObject::create(TYPE_SPHERE);
        sphere.name = kind.label().into();
        sphere.params = SdfParams::SphereParams(SphereParams { radius: 0.48 });
        sphere.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, 0.7, 0.0);
        sphere.material = Material::preset(kind);
        sphere.color = sphere.material.color;
        scene.push(sphere);
    }
    let mut backdrop = SdfObject::create(TYPE_BOX);
    backdrop.name = "Orange bar behind glass".into();
    backdrop.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::new(2.1, 0.12, 0.12),
    });
    backdrop.transform.translation = Vec3::new(0.0, 0.7, -0.9);
    backdrop.material.color = Vec4::new(1.0, 0.23, 0.025, 1.0);
    backdrop.color = backdrop.material.color;
    scene.push(backdrop);
    for (index, operation) in [
        BooleanOperation::Union,
        BooleanOperation::Subtract,
        BooleanOperation::Intersect,
    ]
    .into_iter()
    .enumerate()
    {
        let mut target = SdfObject::create(TYPE_BOX);
        target.name = format!("{} target", operation.label());
        target.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.4),
        });
        target.transform.translation = Vec3::new((index as f32 - 1.0) * 1.35, -0.65, 0.0);
        target.material.color = Vec4::new(0.15, 0.6, 0.8, 1.0);
        target.color = target.material.color;
        let mut operand = SdfObject::create(TYPE_SPHERE);
        operand.name = format!("{} operand", operation.label());
        operand.params = SdfParams::SphereParams(SphereParams { radius: 0.43 });
        operand.transform.translation = target.transform.translation + Vec3::new(0.18, 0.16, 0.35);
        operand.boolean_parent = Some(target.uuid);
        operand.operation = operation;
        operand.material = target.material;
        operand.color = target.color;
        scene.extend([target, operand]);
    }
    scene
}

/// Evaluate each subtree before combining it with its parent. Parent links are
/// independent of storage order, and root objects always union with one another.
pub fn scene_sample(point: Vec3, scene: &[SdfObject]) -> Option<(f32, uuid::Uuid)> {
    fn subtree(point: Vec3, scene: &[SdfObject], index: usize, depth: usize) -> (f32, uuid::Uuid) {
        let object = &scene[index];
        let mut result = (
            object.distance_with_matrix(point, object_world_matrix(scene, object.uuid)),
            object.uuid,
        );
        if depth >= scene.len() {
            return result;
        }
        for (child_index, child) in scene.iter().enumerate() {
            if child.boolean_parent != Some(object.uuid) {
                continue;
            }
            let candidate = subtree(point, scene, child_index, depth + 1);
            let distance =
                boolean_distance(result.0, candidate.0, child.operation, object.softness);
            match child.operation {
                BooleanOperation::Union if candidate.0 < result.0 => result = candidate,
                BooleanOperation::Subtract => result.0 = result.0.max(-candidate.0),
                BooleanOperation::Intersect if candidate.0 > result.0 => result = candidate,
                _ => {}
            }
            result.0 = distance;
        }
        result
    }
    scene
        .iter()
        .enumerate()
        .filter(|(_, object)| object.boolean_parent.is_none())
        .map(|(index, _)| subtree(point, scene, index, 0))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_stress_scene() -> Vec<SdfObject> {
    use sdf_consts::{TYPE_BOX, TYPE_SPHERE};

    let layers = if std::env::args().any(|arg| arg == "--benchmark-1024") {
        16
    } else {
        4
    };
    let mut objects = Vec::with_capacity(layers * 64);
    for z in 0..layers {
        for y in 0..8 {
            for x in 0..8 {
                let object_type = if (x + y + z) % 2 == 0 {
                    TYPE_SPHERE
                } else {
                    TYPE_BOX
                };
                let mut object = SdfObject::create(object_type);
                object.transform.translation = Vec3::new(
                    (x as f32 - 3.5) * 0.42,
                    (y as f32 - 3.5) * 0.42,
                    (z as f32 - (layers as f32 - 1.0) * 0.5) * 0.42,
                );
                object.transform.rotation = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    x as f32 * 0.11,
                    y as f32 * 0.07,
                    z as f32 * 0.17,
                );
                object.transform.scale = Vec3::new(
                    0.8 + (x % 3) as f32 * 0.14,
                    0.8 + (y % 3) as f32 * 0.14,
                    0.8 + (z % 3) as f32 * 0.14,
                );
                object.color = Vec4::new(
                    0.25 + x as f32 * 0.07,
                    0.2 + y as f32 * 0.06,
                    0.35 + z as f32 * 0.14,
                    1.0,
                );
                objects.push(object);
            }
        }
    }
    objects
}

/// Deterministic fixtures for timing and pixel comparisons with every material.
#[cfg(not(target_arch = "wasm32"))]
pub fn renderer_benchmark_scenes() -> Vec<(String, Vec<SdfObject>)> {
    let mut cases = Vec::new();
    let mut wood_gallery = Vec::new();
    for (index, species) in [WoodSpecies::Pine, WoodSpecies::Oak, WoodSpecies::Walnut]
        .into_iter()
        .enumerate()
    {
        let mut block = SdfObject::create_kind(PrimitiveKind::Box);
        block.name = species.label().into();
        block.params = SdfParams::BoxParams(BoxParams {
            box_q: Vec3::splat(0.46),
        });
        block.transform.translation = Vec3::new((index as f32 - 1.0) * 1.12, 0.0, 0.0);
        block.material = Material::wood_preset(species);
        block.material.wood.cut_angle = (index as f32 - 1.0) * 0.58;
        block.color = block.material.color;
        wood_gallery.push(block);
    }
    cases.push(("wood-gallery".into(), wood_gallery));
    let mut cut_block = SdfObject::create_kind(PrimitiveKind::Box);
    cut_block.name = "Oak with drilled hole".into();
    cut_block.params = SdfParams::BoxParams(BoxParams {
        box_q: Vec3::splat(0.55),
    });
    cut_block.material = Material::wood_preset(WoodSpecies::Oak);
    cut_block.color = cut_block.material.color;
    let mut drill = SdfObject::create_kind(PrimitiveKind::Cylinder);
    drill.params = SdfParams::CylinderParams {
        radius: 0.25,
        half_height: 0.75,
    };
    drill.transform.translation = Vec3::new(0.12, 0.04, 0.0);
    drill.transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    drill.boolean_parent = Some(cut_block.uuid);
    drill.operation = BooleanOperation::Subtract;
    drill.material = cut_block.material;
    drill.color = cut_block.color;
    cases.push(("wood-cut".into(), vec![cut_block, drill]));
    for kind in MaterialKind::ALL {
        let mut scene = renderer_stress_scene();
        for object in &mut scene {
            object.material = Material::preset(kind);
            if kind == MaterialKind::Wood {
                object.color = object.material.color;
            }
        }
        let name = match kind {
            MaterialKind::Transparent => "transparent",
            MaterialKind::Metallic => "metallic",
            MaterialKind::Solid => "solid",
            MaterialKind::Wood => "wood",
        };
        cases.push((name.into(), scene));
    }
    let mut mixed = renderer_stress_scene();
    for (i, object) in mixed.iter_mut().enumerate() {
        let template = SdfObject::create(PrimitiveKind::ALL[i % 4].object_type());
        object.object_type = template.object_type;
        object.params = template.params;
        object.material = Material::preset(MaterialKind::ALL[i % 3]);
    }
    cases.push(("mixed".into(), mixed.clone()));
    for pair in mixed.chunks_mut(2) {
        pair[1].boolean_parent = Some(pair[0].uuid);
        pair[1].operation = BooleanOperation::Subtract;
    }
    cases.push(("booleans".into(), mixed.clone()));
    for group in mixed.chunks_mut(4) {
        group[0].boolean_parent = None;
        for i in 1..group.len() {
            group[i].boolean_parent = Some(group[i - 1].uuid);
            group[i].operation = if i == 2 {
                BooleanOperation::Intersect
            } else {
                BooleanOperation::Subtract
            };
        }
    }
    cases.push(("nested".into(), mixed));
    let mut repeated: Vec<_> = renderer_stress_scene().into_iter().take(64).collect();
    for (i, object) in repeated.iter_mut().enumerate() {
        let template = SdfObject::create(PrimitiveKind::ALL[i % 4].object_type());
        object.object_type = template.object_type;
        object.params = template.params;
        object.material = Material::preset(MaterialKind::ALL[i % 3]);
        object.repetition.enabled = true;
        object.repetition.axes = [true; 3];
        object.repetition.count = [3; 3];
        object.repetition.spacing = Vec3::splat(1.5);
    }
    cases.push(("repeated".into(), repeated));
    cases.push(("preview".into(), ui_preview_scene()));
    cases
}
