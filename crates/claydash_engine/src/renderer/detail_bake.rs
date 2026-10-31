//! Project an editable high-detail subtree onto a separate low-poly surface.
use super::poisson_mesh::{
    geometry::{BakedTexture, Mesh},
    textured_glb::Page,
};
use crate::model::{MaterialKind, PreparedSubtreeSampler, SdfObject};
use glam::Vec3;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub parent_id: uuid::Uuid,
    pub source_id: uuid::Uuid,
    pub projection_distance: f32,
    #[serde(default = "default_texture_size")]
    pub texture_size: u32,
    /// Optional world direction from surface toward light. Shadows become fixed albedo.
    pub shadow_direction: Option<[f32; 3]>,
}
fn default_texture_size() -> u32 {
    512
}

pub struct Maps {
    pub normal_png: Vec<u8>,
    /// glTF occlusion R, roughness G, metallic B, all linear.
    pub orm_png: Vec<u8>,
}

impl Settings {
    pub fn validate(self, scene: &[SdfObject]) -> Result<(), String> {
        if !self.projection_distance.is_finite()
            || self.projection_distance <= 0.0
            || !(self.projection_distance * 8.0).is_finite()
        {
            return Err("projection_distance must be positive and finite (world units)".into());
        }
        if !(64..=2048).contains(&self.texture_size) || !self.texture_size.is_power_of_two() {
            return Err("texture_size must be a power of two from 64 to 2048".into());
        }
        if let Some(direction) = self.shadow_direction {
            let direction = Vec3::from_array(direction);
            if !direction.is_finite()
                || !direction.length_squared().is_finite()
                || direction.length_squared() < 1e-12
            {
                return Err("shadow_direction must be finite and nonzero".into());
            }
        }
        for id in [self.parent_id, self.source_id] {
            if !scene.iter().any(|object| object.uuid == id) {
                return Err(format!("unknown object: {id}"));
            }
        }
        // Validate only the sampled subtree. Other assembly materials do not matter.
        for object in scene
            .iter()
            .filter(|object| descendant(scene, object.uuid, self.source_id))
        {
            if !matches!(
                object.material.kind,
                MaterialKind::Solid | MaterialKind::Metallic
            ) || object.material_id.is_some()
                || object.image_stencil.is_some()
                || object.imported_splats.is_some()
                || object.imported_mesh.is_some()
                || object.material.emission_intensity > 0.0
                || object.softness > 0.0
            {
                return Err(format!("detail baking currently requires hard SDF geometry with solid/metallic materials; unsupported source: {}", object.uuid));
            }
        }
        Ok(())
    }
}

fn descendant(scene: &[SdfObject], id: uuid::Uuid, root: uuid::Uuid) -> bool {
    let mut current = Some(id);
    for _ in 0..=scene.len() {
        let Some(id) = current else { return false };
        if id == root {
            return true;
        }
        current = scene
            .iter()
            .find(|object| object.uuid == id)
            .and_then(|object| object.boolean_parent);
    }
    false
}

fn encode_png(size: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(pixels)
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}
fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
fn srgb(value: f32) -> u8 {
    byte(if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.max(0.0).powf(1.0 / 2.4) - 0.055
    })
}

/// First surface on a bounded inward cage ray. Absolute steps also handle inside starts.
fn trace(
    sampler: &mut PreparedSubtreeSampler<'_>,
    origin: Vec3,
    direction: Vec3,
    distance: f32,
    epsilon: f32,
) -> Option<(Vec3, uuid::Uuid)> {
    let mut t = 0.0;
    for _ in 0..256 {
        let point = origin + direction * t;
        let (d, owner) = sampler.sample(point);
        if !d.is_finite() {
            return None;
        }
        if d.abs() < epsilon {
            return Some((point, owner));
        }
        t += (d.abs() * 0.8).max(epsilon * 0.5);
        if t > distance {
            break;
        }
    }
    None
}
fn surface_normal(
    sampler: &mut PreparedSubtreeSampler<'_>,
    p: Vec3,
    e: f32,
    fallback: Vec3,
) -> Vec3 {
    let d = |s: &mut PreparedSubtreeSampler<'_>, v| s.sample(v).0;
    let gradient = Vec3::new(
        d(sampler, p + Vec3::X * e) - d(sampler, p - Vec3::X * e),
        d(sampler, p + Vec3::Y * e) - d(sampler, p - Vec3::Y * e),
        d(sampler, p + Vec3::Z * e) - d(sampler, p - Vec3::Z * e),
    );
    let length = gradient.length();
    if length > 1e-10 && length.is_finite() {
        gradient / length
    } else {
        fallback
    }
}

