//! World-space triangles from Claydash's GLB writer (nodes have identity transforms).
use glam::{Vec2, Vec3, Vec4};

pub struct Surface {
    pub points: [Vec3; 3],
    pub normals: [Vec3; 3],
    pub colors: [Vec4; 3],
    pub uv: [Vec2; 3],
    pub image: Option<usize>,
    pub roughness: f32,
    pub metallic: f32,
}

struct Node {
    min: Vec3,
    max: Vec3,
    children: Option<[usize; 2]>,
    range: std::ops::Range<usize>,
}

pub struct Scene {
    pub faces: Vec<Surface>,
    pub images: Vec<image::RgbaImage>,
    nodes: Vec<Node>,
    pub span: f32,
    pub center: Vec3,
    pub floor: f32,
}

#[derive(Clone, Copy)]
pub struct Hit {
    pub face: usize,
    pub distance: f32,
    pub weights: Vec3,
}

impl Scene {
    pub fn from_export(doc: &gltf::Gltf) -> Result<Self, String> {
        let blob = doc.blob.as_deref().ok_or("Missing GLB buffer")?;
        let mut images = Vec::new();
        for image in doc.images() {
            let gltf::image::Source::View { view, .. } = image.source() else {
                return Err("Expected an embedded image".into());
            };
            images.push(
                image::load_from_memory(&blob[view.offset()..view.offset() + view.length()])
                    .map_err(|e| e.to_string())?
                    .into_rgba8(),
            );
        }
        let mut faces = Vec::new();
        for primitive in doc.meshes().flat_map(|m| m.primitives()) {
            let reader = primitive.reader(|_| Some(blob));
            let points: Vec<_> = reader
                .read_positions()
                .ok_or("Missing positions")?
                .map(Vec3::from)
                .collect();
            let normals: Vec<_> = reader
                .read_normals()
                .ok_or("Missing normals")?
                .map(Vec3::from)
                .collect();
            let uvs: Vec<_> = reader
                .read_tex_coords(0)
                .into_iter()
                .flat_map(|u| u.into_f32())
                .map(Vec2::from)
                .collect();
            let colors: Vec<_> = reader
                .read_colors(0)
                .into_iter()
                .flat_map(|c| c.into_rgba_f32())
                .map(Vec4::from)
                .collect();
            let pbr = primitive.material().pbr_metallic_roughness();
            let factor = Vec4::from(pbr.base_color_factor());
            let image = pbr
                .base_color_texture()
                .map(|t| t.texture().source().index());
            let indices: Vec<_> = reader
                .read_indices()
                .ok_or("Missing indices")?
                .into_u32()
                .collect();
            for tri in indices.chunks_exact(3) {
                let ix = [tri[0], tri[1], tri[2]].map(|i| i as usize);
                faces.push(Surface {
                    points: ix.map(|i| points[i]),
                    normals: ix.map(|i| normals[i]),
                    colors: ix.map(|i| factor * colors.get(i).copied().unwrap_or(Vec4::ONE)),
                    uv: ix.map(|i| uvs.get(i).copied().unwrap_or_default()),
                    image,
                    roughness: pbr.roughness_factor(),
                    metallic: pbr.metallic_factor(),
                });
            }
        }
        if faces.is_empty() {
            return Err("No triangles to render".into());
        }
        let mut nodes = Vec::new();
        build(&mut faces, 0, &mut nodes, 0);
        let min = nodes[0].min;
        let max = nodes[0].max;
        let span = (max - min).max_element();
        if !span.is_finite() || span <= 0.0 {
            return Err("Invalid studio bounds".into());
        }
        Ok(Self {
            faces,
            images,
            nodes,
            span,
            center: (max + min) * 0.5,
            floor: min.y - span * 0.014,
        })
    }

    pub fn hit(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        self.intersect(origin, dir, max, false)
    }

    fn intersect(&self, origin: Vec3, dir: Vec3, max: f32, shadow: bool) -> Option<Hit> {
        let inv = dir.recip().clamp(Vec3::splat(-1e30), Vec3::splat(1e30));
        let mut closest = max;
        let mut hit = None;
        // After 24 levels of surface-area splitting, build() bisects by count.
        let mut stack = [0; usize::BITS as usize + 1];
        let mut count = 1;
        while count > 0 {
            count -= 1;
            let node = &self.nodes[stack[count]];
            let a = (node.min - origin) * inv;
            let b = (node.max - origin) * inv;
            if a.min(b).max_element().max(0.0) > a.max(b).min_element().min(closest) {
                continue;
            }
            if let Some([l, r]) = node.children {
                stack[count] = l;
                stack[count + 1] = r;
                count += 2;
                continue;
            }
            for index in node.range.clone() {
                let [a, b, c] = self.faces[index].points;
                let e = b - a;
                let f = c - a;
                let h = dir.cross(f);
                let det = e.dot(h);
                if det.abs() <= f32::MIN_POSITIVE {
                    continue;
                }
                let s = origin - a;
                let u = s.dot(h) / det;
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = s.cross(e);
                let v = dir.dot(q) / det;
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let t = f.dot(q) / det;
                if t > 0.0 && t < closest {
                    if shadow
                        && self.faces[index].image.is_none()
                        && self.faces[index].colors.iter().all(|c| c.w >= 1.0)
                    {
                        return Some(Hit {
                            face: index,
                            distance: t,
                            weights: Vec3::new(1.0 - u - v, u, v),
                        });
                    }
                    closest = t;
                    hit = Some(Hit {
                        face: index,
                        distance: t,
                        weights: Vec3::new(1.0 - u - v, u, v),
                    })
                }
            }
        }
        hit
    }

