//! Six orthographic views of the source splats baked into independent triangle UV tiles.
use crate::gaussian_ply::GaussianCloud;
use crate::renderer::poisson_mesh::geometry::{baked_triangle_tile_size, BakedTexture, Mesh};
use glam::Vec3;

const VIEW_SIZE: usize = 512;

struct CubeView {
    depth: Vec<f32>,
    color: Vec<[f32; 3]>,
    weight: Vec<f32>,
}

impl CubeView {
    fn new() -> Self {
        Self {
            depth: vec![f32::NEG_INFINITY; VIEW_SIZE * VIEW_SIZE],
            color: vec![[0.; 3]; VIEW_SIZE * VIEW_SIZE],
            weight: vec![0.; VIEW_SIZE * VIEW_SIZE],
        }
    }
}

fn axes(face: usize, point: Vec3) -> (f32, f32, f32) {
    match face {
        0 => (-point.z, point.y, point.x),
        1 => (point.z, point.y, -point.x),
        2 => (point.x, -point.z, point.y),
        3 => (point.x, point.z, -point.y),
        4 => (point.x, point.y, point.z),
        _ => (-point.x, point.y, -point.z),
    }
}

fn face_for(normal: Vec3) -> usize {
    let axis = normal.abs();
    if axis.x >= axis.y && axis.x >= axis.z {
        if normal.x >= 0. {
            0
        } else {
            1
        }
    } else if axis.y >= axis.z {
        if normal.y >= 0. {
            2
        } else {
            3
        }
    } else if normal.z >= 0. {
        4
    } else {
        5
    }
}

fn view_direction(face: usize) -> Vec3 {
    match face {
        0 => Vec3::X,
        1 => Vec3::NEG_X,
        2 => Vec3::Y,
        3 => Vec3::NEG_Y,
        4 => Vec3::Z,
        _ => Vec3::NEG_Z,
    }
}

fn view_sample(view: &CubeView, face: usize, point: Vec3, half_extent: f32) -> Option<[f32; 3]> {
    let (u, v, depth) = axes(face, point);
    let x = pixel(u, half_extent).clamp(0., (VIEW_SIZE - 1) as f32);
    let y = pixel(v, half_extent).clamp(0., (VIEW_SIZE - 1) as f32);
    let left = x.floor() as usize;
    let top = y.floor() as usize;
    let right = (left + 1).min(VIEW_SIZE - 1);
    let bottom = (top + 1).min(VIEW_SIZE - 1);
    let fx = x - left as f32;
    let fy = y - top as f32;
    let mut color = [0.; 3];
    let mut weight = 0.;
    for (px, py, spatial) in [
        (left, top, (1. - fx) * (1. - fy)),
        (right, top, fx * (1. - fy)),
        (left, bottom, (1. - fx) * fy),
        (right, bottom, fx * fy),
    ] {
        let index = py * VIEW_SIZE + px;
        if (view.depth[index] - depth).abs() > half_extent * 0.04 {
            continue;
        }
        weight += spatial;
        for channel in 0..3 {
            color[channel] += view.color[index][channel] * spatial;
        }
    }
    (weight > 0.).then(|| color.map(|channel| channel / weight))
}

fn pixel(value: f32, half_extent: f32) -> f32 {
    (value / (2. * half_extent) + 0.5) * (VIEW_SIZE - 1) as f32
}

fn byte(linear: f32) -> u8 {
    let x = linear.clamp(0., 1.);
    let srgb = if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    };
    (srgb * 255.).round() as u8
}