pub fn bake(
    scene: &[SdfObject],
    settings: Settings,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Page, String> {
    settings.validate(scene)?;
    let parent = scene.iter().find(|o| o.uuid == settings.parent_id).unwrap();
    // Retain the parent's ancestors for world transforms, but omit its detail children.
    let low: Vec<_> = scene
        .iter()
        .filter(|o| descendant(scene, settings.parent_id, o.uuid))
        .cloned()
        .collect();
    let mut mesh: Mesh = super::direct_mesh::build(&low, settings.parent_id, 8, cancel, progress)?;
    let count = mesh.triangles.len();
    if count == 0 {
        return Err("bake parent has no triangles".into());
    }
    let columns = (count as f64).sqrt().ceil() as u32;
    let tile = settings.texture_size / columns;
    if tile < 8 {
        return Err(
            "parent has too many faces for texture_size; use a simpler parent or larger texture"
                .into(),
        );
    }
    let size = settings.texture_size;
    let mut albedo = vec![255; (size * size * 4) as usize];
    let mut normal = vec![255; albedo.len()];
    let mut orm = vec![255; albedo.len()];
    for pixel in normal.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[128, 128, 255, 255]);
    }
    for pixel in orm.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 255, 0, 255]);
    }
    let mut sampler =
        PreparedSubtreeSampler::new(scene, settings.source_id).ok_or("missing source")?;
    let objects: std::collections::HashMap<_, _> = scene.iter().map(|o| (o.uuid, o)).collect();
    let e = (settings.projection_distance * 0.0001).max(1e-6);
    let light = settings
        .shadow_direction
        .map(Vec3::from_array)
        .map(|v| v / v.length());
    let mut hits = 0usize;
    mesh.uvs.clear();
    for face in 0..count {
        let [a, b, c] = mesh.triangle(face);
        let x = face as u32 % columns * tile;
        let y = face as u32 / columns * tile;
        let inset = 2.0;
        let span = tile as f32 - 4.0;
        mesh.uvs.push(
            [
                [x as f32 + inset, y as f32 + inset],
                [x as f32 + inset + span, y as f32 + inset],
                [x as f32 + inset, y as f32 + inset + span],
            ]
            .map(|uv| [uv[0] / size as f32, uv[1] / size as f32]),
        );
        for row in 0..tile {
            super::direct_mesh::check_cancel(cancel)?;
            for col in 0..tile {
                let mut u = ((col as f32 + 0.5 - inset) / span).max(0.0);
                let mut v = ((row as f32 + 0.5 - inset) / span).max(0.0);
                if u + v > 1.0 {
                    let sum = u + v;
                    u /= sum;
                    v /= sum;
                }
                let p = a * (1.0 - u - v) + b * u + c * v;
                let n = mesh.normals[face][0] * (1.0 - u - v)
                    + mesh.normals[face][1] * u
                    + mesh.normals[face][2] * v;
                let n = n / n.length();
                let edge = b - a;
                let tangent = edge - n * edge.dot(n);
                let tangent = tangent / tangent.length();
                // Atlas V grows along c-a. Keep the handedness for glTF's UV-derived tangent frame.
                let sign = if n.cross(tangent).dot(c - a) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let bitangent = n.cross(tangent) * sign;
                let hit = trace(
                    &mut sampler,
                    p + n * settings.projection_distance,
                    -n,
                    settings.projection_distance * 2.0,
                    e,
                );
                let offset = (((y + row) * size + x + col) * 4) as usize;
                let Some((surface, owner)) = hit else {
                    // A miss retains the parent's solid appearance and flat tangent normal.
                    albedo[offset..offset + 4].copy_from_slice(&[
                        srgb(parent.color.x),
                        srgb(parent.color.y),
                        srgb(parent.color.z),
                        byte(parent.color.w * parent.material.opacity),
                    ]);
                    continue;
                };
                hits += 1;
                let sn = surface_normal(&mut sampler, surface, e, n);
                let object = objects[&owner];
                let mut ao = 0.0;
                for step in 1..=5 {
                    let distance = settings.projection_distance * step as f32 / 5.0;
                    ao += ((distance - sampler.sample(surface + sn * distance).0) / distance)
                        .clamp(0.0, 1.0)
                        / 5.0;
                }
                let shadow = light.map_or(1.0, |l| {
                    if trace(
                        &mut sampler,
                        surface + sn * (e * 8.0),
                        l,
                        settings.projection_distance * 8.0,
                        e,
                    )
                    .is_some()
                    {
                        0.25
                    } else {
                        1.0
                    }
                });
                albedo[offset..offset + 4].copy_from_slice(&[
                    srgb(object.color.x * shadow),
                    srgb(object.color.y * shadow),
                    srgb(object.color.z * shadow),
                    byte(object.color.w * object.material.opacity),
                ]);
                normal[offset..offset + 4].copy_from_slice(&[
                    byte(sn.dot(tangent) * 0.5 + 0.5),
                    byte(sn.dot(bitangent) * 0.5 + 0.5),
                    byte(sn.dot(n) * 0.5 + 0.5),
                    255,
                ]);
                orm[offset..offset + 4].copy_from_slice(&[
                    byte(1.0 - ao),
                    byte(object.material.roughness),
                    byte(object.material.metallic),
                    255,
                ]);
            }
        }
        progress.store(((face + 1) * 100 / count) as u32, Ordering::Relaxed);
    }
    if hits == 0 {
        return Err("no source surface reached the parent projection cage; increase projection_distance or align the source".into());
    }
    let png = encode_png(size, &albedo)?;
    let maps = Maps {
        normal_png: encode_png(size, &normal)?,
        orm_png: encode_png(size, &orm)?,
    };
    mesh.baked_texture = Some(BakedTexture {
        size,
        png: png.clone(),
    });
    Ok(Page {
        name: format!("{} (baked detail)", parent.name),
        root: parent.uuid,
        mesh: Arc::new(mesh),
        first: 0,
        count,
        size,
        png,
        colors: None,
        materials: None,
        detail_maps: Some(maps),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PrimitiveKind, SdfParams};

    fn fixture() -> (Vec<SdfObject>, Settings) {
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.color = glam::Vec4::new(0.1, 0.3, 0.8, 1.0);
        if let SdfParams::BoxParams(p) = &mut parent.params {
            p.corner_radius = 0.0;
        }
        let mut detail = SdfObject::create_kind(PrimitiveKind::Sphere);
        detail.boolean_parent = Some(parent.uuid);
        detail.transform.translation = Vec3::new(0.0, 0.0, 0.5);
        detail.color = glam::Vec4::new(0.9, 0.02, 0.01, 1.0);
        let settings = Settings {
            parent_id: parent.uuid,
            source_id: parent.uuid,
            projection_distance: 1.0,
            texture_size: 64,
            shadow_direction: None,
        };
        (vec![parent, detail], settings)
    }

    #[test]
    fn projects_detail_without_adding_geometry_and_exports_linear_maps() {
        let (scene, settings) = fixture();
        let original = serde_json::to_value(&scene).unwrap();
        let page = bake(
            &scene,
            settings,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        let low = super::super::direct_mesh::build(
            &scene[..1],
            scene[0].uuid,
            8,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert_eq!(page.mesh.positions, low.positions);
        assert_eq!(page.mesh.triangles, low.triangles);
        assert_eq!(serde_json::to_value(&scene).unwrap(), original);
        let color = image::load_from_memory(&page.png).unwrap().to_rgba8();
        assert!(
            color
                .pixels()
                .any(|p| u16::from(p[0]) > u16::from(p[2]) + 30),
            "red detail projected onto blue parent"
        );
        let shadowed = bake(
            &scene,
            Settings {
                shadow_direction: Some([1.0, 0.0, 1.0]),
                ..settings
            },
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        let shadowed = image::load_from_memory(&shadowed.png).unwrap().to_rgba8();
        assert!(
            color
                .pixels()
                .zip(shadowed.pixels())
                .any(|(a, b)| a[0] > b[0] || a[2] > b[2]),
            "optional fixed-light self shadows darken some texels"
        );
        let maps = page.detail_maps.as_ref().unwrap();
        let normals = image::load_from_memory(&maps.normal_png)
            .unwrap()
            .to_rgba8();
        assert!(normals
            .pixels()
            .any(|p| p[0].abs_diff(128) > 20 || p[1].abs_diff(128) > 20));
        let orm = image::load_from_memory(&maps.orm_png).unwrap().to_rgba8();
        assert!(
            orm.pixels().any(|p| p[0] < 250),
            "detail contact occludes ambient light"
        );
        let bytes = super::super::poisson_mesh::textured_glb::encode_textured(&[page]).unwrap();
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let glb: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
        assert_eq!(glb["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(glb["images"].as_array().unwrap().len(), 3);
        let material = &glb["materials"][0];
        assert!(material["normalTexture"]["index"].is_number());
        assert!(material["occlusionTexture"]["index"].is_number());
        assert!(material["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"].is_number());
    }

    #[test]
    fn rejects_invalid_settings_unsupported_sources_and_honors_cancel() {
        let (mut scene, settings) = fixture();
        for invalid in [
            Settings {
                texture_size: 63,
                ..settings
            },
            Settings {
                projection_distance: f32::NAN,
                ..settings
            },
            Settings {
                shadow_direction: Some([0.0; 3]),
                ..settings
            },
        ] {
            assert!(invalid.validate(&scene).is_err());
        }
        assert!(bake(&scene, settings, &AtomicBool::new(true), &AtomicU32::new(0)).is_err());
        scene[1].material.kind = MaterialKind::Wood;
        assert!(settings
            .validate(&scene)
            .unwrap_err()
            .contains("unsupported source"));
    }

    #[test]
    fn transformed_parent_preserves_world_geometry_and_separate_source_can_miss() {
        let (mut scene, settings) = fixture();
        scene[0].transform.translation = Vec3::new(2.0, 3.0, -1.0);
        scene[0].transform.rotation = glam::Quat::from_rotation_y(0.4);
        let page = bake(
            &scene,
            settings,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )
        .unwrap();
        assert!(page.mesh.positions.iter().all(|p| p.x > 0.0));
        let mut distant = SdfObject::create_kind(PrimitiveKind::Sphere);
        distant.transform.translation = Vec3::splat(100.0);
        let missed = Settings {
            source_id: distant.uuid,
            ..settings
        };
        scene.push(distant);
        assert!(
            bake(&scene, missed, &AtomicBool::new(false), &AtomicU32::new(0))
                .err()
                .unwrap()
                .contains("no source surface")
        );
    }
}
