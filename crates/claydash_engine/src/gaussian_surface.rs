//! Continuous surface reconstruction for Gaussian scans.
use crate::{
    gaussian_ply::{Gaussian, GaussianCloud},
    renderer::poisson_mesh::geometry::Mesh,
};
use glam::Vec3;
use std::collections::HashMap;

#[path = "gaussian_surface_texture.rs"]
mod texture;

fn is_surface_splat(splat: &Gaussian) -> bool {
    splat.position.is_finite()
        && splat.opacity >= 0.05
        && splat.scale.is_finite()
        && splat.scale.min_element() > 0.
}

fn surface_splats(cloud: &GaussianCloud) -> impl Iterator<Item = &Gaussian> {
    cloud.splats.iter().filter(|splat| is_surface_splat(splat))
}

#[derive(Clone, Copy)]
struct Sample {
    point: Vec3,
    normal: Vec3,
    color: [f32; 3],
}

struct Grid {
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
    cell: f32,
}

impl Grid {
    fn key(&self, point: Vec3) -> (i32, i32, i32) {
        let p = (point / self.cell).floor().as_ivec3();
        (p.x, p.y, p.z)
    }

    fn new(samples: &[Sample], cell: f32) -> Self {
        let mut grid = Self {
            cells: HashMap::new(),
            cell,
        };
        for (index, sample) in samples.iter().enumerate() {
            grid.cells
                .entry(grid.key(sample.point))
                .or_default()
                .push(index);
        }
        grid
    }

    fn nearby(&self, point: Vec3, reach: i32, out: &mut Vec<usize>) {
        out.clear();
        let (x, y, z) = self.key(point);
        for a in -reach..=reach {
            for b in -reach..=reach {
                for c in -reach..=reach {
                    if let Some(indices) = self.cells.get(&(x + a, y + b, z + c)) {
                        out.extend(indices);
                    }
                }
            }
        }
    }
}

fn color_grid_cell(samples: &[Sample]) -> f32 {
    let (minimum, maximum) = samples.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(minimum, maximum), sample| (minimum.min(sample.point), maximum.max(sample.point)),
    );
    ((maximum - minimum).max_element() / (samples.len() as f32).cbrt()).max(1e-6)
}

fn sample_normal(splat: &Gaussian, centroid: Vec3) -> Vec3 {
    let axis = if splat.scale.x <= splat.scale.y && splat.scale.x <= splat.scale.z {
        Vec3::X
    } else if splat.scale.y <= splat.scale.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let mut normal = splat.rotation * axis;
    if normal.dot(splat.position - centroid) < 0. {
        normal = -normal;
    }
    normal
}

fn root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn join(parent: &mut [usize], a: usize, b: usize) {
    let a = root(parent, a);
    let b = root(parent, b);
    parent[b] = a;
}

fn keep_surface_components(
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut parent: Vec<_> = (0..positions.len()).collect();
    for face in &triangles {
        join(&mut parent, face[0] as usize, face[1] as usize);
        join(&mut parent, face[0] as usize, face[2] as usize);
    }
    let mut face_counts = HashMap::<usize, usize>::new();
    for face in &triangles {
        *face_counts
            .entry(root(&mut parent, face[0] as usize))
            .or_default() += 1;
    }
    let largest = face_counts.values().copied().max().unwrap_or(0);
    let minimum_faces = 24.max(largest / 200);
    let mut indices = HashMap::<u32, u32>::new();
    let mut kept_positions = Vec::new();
    let mut kept_faces = Vec::new();
    for face in triangles {
        if face_counts
            .get(&root(&mut parent, face[0] as usize))
            .copied()
            .unwrap_or(0)
            < minimum_faces
        {
            continue;
        }
        let mut mapped = [0; 3];
        for (corner, source) in face.into_iter().enumerate() {
            mapped[corner] = *indices.entry(source).or_insert_with(|| {
                let next = kept_positions.len() as u32;
                kept_positions.push(positions[source as usize]);
                next
            });
        }
        kept_faces.push(mapped);
    }
    (kept_positions, kept_faces)
}

fn smooth_positions(mut positions: Vec<Vec3>, triangles: &[[u32; 3]]) -> Vec<Vec3> {
    let mut edges = Vec::with_capacity(triangles.len() * 3);
    for face in triangles {
        for corner in 0..3 {
            let a = face[corner] as usize;
            let b = face[(corner + 1) % 3] as usize;
            edges.push((a.min(b), a.max(b)));
        }
    }
    edges.sort_unstable();
    let mut neighbors = vec![Vec::<usize>::new(); positions.len()];
    let mut boundary = vec![false; positions.len()];
    let mut start = 0;
    while start < edges.len() {
        let edge = edges[start];
        let mut end = start + 1;
        while end < edges.len() && edges[end] == edge {
            end += 1;
        }
        neighbors[edge.0].push(edge.1);
        neighbors[edge.1].push(edge.0);
        if end - start != 2 {
            boundary[edge.0] = true;
            boundary[edge.1] = true;
        }
        start = end;
    }
    let mut next = positions.clone();
    for _ in 0..6 {
        for factor in [0.5, -0.53] {
            for (index, adjacent) in neighbors.iter().enumerate() {
                if boundary[index] || adjacent.is_empty() {
                    next[index] = positions[index];
                    continue;
                }
                let average = adjacent.iter().map(|&other| positions[other]).sum::<Vec3>()
                    / adjacent.len() as f32;
                next[index] = positions[index] + (average - positions[index]) * factor;
            }
            std::mem::swap(&mut positions, &mut next);
        }
    }
    positions
}

