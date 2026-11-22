//! Native studio presentation. Geometry is tessellated in world space and the
//! lights, camera and floor exist only while tracing the image.
pub mod bake;
mod geometry;
use crate::{
    model::{MaterialAsset, SdfObject},
    renderer::{
        direct_mesh,
        poisson_mesh::{self, textured_glb},
    },
};
use geometry::{Hit, Scene};
use glam::{Vec2, Vec3};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Bow,
    Stern,
    Deck,
}
impl View {
    pub const ALL: [Self; 3] = [Self::Bow, Self::Stern, Self::Deck];
    pub fn label(self) -> &'static str {
        match self {
            Self::Bow => "Bow",
            Self::Stern => "Stern",
            Self::Deck => "Deck",
        }
    }
    fn eye(self) -> Vec3 {
        match self {
            Self::Bow => Vec3::new(4.0, 3.5, 7.0),
            Self::Stern => Vec3::new(-5.0, 4.3, 6.0),
            Self::Deck => Vec3::Y,
        }
    }
}

fn direct_export(
    objects: &[SdfObject],
    assets: &[MaterialAsset],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let progress = AtomicU32::new(0);
    let mut pages = Vec::new();
    let mut material_renderer = None;
    if objects.iter().any(|o| o.imported_splats.is_some()) {
        return Err("Studio rendering requires a mesh or solid scene".into());
    }
    for object in objects.iter().filter(|o| o.boolean_parent.is_none()) {
        let mesh = match poisson_mesh::imported_mesh_world(objects, object, cancel)? {
            Some(mesh) => mesh,
            None => direct_mesh::build(
                objects,
                object.uuid,
                direct_mesh::DEFAULT_RESOLUTION,
                cancel,
                &progress,
            )
            .map_err(|e| format!("{}: {e}", object.name))?,
        };
        if mesh.triangles.is_empty() {
            continue;
        }
        let output = poisson_mesh::glb::ObjectMesh {
            name: object.name.clone(),
            mesh,
        };
        for mut page in textured_glb::material_pages(objects, object.uuid, output, None) {
            if page.materials.is_none() && page.png.is_empty() {
                if material_renderer.is_none() {
                    material_renderer = Some(pollster::block_on(
                        crate::renderer::Renderer::new_headless(crate::renderer::HeadlessOptions {
                            width: 64,
                            height: 64,
                            ..Default::default()
                        }),
                    )?);
                }
                let renderer = material_renderer.as_mut().unwrap();
                let readback = renderer.bake_poisson_page(
                    objects,
                    assets,
                    &crate::camera::Camera::new(),
                    Default::default(),
                    &page,
                )?;
                let start = std::time::Instant::now();
                page.png = loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Render cancelled".into());
                    }
                    if start.elapsed() > std::time::Duration::from_secs(60) {
                        return Err("Studio material bake timed out".into());
                    }
                    renderer.poll_poisson_bake();
                    if let Some(result) = readback.poll() {
                        break result?;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                };
            }
            pages.push(page);
        }
    }
    textured_glb::encode_textured(&pages)
}

struct Studio<'a> {
    scene: &'a Scene,
    lights: [(Vec3, Vec3, f32); 2],
}
impl Studio<'_> {
    fn shade(
        &self,
        p: Vec3,
        n: Vec3,
        eye: Vec3,
        base: Vec3,
        metal: f32,
        rough: f32,
        floor: bool,
    ) -> Vec3 {
        let s = self.scene;
        let origin = p + n * s.span * 0.00002;
        let ao = s.ambient(origin, n, if floor { 8 } else { 16 });
        let mut color = Vec3::new(0.79, 0.86, 0.94) * 0.58 * ao * base;
        for &(light, power, radius) in &self.lights {
            let delta = light - p;
            let dir = delta.normalize();
            let axis = dir.any_orthonormal_vector();
            let tangent = dir.cross(axis);
            let mut direct = Vec3::ZERO;
            let samples = if floor { 96 } else { 12 };
            for i in 0..samples {
                let (x, y) = geometry::disk(i, samples);
                let ray = delta + (axis * x + tangent * y) * radius;
                let distance = ray.length();
                let d = ray / distance;
                let cosine = d.dot(n).max(0.0);
                if cosine <= 0.0 {
                    continue;
                }
                let visibility = s.visibility(origin, d, distance);
                let half = (d + eye).normalize_or_zero();
                let exponent = 2.0 / rough.max(0.08).powi(2) - 2.0;
                let spec = n.dot(half).max(0.0).powf(exponent) * (exponent + 2.0) * 0.125;
                let brdf = base * (1.0 - metal) + Vec3::splat(0.04).lerp(base, metal) * spec;
                direct += brdf * (cosine * visibility / ray.length_squared());
            }
            color += direct * power / samples as f32;
        }
        color
    }
    fn trace(&self, mut origin: Vec3, dir: Vec3, background: Vec3, mut next: Option<Hit>) -> Vec3 {
        let s = self.scene;
        let mut color = Vec3::ZERO;
        let mut remaining = 1.0;
        for _ in 0..12 {
            let Some(hit) = next else {
                return color + remaining * background;
            };
            let face = &s.faces[hit.face];
            let base = s.color(hit);
            let p = origin + dir * hit.distance;
            let [a, b, c] = face.normals;
            let mut n =
                (a * hit.weights.x + b * hit.weights.y + c * hit.weights.z).normalize_or_zero();
            if dir.dot(n) > 0.0 {
                n = -n
            }
            color += remaining
                * base.w
                * self.shade(
                    p,
                    n,
                    -dir,
                    base.truncate(),
                    face.metallic,
                    face.roughness,
                    false,
                );
            remaining *= 1.0 - base.w;
            if remaining < 0.005 {
                break;
            }
            origin = p + dir * s.span * 0.00001;
            next = s.hit(origin, dir, (s.floor - origin.y) / dir.y);
        }
        color
    }
}

