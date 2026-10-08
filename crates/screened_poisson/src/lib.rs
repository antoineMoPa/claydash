//! Screened Poisson reconstruction of oriented surface samples.
//!
//! This implements a bounded-grid screened Poisson model with Neumann
//! boundaries. Its graph-edge gradient term approximates PoissonRecon's
//! degree-one finite-element system, while the point term uses trilinear
//! interpolation. It does not implement PoissonRecon's adaptive octree,
//! exact FEM stiffness, density weighting, or manifold extraction.

use std::{collections::HashMap, sync::atomic::{AtomicBool, Ordering}};

#[derive(Clone, Copy, Debug)]
pub struct Sample { pub point: [f32; 3], pub normal: [f32; 3] }

#[derive(Debug)]
pub struct Mesh { pub positions: Vec<[f32; 3]>, pub triangles: Vec<[u32; 3]> }

#[derive(Clone, Copy, Debug)]
pub struct Settings { pub cells: usize, pub point_weight: f32, pub max_iterations: usize }

pub const MIN_CELLS: usize = 8;
pub const MAX_CELLS: usize = 256;

fn zeroed<T: Clone>(count: usize, value: T) -> Result<Vec<T>, String> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| "Poisson grid allocation failed")?;
    values.resize(count, value);
    Ok(values)
}

impl Default for Settings {
    fn default() -> Self { Self { cells: 64, point_weight: 2.0, max_iterations: 240 } }
}

#[derive(Clone, Copy)]
struct Interpolation { nodes: [usize; 8], weights: [f32; 8] }

struct Grid {
    side: usize,
    cells: usize,
    origin: [f32; 3],
    spacing: f32,
    interpolations: Vec<Interpolation>,
    diagonal: Vec<f32>,
    right_hand: Vec<f32>,
}

impl Grid {
    #[inline] fn index(&self, x: usize, y: usize, z: usize) -> usize { (z * self.side + y) * self.side + x }

    fn interpolate(&self, point: [f32; 3]) -> Interpolation {
        let mut base = [0; 3];
        let mut fraction = [0.0; 3];
        for axis in 0..3 {
            let coordinate = ((point[axis] - self.origin[axis]) / self.spacing).clamp(0.0, self.cells as f32 - 0.00001);
            base[axis] = coordinate.floor() as usize;
            fraction[axis] = coordinate - base[axis] as f32;
        }
        let mut result = Interpolation { nodes: [0; 8], weights: [0.0; 8] };
        for corner in 0..8 {
            let x = corner & 1;
            let y = (corner >> 1) & 1;
            let z = (corner >> 2) & 1;
            result.nodes[corner] = self.index(base[0] + x, base[1] + y, base[2] + z);
            result.weights[corner] = (if x == 0 { 1.0 - fraction[0] } else { fraction[0] })
                * (if y == 0 { 1.0 - fraction[1] } else { fraction[1] })
                * (if z == 0 { 1.0 - fraction[2] } else { fraction[2] });
        }
        result
    }