pub(super) fn bake(cloud: &GaussianCloud, mesh: &Mesh) -> Result<BakedTexture, String> {
    let count = mesh.triangles.len();
    let tile =
        baked_triangle_tile_size(count).ok_or("Too many triangles for the baked scan texture")?;
    let size = (((count as f32).sqrt().ceil() as usize) * tile)
        .next_power_of_two()
        .max(64);
    if size > 4096 {
        return Err("Too many triangles for the baked scan texture".into());
    }
    let (minimum, maximum) = mesh.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(minimum, maximum), point| (minimum.min(*point), maximum.max(*point)),
    );
    let center = (minimum + maximum) * 0.5;
    let half_extent = ((maximum - minimum).max_element() * 0.52).max(1e-6);
    let mut views: [CubeView; 6] = std::array::from_fn(|_| CubeView::new());
    for splat in &cloud.splats {
        if !splat.position.is_finite()
            || !splat.scale.is_finite()
            || splat.scale.min_element() <= 0.
            || !splat.rotation.is_finite()
            || !splat.color.iter().all(|v| v.is_finite())
            || splat.opacity < 0.05
        {
            continue;
        }
        let point = splat.position - center;
        let axes_3d = [
            Vec3::X * splat.scale.x,
            Vec3::Y * splat.scale.y,
            Vec3::Z * splat.scale.z,
        ]
        .map(|axis| splat.rotation * axis);
        let pixels_per_unit = (VIEW_SIZE - 1) as f32 / (2. * half_extent);
        for (face, view) in views.iter_mut().enumerate() {
            let (u, v, depth) = axes(face, point);
            let cx = pixel(u, half_extent);
            let cy = pixel(v, half_extent);
            let projected = axes_3d.map(|axis| {
                let (u, v, _) = axes(face, axis);
                (u * pixels_per_unit, v * pixels_per_unit)
            });
            let covariance_u = 0.25 + projected.iter().map(|(u, _)| u * u).sum::<f32>();
            let covariance_v = 0.25 + projected.iter().map(|(_, v)| v * v).sum::<f32>();
            let covariance_uv = projected.iter().map(|(u, v)| u * v).sum::<f32>();
            let determinant = covariance_u * covariance_v - covariance_uv * covariance_uv;
            let largest = 0.5
                * (covariance_u
                    + covariance_v
                    + ((covariance_u - covariance_v).powi(2) + 4. * covariance_uv.powi(2)).sqrt());
            let radius = (2.5 * largest.sqrt()).ceil().clamp(1., 8.) as i32;
            let left = (cx.floor() as i32 - radius).max(0);
            let right = (cx.ceil() as i32 + radius).min(VIEW_SIZE as i32 - 1);
            let top = (cy.floor() as i32 - radius).max(0);
            let bottom = (cy.ceil() as i32 + radius).min(VIEW_SIZE as i32 - 1);
            for y in top..=bottom {
                for x in left..=right {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let squared = (covariance_v * dx * dx - 2. * covariance_uv * dx * dy
                        + covariance_u * dy * dy)
                        / determinant;
                    if squared > 6.25 {
                        continue;
                    }
                    let index = y as usize * VIEW_SIZE + x as usize;
                    let contribution = splat.opacity * (-0.5 * squared).exp();
                    let depth_window =
                        (splat.scale.max_element() * 1.5).max(half_extent / VIEW_SIZE as f32 * 2.);
                    if depth > view.depth[index] + depth_window {
                        view.depth[index] = depth;
                        view.color[index] = splat.color;
                        view.weight[index] = contribution;
                    } else if depth >= view.depth[index] - depth_window {
                        let total = view.weight[index] + contribution;
                        if total > 0. {
                            for channel in 0..3 {
                                view.color[index][channel] = (view.color[index][channel]
                                    * view.weight[index]
                                    + splat.color[channel] * contribution)
                                    / total;
                            }
                            view.weight[index] = total;
                            view.depth[index] = view.depth[index].max(depth);
                        }
                    }
                }
            }
        }
    }
    let mut pixels = vec![0u8; size * size * 4];
    let columns = size / tile;
    for (face_index, triangle) in mesh.triangles.iter().enumerate() {
        let points = triangle.map(|index| mesh.positions[index as usize]);
        let direction = (points[1] - points[0]).cross(points[2] - points[0]);
        let face = face_for(direction);
        let source_colors = triangle.map(|index| mesh.colors[index as usize]);
        let tile_x = (face_index % columns) * tile;
        let tile_y = (face_index / columns) * tile;
        let inset = tile as f32 / 8.;
        let span = tile as f32 - 2. * inset;
        for row in 0..tile {
            for column in 0..tile {
                let mut u = ((column as f32 - inset) / span).max(0.);
                let mut v = ((row as f32 - inset) / span).max(0.);
                if u + v > 1. {
                    let excess = (u + v - 1.) * 0.5;
                    u = (u - excess).clamp(0., 1.);
                    v = (v - excess).clamp(0., 1.);
                }
                let point = points[0] * (1. - u - v) + points[1] * u + points[2] * v - center;
                let normal = (mesh.normals[face_index][0] * (1. - u - v)
                    + mesh.normals[face_index][1] * u
                    + mesh.normals[face_index][2] * v)
                    .normalize_or_zero();
                let normal = if normal.length_squared() > 0. {
                    normal
                } else {
                    view_direction(face)
                };
                let mut color = [0.; 3];
                let mut total = 0.;
                for (side, view) in views.iter().enumerate() {
                    let direction_weight = normal.dot(view_direction(side)).max(0.).powi(2);
                    if direction_weight == 0. {
                        continue;
                    }
                    if let Some(sampled) = view_sample(view, side, point, half_extent) {
                        total += direction_weight;
                        for channel in 0..3 {
                            color[channel] += sampled[channel] * direction_weight;
                        }
                    }
                }
                let vertex_color = std::array::from_fn(|component| {
                    source_colors[0][component] * (1. - u - v)
                        + source_colors[1][component] * u
                        + source_colors[2][component] * v
                });
                if total > 0. {
                    for channel in 0..3 {
                        // The local color remains continuous where a cube view
                        // changes or a depth test drops its projected sample.
                        color[channel] = 0.5 * color[channel] / total + 0.5 * vertex_color[channel];
                    }
                } else {
                    color = vertex_color;
                }
                let target = ((tile_y + row) * size + tile_x + column) * 4;
                pixels[target..target + 4].copy_from_slice(&[
                    byte(color[0]),
                    byte(color[1]),
                    byte(color[2]),
                    255,
                ]);
            }
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, size as u32, size as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(&pixels)
            .map_err(|error| error.to_string())?;
    }
    Ok(BakedTexture {
        size: size as u32,
        png,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_color_blends_adjacent_pixels() {
        let mut view = CubeView::new();
        for y in [255, 256] {
            for x in [255, 256] {
                let index = y * VIEW_SIZE + x;
                view.depth[index] = 0.;
                view.color[index] = if x == 255 { [1., 0., 0.] } else { [0., 0., 1.] };
            }
        }
        let color = view_sample(&view, 4, Vec3::ZERO, 1.).unwrap();
        assert!((color[0] - 0.5).abs() < 1e-5);
        assert!((color[2] - 0.5).abs() < 1e-5);
    }
}