pub fn reconstruct(cloud: &GaussianCloud, owner: uuid::Uuid) -> Result<Mesh, String> {
    let centroid = cloud
        .splats
        .iter()
        .filter(|s| is_surface_splat(s))
        .fold((Vec3::ZERO, 0usize), |(sum, count), splat| {
            (sum + splat.position, count + 1)
        });
    if centroid.1 < 3 {
        return Err("At least three visible Gaussians are needed".into());
    }
    let centroid = centroid.0 / centroid.1 as f32;
    let samples: Vec<Sample> = surface_splats(cloud)
        .map(|splat| Sample {
            point: splat.position,
            normal: sample_normal(splat, centroid),
            color: splat.color,
        })
        .collect();
    let oriented: Vec<_> = samples
        .iter()
        .map(|sample| screened_poisson::Sample {
            point: sample.point.to_array(),
            normal: sample.normal.to_array(),
        })
        .collect();
    let solved = screened_poisson::reconstruct(
        &oriented,
        screened_poisson::Settings {
            cells: 96,
            field_smoothing_passes: 2,
            ..Default::default()
        },
        None,
        |_| {},
    )?;
    let positions = solved.positions.into_iter().map(Vec3::from_array).collect();
    let (positions, triangles) = keep_surface_components(positions, solved.triangles);
    if triangles.is_empty() {
        return Err("No continuous Gaussian surface found".into());
    }
    let positions = smooth_positions(positions, &triangles);
    let face_count = triangles.len();
    let mut mesh = Mesh::from_parts(positions, triangles, vec![owner; face_count]);
    let mut vertex_normals = vec![Vec3::ZERO; mesh.positions.len()];
    for face in &mesh.triangles {
        let [a, b, c] = face.map(|index| mesh.positions[index as usize]);
        let area_normal = (b - a).cross(c - a);
        for &index in face {
            vertex_normals[index as usize] += area_normal;
        }
    }
    mesh.normals = mesh
        .triangles
        .iter()
        .map(|face| face.map(|index| vertex_normals[index as usize].normalize_or_zero()))
        .collect();
    let cell = color_grid_cell(&samples);
    let grid = Grid::new(&samples, cell);
    let mut nearby = Vec::new();
    mesh.colors = mesh
        .positions
        .iter()
        .map(|&point| {
            grid.nearby(point, 2, &mut nearby);
            if nearby.len() > 8 {
                nearby.select_nth_unstable_by(7, |&a, &b| {
                    samples[a]
                        .point
                        .distance_squared(point)
                        .total_cmp(&samples[b].point.distance_squared(point))
                });
            }
            let mut color = [0.; 3];
            let mut total = 0.;
            for &index in nearby.iter().take(8) {
                let distance = samples[index].point.distance_squared(point);
                let weight = 1. / (distance + (cell * 0.35).powi(2));
                total += weight;
                for channel in 0..3 {
                    color[channel] += samples[index].color[channel] * weight;
                }
            }
            if total > 0. {
                color.map(|channel| channel / total)
            } else {
                [0.8; 3]
            }
        })
        .collect();
    mesh.baked_texture = Some(texture::bake(cloud, &mesh)?);
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn sheet() -> Vec<Gaussian> {
        (0..8)
            .flat_map(|y| {
                (0..8).map(move |x| Gaussian {
                    position: Vec3::new(x as f32, y as f32, 0.),
                    scale: Vec3::new(0.3, 0.3, 0.03),
                    rotation: Quat::IDENTITY,
                    color: [1., 1., 1.],
                    opacity: 1.,
                    sh_rest: Vec::new(),
                })
            })
            .collect()
    }

    #[test]
    fn surface_input_keeps_every_visible_splat() {
        let visible = sheet()[0].clone();
        let mut invisible = visible.clone();
        invisible.opacity = 0.;
        let cloud = GaussianCloud {
            splats: (0..10_000)
                .flat_map(|_| [invisible.clone(), visible.clone()])
                .collect(),
        };
        assert_eq!(surface_splats(&cloud).count(), 10_000);
    }

    #[test]
    fn conversion_builds_a_colored_textured_mesh_with_shared_normals() {
        let cloud = GaussianCloud { splats: sheet() };
        let mesh = reconstruct(&cloud, uuid::Uuid::new_v4()).unwrap();
        assert!(!mesh.triangles.is_empty());
        assert_eq!(mesh.colors.len(), mesh.positions.len());
        assert!(mesh.baked_texture.is_some());
        let mut shared_normals = HashMap::new();
        for (face, normals) in mesh.triangles.iter().zip(&mesh.normals) {
            for corner in 0..3 {
                assert!((face[corner] as usize) < mesh.positions.len());
                if let Some(previous) = shared_normals.insert(face[corner], normals[corner]) {
                    assert!(previous.distance(normals[corner]) < 1e-5);
                }
            }
        }
    }

    #[test]
    fn smoothing_reduces_interior_spike_and_keeps_boundary() {
        let mut positions = (0..3)
            .flat_map(|y| (0..3).map(move |x| Vec3::new(x as f32, y as f32, 0.)))
            .collect::<Vec<_>>();
        positions[4].z = 1.;
        let mut triangles = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                let a = (y * 3 + x) as u32;
                triangles.extend([[a, a + 1, a + 3], [a + 1, a + 4, a + 3]]);
            }
        }
        let smoothed = smooth_positions(positions.clone(), &triangles);
        assert!(smoothed[4].z < 0.6);
        for &index in &[0, 1, 2, 3, 5, 6, 7, 8] {
            assert_eq!(smoothed[index], positions[index]);
        }
    }
}