    fn new(samples: &[Sample], settings: Settings, cancel: Option<&AtomicBool>) -> Result<Self, String> {
        if !(MIN_CELLS..=MAX_CELLS).contains(&settings.cells) {
            return Err(format!("Poisson grid size must be {MIN_CELLS}..={MAX_CELLS}"));
        }
        if !settings.point_weight.is_finite() || settings.point_weight < 0.0 { return Err("Invalid Poisson point weight".into()); }
        if settings.max_iterations == 0 { return Err("Poisson solver needs at least one iteration".into()); }
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for sample in samples {
            if sample.point.iter().chain(sample.normal.iter()).any(|v| !v.is_finite()) { return Err("Invalid oriented sample".into()); }
            let length_squared = sample.normal.iter().map(|v|v*v).sum::<f32>();
            if !(0.25..=4.0).contains(&length_squared) { return Err("Oriented sample has invalid normal length".into()); }
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(sample.point[axis]);
                maximum[axis] = maximum[axis].max(sample.point[axis]);
            }
        }
        let extent = (0..3).map(|axis| maximum[axis] - minimum[axis]).fold(0.0, f32::max);
        if !extent.is_finite() || extent <= 1e-7 { return Err("Oriented samples have invalid spatial extent".into()); }
        let cube = extent * 1.2;
        if !cube.is_finite() { return Err("Poisson grid exceeds finite coordinate range".into()); }
        let origin = std::array::from_fn(|axis| (minimum[axis] + maximum[axis] - cube) * 0.5);
        if origin.iter().any(|value| !value.is_finite()) { return Err("Poisson grid origin is invalid".into()); }
        let side = settings.cells.checked_add(1).ok_or("Poisson grid size overflow")?;
        let count = side.checked_pow(3).ok_or("Poisson grid size overflow")?;
        let mut interpolations = Vec::new();
        interpolations.try_reserve_exact(samples.len()).map_err(|_| "Poisson sample allocation failed")?;
        let mut normal = zeroed(count, [0.0; 3])?;
        let diagonal = zeroed(count, 0.0)?;
        let right_hand = zeroed(count, 0.0)?;
        let mut density = zeroed(count, 0.0)?;
        let mut grid = Self { side, cells: settings.cells, origin, spacing: cube / settings.cells as f32,
            interpolations, diagonal, right_hand };
        for (sample_index, sample) in samples.iter().enumerate() {
            if sample_index % 1024 == 0 && cancel.is_some_and(|flag|flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
            let interpolation = grid.interpolate(sample.point);
            for corner in 0..8 {
                let node = interpolation.nodes[corner];
                let weight = interpolation.weights[corner];
                density[node] += weight;
                for axis in 0..3 { normal[node][axis] += sample.normal[axis] * weight; }
                grid.diagonal[node] += settings.point_weight * weight * weight;
            }
            grid.interpolations.push(interpolation);
        }
        for index in 0..count {
            if density[index] > 0.0 {
                for axis in 0..3 { normal[index][axis] /= density[index]; }
            }
        }
        drop(density);
        // Each undirected grid edge contributes (u_j-u_i-h*V_edge)^2.
        // Its weak-form right hand side is the splatted normal flux.
        for z in 0..side { for y in 0..side { for x in 0..side {
            if x == 0 && y == 0 && z % 4 == 0 && cancel.is_some_and(|flag|flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
            let i = grid.index(x, y, z);
            for axis in 0..3 {
                let (nx, ny, nz) = match axis {
                    0 if x < settings.cells => (x + 1, y, z),
                    1 if y < settings.cells => (x, y + 1, z),
                    2 if z < settings.cells => (x, y, z + 1),
                    _ => continue,
                };
                let j = grid.index(nx, ny, nz);
                let flux = grid.spacing * (normal[i][axis] + normal[j][axis]) * 0.5;
                grid.right_hand[i] -= flux;
                grid.right_hand[j] += flux;
                grid.diagonal[i] += 1.0;
                grid.diagonal[j] += 1.0;
            }
        } } }
        drop(normal);
        Ok(grid)
    }

    fn multiply(&self, input: &[f32], output: &mut [f32], point_weight: f32) {
        output.fill(0.0);
        for z in 0..self.side { for y in 0..self.side { for x in 0..self.side {
            let i = self.index(x, y, z);
            if x < self.cells { let j = i + 1; let d = input[i] - input[j]; output[i] += d; output[j] -= d; }
            if y < self.cells { let j = i + self.side; let d = input[i] - input[j]; output[i] += d; output[j] -= d; }
            if z < self.cells { let j = i + self.side * self.side; let d = input[i] - input[j]; output[i] += d; output[j] -= d; }
        } } }
        for interpolation in &self.interpolations {
            let value = (0..8).map(|corner| interpolation.weights[corner] * input[interpolation.nodes[corner]]).sum::<f32>() * point_weight;
            for corner in 0..8 { output[interpolation.nodes[corner]] += interpolation.weights[corner] * value; }
        }
    }

    fn solve(&self, settings: Settings, cancel: Option<&AtomicBool>, mut progress: impl FnMut(f32)) -> Result<Vec<f32>, String> {
        let count = self.diagonal.len();
        let mut solution = zeroed(count, 0.0)?;
        let mut residual = Vec::new();
        residual.try_reserve_exact(count).map_err(|_| "Poisson grid allocation failed")?;
        residual.extend_from_slice(&self.right_hand);
        let mut direction = zeroed(count, 0.0)?;
        let mut product = zeroed(count, 0.0)?;
        for i in 0..count { product[i] = residual[i] / self.diagonal[i].max(1e-8); direction[i] = product[i]; }
        let mut rz = dot(&residual, &product);
        let initial = dot(&residual, &residual).sqrt().max(1e-20);
        for iteration in 0..settings.max_iterations {
            if iteration % 8 == 0 {
                if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
                progress(iteration as f32 / settings.max_iterations as f32);
            }
            self.multiply(&direction, &mut product, settings.point_weight);
            let denominator = dot(&direction, &product);
            if !denominator.is_finite() || !rz.is_finite() { return Err("Poisson solve diverged".into()); }
            if denominator.abs() < 1e-20 { break; }
            let alpha = rz / denominator;
            if !alpha.is_finite() { return Err("Poisson solve diverged".into()); }
            for i in 0..count { solution[i] += alpha * direction[i]; residual[i] -= alpha * product[i]; }
            if dot(&residual, &residual).sqrt() / initial < 2e-4 { break; }
            for i in 0..count { product[i] = residual[i] / self.diagonal[i].max(1e-8); }
            let next_rz = dot(&residual, &product);
            if rz.abs() < 1e-20 { break; }
            let beta = next_rz / rz;
            for i in 0..count { direction[i] = product[i] + beta * direction[i]; }
            rz = next_rz;
        }
        progress(1.0);
        if solution.iter().any(|value| !value.is_finite()) { return Err("Poisson solve produced invalid values".into()); }
        Ok(solution)
    }
}

#[inline] fn dot(a: &[f32], b: &[f32]) -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() }

pub fn reconstruct(samples: &[Sample], settings: Settings, cancel: Option<&AtomicBool>, mut progress: impl FnMut(f32)) -> Result<Mesh, String> {
    if samples.len() < 3 { return Err("Too few valid oriented samples".into()); }
    progress(0.0);
    let grid = Grid::new(samples, settings, cancel)?;
    progress(0.1);
    let solution = grid.solve(settings, cancel, |fraction| progress(0.1 + 0.7*fraction))?;
    let iso = grid.interpolations.iter().map(|interp| (0..8).map(|corner| solution[interp.nodes[corner]] * interp.weights[corner]).sum::<f32>()).sum::<f32>() / samples.len() as f32;
    if !iso.is_finite() { return Err("Poisson iso value is invalid".into()); }
    let mesh = extract(&grid, &solution, iso, cancel, |fraction| progress(0.8+0.2*fraction))?;
    progress(1.0);
    Ok(mesh)
}

fn extract(grid: &Grid, values: &[f32], iso: f32, cancel: Option<&AtomicBool>, mut progress: impl FnMut(f32)) -> Result<Mesh, String> {
    // Six tetrahedra around the same cube diagonal. Adjacent cubes choose the
    // same face diagonal, so intersections can be shared by global grid edge.
    const CORNERS: [[usize; 3]; 8] = [[0,0,0],[1,0,0],[0,1,0],[1,1,0],[0,0,1],[1,0,1],[0,1,1],[1,1,1]];
    const TETS: [[usize; 4]; 6] = [[0,1,3,7],[0,3,2,7],[0,2,6,7],[0,6,4,7],[0,4,5,7],[0,5,1,7]];
    let mut mesh = Mesh { positions: Vec::new(), triangles: Vec::new() };
    let mut edges = HashMap::<(usize, usize), u32>::new();
    for z in 0..grid.cells { for y in 0..grid.cells { for x in 0..grid.cells {
        if x == 0 && y == 0 && z % 4 == 0 {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Mesh computation cancelled".into()); }
            progress(z as f32/grid.cells as f32);
        }
        let nodes = CORNERS.map(|corner| grid.index(x + corner[0], y + corner[1], z + corner[2]));
        for tet in TETS {
            let mut inside = [0usize; 4]; let mut outside = [0usize; 4]; let mut ni = 0; let mut no = 0;
            for corner in tet { if values[nodes[corner]] < iso { inside[ni] = corner; ni += 1; } else { outside[no] = corner; no += 1; } }
            if ni == 0 || ni == 4 { continue; }
            let mut vertex = |a: usize, b: usize| -> Result<u32, String> {
                let ia = nodes[a]; let ib = nodes[b]; let key = (ia.min(ib), ia.max(ib));
                if let Some(&index) = edges.get(&key) { return Ok(index); }
                let t = ((iso - values[ia]) / (values[ib] - values[ia])).clamp(0.0, 1.0);
                let mut position = [0.0; 3];
                for axis in 0..3 {
                    let ca = [x,y,z][axis] + CORNERS[a][axis];
                    let cb = [x,y,z][axis] + CORNERS[b][axis];
                    position[axis] = grid.origin[axis] + grid.spacing * (ca as f32 + t * (cb as f32 - ca as f32));
                }
                let index = u32::try_from(mesh.positions.len()).map_err(|_| "Poisson mesh has too many vertices")?;
                mesh.positions.push(position); edges.insert(key, index); Ok(index)
            };
            // The vector from negative to positive tetrahedron corners is
            // aligned with increasing scalar value. It provides a consistent
            // orientation across all faces of each interpolated tetrahedron.
            let mut direction = [0.0f32;3];
            for axis in 0..3 {
                let low = inside[..ni].iter().map(|&corner| CORNERS[corner][axis] as f32).sum::<f32>() / ni as f32;
                let high = outside[..no].iter().map(|&corner| CORNERS[corner][axis] as f32).sum::<f32>() / no as f32;
                direction[axis] = high - low;
            }
            match ni {
                1 => { let a = inside[0]; let tri = [vertex(a,outside[0])?,vertex(a,outside[1])?,vertex(a,outside[2])?]; emit(&mut mesh,tri,direction); }
                3 => { let a = outside[0]; let tri = [vertex(a,inside[0])?,vertex(a,inside[2])?,vertex(a,inside[1])?]; emit(&mut mesh,tri,direction); }
                2 => {
                    let a=vertex(inside[0],outside[0])?; let b=vertex(inside[0],outside[1])?;
                    let c=vertex(inside[1],outside[0])?; let d=vertex(inside[1],outside[1])?;
                    emit(&mut mesh,[a,b,c],direction); emit(&mut mesh,[b,d,c],direction);
                }
                _ => unreachable!(),
            }
        }
    } } }
    if mesh.triangles.is_empty() { return Err("Poisson reconstruction returned no surface".into()); }
    progress(1.0);
    Ok(mesh)
}

fn emit(mesh: &mut Mesh, mut triangle: [u32;3], direction: [f32;3]) {
    let a=mesh.positions[triangle[0] as usize]; let b=mesh.positions[triangle[1] as usize]; let c=mesh.positions[triangle[2] as usize];
    let ab=[b[0]-a[0],b[1]-a[1],b[2]-a[2]]; let ac=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
    let cross=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]];
    if dot(&cross,&direction)<0.0 { triangle.swap(1,2); }
    mesh.triangles.push(triangle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn cancellation_during_solve_returns_error() {
        let samples=box_samples([1.0,0.8,0.6],24);
        let cancelled=AtomicBool::new(false);
        let result=reconstruct(&samples,Settings::default(),Some(&cancelled), |fraction| {
            if fraction>=0.1 { cancelled.store(true,Ordering::Relaxed); }
        });
        assert_eq!(result.unwrap_err(),"Mesh computation cancelled");
    }

    #[test]
    fn grid_accepts_resolution_above_previous_limit() {
        let samples = box_samples([1.0, 0.8, 0.6], 8);
        let settings = Settings { cells: 129, ..Settings::default() };
        let grid = Grid::new(&samples, settings, None).unwrap();
        assert_eq!(grid.diagonal.len(), 130usize.pow(3));
        let solution = grid.solve(Settings { max_iterations: 4, ..settings }, None, |_| {}).unwrap();
        assert_eq!(solution.len(), grid.diagonal.len());
        assert!(solution.iter().all(|value| value.is_finite()));
        assert!(Grid::new(&samples, Settings { cells: MAX_CELLS + 1, ..settings }, None).is_err());
    }

    #[test]
    #[ignore = "runs a full maximum-grid reconstruction; run manually for memory validation"]
    fn maximum_grid_reconstructs_sphere() {
        let start = std::time::Instant::now();
        let mut samples = Vec::new();
        for latitude in 1..24 {
            let theta = std::f32::consts::PI * latitude as f32 / 24.0;
            for longitude in 0..48 {
                let phi = std::f32::consts::TAU * longitude as f32 / 48.0;
                let point = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
                samples.push(Sample { point, normal: point });
            }
        }
        let mesh = reconstruct(&samples, Settings { cells: MAX_CELLS,
            ..Settings::default() }, None, |_| {}).unwrap();
        assert!(!mesh.triangles.is_empty());
        assert!(mesh.positions.iter().flatten().all(|coordinate| coordinate.is_finite()));
        let mut radii: Vec<_> = mesh.positions.iter().map(|point|
            (point[0] * point[0] + point[1] * point[1] + point[2] * point[2]).sqrt()).collect();
        radii.sort_unstable_by(f32::total_cmp);
        let p05 = radii[radii.len() / 20];
        let p95 = radii[radii.len() * 19 / 20];
        eprintln!("256-cell sphere: {} vertices, {} triangles, radius p05={p05:.3}, p95={p95:.3}, time {:?}",
            mesh.positions.len(), mesh.triangles.len(), start.elapsed());
        assert!(p05 > 0.7 && p95 < 1.3, "most of the sphere should remain near unit radius");
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let samples=box_samples([1.0,0.8,0.6],4);
        assert!(reconstruct(&samples,Settings{max_iterations:0,..Settings::default()},None, |_|{}).is_err());
        let mut invalid=samples.clone(); invalid[0].normal=[0.0;3];
        assert!(reconstruct(&invalid,Settings::default(),None, |_|{}).is_err());
        let mut invalid=samples; invalid[0].point=[f32::MAX,0.0,0.0];
        assert!(reconstruct(&invalid,Settings::default(),None, |_|{}).is_err());
    }

    fn box_samples(half: [f32;3], divisions: usize) -> Vec<Sample> {
        let mut samples=Vec::new();
        for axis in 0..3 { for sign in [-1.0f32,1.0] {
            for v in 0..divisions { for u in 0..divisions {
                let mut point=[0.0;3]; let mut normal=[0.0;3];
                let other=[(axis+1)%3,(axis+2)%3];
                point[axis]=sign*half[axis]; normal[axis]=sign;
                point[other[0]]=(2.0*(u as f32+0.5)/divisions as f32-1.0)*half[other[0]];
                point[other[1]]=(2.0*(v as f32+0.5)/divisions as f32-1.0)*half[other[1]];
                samples.push(Sample{point,normal});
            } }
        } }
        samples
    }

    fn assert_closed(mesh: &Mesh) {
        let mut edges=HashMap::<(u32,u32),(usize,usize)>::new();
        for triangle in &mesh.triangles { for edge in 0..3 {
            let a=triangle[edge]; let b=triangle[(edge+1)%3];
            let entry=edges.entry((a.min(b),a.max(b))).or_default(); entry.0+=1; if a<b { entry.1+=1; }
        } }
        assert_eq!(edges.values().filter(|&&(n,d)|n!=2||d!=1).count(),0);
    }

    #[test]
    fn box_and_thin_slab_are_closed() {
        for half in [[1.0,0.8,0.6],[1.0,0.8,0.08]] {
            let samples=box_samples(half,32);
            let mesh=reconstruct(&samples,Settings::default(),None, |_|{}).unwrap();
            assert_closed(&mesh);
            let minimum=std::array::from_fn::<f32,3,_>(|axis|mesh.positions.iter().map(|p|p[axis]).fold(f32::INFINITY,f32::min));
            let maximum=std::array::from_fn::<f32,3,_>(|axis|mesh.positions.iter().map(|p|p[axis]).fold(f32::NEG_INFINITY,f32::max));
            for axis in 0..3 {
                assert!((minimum[axis]+half[axis]).abs()<0.08,"axis {axis} min {}",minimum[axis]);
                assert!((maximum[axis]-half[axis]).abs()<0.08,"axis {axis} max {}",maximum[axis]);
            }
        }
    }

    #[test]
    fn nearby_disjoint_spheres_remain_separate() {
        let mut samples=Vec::new();
        for center in [-0.8f32,0.8] {
            for latitude in 1..24 { for longitude in 0..48 {
                let theta=std::f32::consts::PI*latitude as f32/24.0;
                let phi=std::f32::consts::TAU*longitude as f32/48.0;
                let normal=[theta.sin()*phi.cos(),theta.sin()*phi.sin(),theta.cos()];
                samples.push(Sample{point:[center+0.6*normal[0],0.6*normal[1],0.6*normal[2]],normal});
            } }
        }
        let mesh=reconstruct(&samples,Settings::default(),None, |_|{}).unwrap();
        assert_closed(&mesh);
        let left=mesh.triangles.iter().filter(|triangle|triangle.iter().all(|&i|mesh.positions[i as usize][0]<0.0)).count();
        let right=mesh.triangles.iter().filter(|triangle|triangle.iter().all(|&i|mesh.positions[i as usize][0]>0.0)).count();
        assert!(left>1000&&right>1000);
        assert_eq!(left+right,mesh.triangles.len(),"two surfaces joined across the gap");
    }

    #[test]
    fn sphere_is_closed_and_faces_outward() {
        let mut samples = Vec::new();
        for latitude in 1..24 {
            let theta = std::f32::consts::PI * latitude as f32 / 24.0;
            for longitude in 0..48 {
                let phi = std::f32::consts::TAU * longitude as f32 / 48.0;
                let p = [theta.sin()*phi.cos(), theta.sin()*phi.sin(), theta.cos()];
                samples.push(Sample { point: p, normal: p });
            }
        }
        let mesh = reconstruct(&samples, Settings { cells: 48, ..Settings::default() }, None, |_| {}).unwrap();
        let mut edges = HashMap::<(u32,u32),(usize,usize)>::new();
        let mut outward=0;
        let mut radii=Vec::new();
        for triangle in &mesh.triangles {
            let [a,b,c]=triangle.map(|i| mesh.positions[i as usize]);
            let ab=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];
            let ac=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
            let cross=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]];
            let center: [f32;3]=std::array::from_fn(|axis| (a[axis]+b[axis]+c[axis])/3.0);
            if dot(&cross,&center)>0.0 { outward+=1; }
            radii.push(dot(&center,&center).sqrt());
            for edge in 0..3 { let a=triangle[edge]; let b=triangle[(edge+1)%3]; let entry=edges.entry((a.min(b),a.max(b))).or_default(); entry.0+=1; if a<b { entry.1+=1; } }
        }
        radii.sort_by(f32::total_cmp);
        eprintln!("sphere: {} triangles, outward {outward}, open edges {}, wrong directions {}, median radius {}", mesh.triangles.len(), edges.values().filter(|&&(n,_)|n!=2).count(), edges.values().filter(|&&(n,d)|n==2&&d!=1).count(), radii[radii.len()/2]);
        assert!(mesh.triangles.len()>1000);
        assert!(outward*10>mesh.triangles.len()*9);
        assert_eq!(edges.values().filter(|&&(n,_)|n!=2).count(),0);
        assert_eq!(edges.values().filter(|&&(n,d)|n==2&&d!=1).count(),0);
        assert!((radii[radii.len()/2]-1.0).abs()<0.08);
    }
}