pub fn render(
    objects: &[SdfObject],
    assets: &[MaterialAsset],
    view: View,
    size: [u32; 2],
    path: &Path,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<(), String> {
    let bytes = direct_export(objects, assets, cancel)?;
    let model = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let scene = Scene::from_export(&model)?;
    let eye = view.eye().normalize();
    let right = if view == View::Deck {
        // Fix the roll explicitly at the pole to match the overhead reference.
        Vec3::NEG_X
    } else {
        Vec3::Y.cross(eye).normalize()
    };
    let up = eye.cross(right);
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = -min;
    for p in scene.faces.iter().flat_map(|f| f.points) {
        let d = p - scene.center;
        let proj = Vec2::new(right.dot(d), up.dot(d));
        min = min.min(proj);
        max = max.max(proj);
    }
    let center = (max + min) * 0.5;
    let [width, height] = size;
    let pixel = ((max.x - min.x) / width as f32).max((max.y - min.y) / height as f32) * 1.17;
    let focus = scene.center + right * center.x + up * center.y + eye * scene.span * 2.0;
    let light = |pos: Vec3, color: Vec3, power: f32, diameter: f32| {
        (
            scene.center + pos * scene.span,
            color * (power / std::f32::consts::PI.powi(2)) * scene.span.powi(2),
            diameter * scene.span * 0.5,
        )
    };
    let studio = Studio {
        scene: &scene,
        lights: [
            light(
                Vec3::new(1.35, 2.7, 1.7),
                Vec3::new(1.0, 0.92, 0.79),
                210.0,
                1.8,
            ),
            light(
                Vec3::new(-1.0, 1.7, -1.2),
                Vec3::new(0.77, 0.87, 1.0),
                150.0,
                1.25,
            ),
        ],
    };
    // The matte sweep is continuous behind the model. Filter its light samples
    // separately so the penumbra stays smooth without blurring rigging or glass.
    let floor_width = width.div_ceil(4) + 1;
    let floor_height = height.div_ceil(4) + 1;
    let mut floor = image::Rgb32FImage::new(floor_width, floor_height);
    for (x, y, texel) in floor.enumerate_pixels_mut() {
        if x == 0 && cancel.load(Ordering::Relaxed) {
            return Err("Render cancelled".into());
        }
        let origin = focus
            + pixel
                * (right * (4.0 * x as f32 - width as f32 * 0.5)
                    - up * (4.0 * y as f32 - height as f32 * 0.5));
        let point = origin - eye * ((origin.y - scene.floor) / eye.y);
        *texel = image::Rgb(
            studio
                .shade(
                    point,
                    Vec3::Y,
                    eye,
                    Vec3::new(0.123, 0.151, 0.163),
                    0.0,
                    0.8,
                    true,
                )
                .to_array(),
        );
    }
    let floor = image::imageops::blur(&floor, (scene.span / pixel * 0.003 / 4.0).clamp(0.6, 4.0));
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(12);
    let stride = width as usize * 3;
    let rows = (height as usize).div_ceil(workers);
    std::thread::scope(|scope| {
        for (band, chunk) in pixels.chunks_mut(rows * stride).enumerate() {
            let studio = &studio;
            let floor = &floor;
            scope.spawn(move || {
                for (row, line) in chunk.chunks_mut(stride).enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let y = (band * rows + row) as f32 - height as f32 * 0.5;
                    for (col, rgb) in line.chunks_exact_mut(3).enumerate() {
                        let x = col as f32 - width as f32 * 0.5;
                        let background = Vec3::from(
                            image::imageops::interpolate_bilinear(
                                floor,
                                (col as f32 + 0.5) * 0.25,
                                ((band * rows + row) as f32 + 0.5) * 0.25,
                            )
                            .expect("floor covers every output pixel")
                            .0,
                        );
                        let samples = [
                            (0.125, 0.375),
                            (0.625, 0.125),
                            (0.875, 0.625),
                            (0.375, 0.875),
                        ]
                        .map(|(dx, dy)| {
                            let pos = focus + pixel * (right * (x + dx) - up * (y + dy));
                            let hit =
                                studio
                                    .scene
                                    .hit(pos, -eye, (pos.y - studio.scene.floor) / eye.y);
                            (pos, hit)
                        });
                        // Share lighting within a triangle; keep all four coverage
                        // samples on silhouettes, thin geometry and glass.
                        let color = match samples[0].1 {
                            None if samples.iter().all(|(_, h)| h.is_none()) => background,
                            Some(hit)
                                if studio.scene.color(hit).w >= 1.0
                                    && samples
                                        .iter()
                                        .all(|(_, h)| h.is_some_and(|h| h.face == hit.face)) =>
                            {
                                let pos = focus + pixel * (right * (x + 0.5) - up * (y + 0.5));
                                let center_hit = Hit {
                                    face: hit.face,
                                    weights: samples
                                        .iter()
                                        .map(|(_, h)| h.unwrap().weights)
                                        .sum::<Vec3>()
                                        * 0.25,
                                    distance: samples
                                        .iter()
                                        .map(|(_, h)| h.unwrap().distance)
                                        .sum::<f32>()
                                        * 0.25,
                                };
                                studio.trace(pos, -eye, background, Some(center_hit))
                            }
                            _ => {
                                samples
                                    .into_iter()
                                    .map(|(pos, h)| studio.trace(pos, -eye, background, h))
                                    .sum::<Vec3>()
                                    * 0.25
                            }
                        };
                        let exposed = color * 0.62;
                        for (out, v) in rgb.iter_mut().zip(exposed.to_array()) {
                            let mapped = v * (2.51 * v + 0.03) / (v * (2.43 * v + 0.59) + 0.14);
                            *out = (255.0 * mapped.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8;
                        }
                    }
                    progress.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    if cancel.load(Ordering::Relaxed) {
        return Err("Render cancelled".into());
    }
    let temporary = path.with_file_name(format!(".claydash-studio-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        image::save_buffer_with_format(
            &temporary,
            &pixels,
            width,
            height,
            image::ColorType::Rgb8,
            image::ImageFormat::Png,
        )
        .map_err(|error| error.to_string())?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Render cancelled".into());
        }
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("write {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bake_preserves_surfaces_and_shades_sheltered_texels() {
        let objects: Vec<_> = claydash_engine::scene_language::compile(
            "seat = box(size: [1,0.1,1]); roof = box(size: [2,0.1,2], at: [0,0.18,0]);",
        )
        .unwrap()
        .into_iter()
        .map(|(_, object)| object)
        .collect();
        let cancel = AtomicBool::new(false);
        let original = direct_export(&objects, &[], &cancel).unwrap();
        let baked = bake::bake(original.clone(), &cancel).unwrap();
        let before = gltf::Gltf::from_slice(&original).unwrap();
        let after = gltf::Gltf::from_slice(&baked).unwrap();
        assert_eq!(after.nodes().count(), 2);
        let mut darkest = 1.0_f32;
        let mut lightest = 0.0_f32;
        for (a, b) in before
            .meshes()
            .flat_map(|m| m.primitives())
            .zip(after.meshes().flat_map(|m| m.primitives()))
        {
            let raw = a.reader(|_| before.blob.as_deref());
            let baked = b.reader(|_| after.blob.as_deref());
            let positions: Vec<_> = raw.read_positions().unwrap().collect();
            assert!(raw
                .read_indices()
                .unwrap()
                .into_u32()
                .map(|i| positions[i as usize])
                .eq(baked.read_positions().unwrap()));
            assert_eq!(
                a.material().pbr_metallic_roughness().base_color_factor(),
                b.material().pbr_metallic_roughness().base_color_factor()
            );
            let map = b.material().occlusion_texture().unwrap();
            assert!(baked.read_tex_coords(map.tex_coord()).is_some());
            let gltf::image::Source::View { view, .. } = map.texture().source().source() else {
                panic!("missing atlas")
            };
            let blob = after.blob.as_ref().unwrap();
            let pixels =
                image::load_from_memory(&blob[view.offset()..view.offset() + view.length()])
                    .unwrap()
                    .into_luma8();
            for sample in pixels.as_raw() {
                darkest = darkest.min(*sample as f32 / 255.0);
                lightest = lightest.max(*sample as f32 / 255.0);
            }
        }
        assert!(darkest < 0.5 && lightest > 0.95, "{darkest}..{lightest}");
    }
}