    pub fn color(&self, hit: Hit) -> Vec4 {
        let face = &self.faces[hit.face];
        let w = hit.weights;
        let [a, b, c] = face.colors;
        let base = a * w.x + b * w.y + c * w.z;
        let Some(image) = face.image.map(|i| &self.images[i]) else {
            return base;
        };
        let [a, b, c] = face.uv;
        let uv = (a * w.x + b * w.y + c * w.z).clamp(Vec2::ZERO, Vec2::ONE);
        let pixel = image.get_pixel(
            (uv.x * (image.width() - 1) as f32).round() as u32,
            (uv.y * (image.height() - 1) as f32).round() as u32,
        );
        let [r, g, b, a] = pixel.0.map(|v| v as f32 / 255.0);
        base * Vec4::new(r.powf(2.2), g.powf(2.2), b.powf(2.2), a)
    }

    pub fn visibility(&self, mut p: Vec3, d: Vec3, mut range: f32) -> f32 {
        let mut visibility = 1.0;
        for _ in 0..8 {
            let Some(hit) = self.intersect(p, d, range, true) else {
                break;
            };
            visibility *= 1.0 - self.color(hit).w;
            if visibility < 0.01 {
                return 0.0;
            }
            let step = hit.distance + self.span * 0.00001;
            p += d * step;
            range -= step;
        }
        visibility
    }

    pub fn ambient(&self, p: Vec3, n: Vec3, count: u32) -> f32 {
        let axis = n.any_orthonormal_vector();
        let tangent = n.cross(axis);
        let mut sum = 0.0;
        for i in 0..count {
            let (x, y) = disk(i, count);
            let d = axis * x + tangent * y + n * (1.0 - x * x - y * y).max(0.0).sqrt();
            sum += self.visibility(p, d, self.span * 0.18);
        }
        sum / count as f32
    }
}

pub fn disk(i: u32, count: u32) -> (f32, f32) {
    let r = ((i as f32 + 0.5) / count as f32).sqrt();
    let (s, c) = (i as f32 * 2.3999631).sin_cos();
    (r * c, r * s)
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
    count: usize,
}
impl Bounds {
    const EMPTY: Self = Self {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
        count: 0,
    };
    fn add(&mut self, other: Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.count += other.count;
    }
    fn cost(self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let d = self.max - self.min;
        self.count as f32 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
}
impl Surface {
    fn bounds(&self) -> Bounds {
        let [a, b, c] = self.points;
        Bounds {
            min: a.min(b).min(c),
            max: a.max(b).max(c),
            count: 1,
        }
    }
    fn centroid(&self) -> Vec3 {
        (self.points[0] + self.points[1] + self.points[2]) / 3.0
    }
}
fn build(faces: &mut [Surface], first: usize, nodes: &mut Vec<Node>, depth: u32) -> usize {
    let mut bounds = Bounds::EMPTY;
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = -lo;
    for f in faces.iter() {
        bounds.add(f.bounds());
        lo = lo.min(f.centroid());
        hi = hi.max(f.centroid());
    }
    let id = nodes.len();
    nodes.push(Node {
        min: bounds.min,
        max: bounds.max,
        children: None,
        range: first..first + faces.len(),
    });
    if faces.len() <= 6 {
        return id;
    }
    // Minimize expected traversal work for the long thin faces common on rails,
    // decks and masts. Centroid-median splitting alone makes their boxes overlap.
    let axis = (hi - lo).max_position();
    let extent = hi[axis] - lo[axis];
    let bin =
        |face: &Surface| (((face.centroid()[axis] - lo[axis]) * 12.0 / extent) as usize).min(11);
    let mut mid = 0;
    if extent > 0.0 && depth < 24 {
        let mut bins = [Bounds::EMPTY; 12];
        for f in faces.iter() {
            bins[bin(f)].add(f.bounds())
        }
        let mut right = [Bounds::EMPTY; 12];
        for i in (0..11).rev() {
            right[i] = right[i + 1];
            right[i].add(bins[i + 1]);
        }
        let mut left = Bounds::EMPTY;
        let mut best = f32::INFINITY;
        let mut split = 0;
        for i in 0..11 {
            left.add(bins[i]);
            let cost = left.cost() + right[i].cost();
            if left.count > 0 && right[i].count > 0 && cost < best {
                best = cost;
                split = i
            }
        }
        for i in 0..faces.len() {
            if bin(&faces[i]) <= split {
                faces.swap(mid, i);
                mid += 1;
            }
        }
    }
    if mid == 0 || mid == faces.len() {
        mid = faces.len() / 2;
        faces.select_nth_unstable_by(mid, |a, b| {
            a.centroid()[axis].total_cmp(&b.centroid()[axis])
        });
    }
    let (l, r) = faces.split_at_mut(mid);
    nodes[id].children = Some([
        build(l, first, nodes, depth + 1),
        build(r, first + mid, nodes, depth + 1),
    ]);
    id
}
