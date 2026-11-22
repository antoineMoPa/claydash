//! Sample triangle interiors in world space and store ambient visibility in a
//! separate glTF UV atlas. The original material channels and triangle corners
//! are copied exactly, including seams on indexed meshes.
use super::geometry::Scene;
use glam::Vec3;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

struct Tile {
    corners: [usize; 3],
    width: usize,
    height: usize,
    apex: f32,
    origin: [usize; 2],
}

impl Tile {
    fn new(points: [Vec3; 3], density: f32) -> Self {
        let edges = [
            points[1] - points[0],
            points[2] - points[1],
            points[0] - points[2],
        ];
        let i = (0..3)
            .max_by(|&a, &b| {
                edges[a]
                    .length_squared()
                    .total_cmp(&edges[b].length_squared())
            })
            .unwrap();
        let corners = [i, (i + 1) % 3, (i + 2) % 3];
        let axis = edges[i].normalize_or_zero();
        let delta = points[corners[2]] - points[i];
        let projection = delta.dot(axis);
        let base = edges[i].length();
        let height = (delta - axis * projection).length();
        Self {
            corners,
            width: (base * density).ceil().clamp(1.0, 96.0) as usize + 3,
            height: (height * density).ceil().clamp(1.0, 96.0) as usize + 3,
            apex: if base > 0.0 {
                (projection / base).clamp(0.0, 1.0)
            } else {
                0.0
            },
            origin: [0, 0],
        }
    }
    fn uv(&self, width: usize, height: usize) -> [[f32; 2]; 3] {
        let mut uv = [[0.0; 2]; 3];
        for (i, (u, v)) in [(0.0, 0.0), (1.0, 0.0), (self.apex, 1.0)]
            .into_iter()
            .enumerate()
        {
            uv[self.corners[i]] = [
                (self.origin[0] as f32 + 1.5 + u * (self.width - 3) as f32) / width as f32,
                (self.origin[1] as f32 + 1.5 + v * (self.height - 3) as f32) / height as f32,
            ];
        }
        uv
    }
}

// One texel of edge dilation surrounds each island. Sort by height so long,
// narrow faces (rails and rigging) pack without wasting square tiles.
fn layout(tiles: &mut [Tile]) -> [usize; 2] {
    let area: usize = tiles.iter().map(|t| t.width * t.height).sum();
    let width = ((area as f32).sqrt().ceil() as usize)
        .max(tiles.iter().map(|t| t.width).max().unwrap())
        .next_power_of_two();
    let mut order: Vec<_> = (0..tiles.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(tiles[i].height));
    let (mut x, mut y, mut row_height) = (0, 0, 0);
    for i in order {
        let tile = &mut tiles[i];
        if x + tile.width > width {
            x = 0;
            y += row_height;
            row_height = 0
        }
        tile.origin = [x, y];
        row_height = row_height.max(tile.height);
        x += tile.width;
    }
    [width, y + row_height]
}

struct Encoder {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
}
impl Encoder {
    fn view(&mut self, data: &[u8]) -> usize {
        self.binary.resize(self.binary.len().next_multiple_of(4), 0);
        let view = self.views.len();
        self.views
            .push(json!({"buffer":0,"byteOffset":self.binary.len(),"byteLength":data.len()}));
        self.binary.extend_from_slice(data);
        view
    }
    fn attribute(&mut self, data: &[u8], mut accessor: Value, count: usize) -> usize {
        let view = self.view(data);
        self.views[view]["target"] = json!(34962);
        accessor["bufferView"] = json!(view);
        accessor["byteOffset"] = json!(0);
        accessor["count"] = json!(count);
        let id = self.accessors.len();
        self.accessors.push(accessor);
        id
    }
}

fn atlas(
    scene: &Scene,
    indices: &[u32],
    positions: &[Vec3],
    normals: &[Vec3],
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut tiles: Vec<_> = indices
        .chunks_exact(3)
        .map(|t| {
            Tile::new(
                [
                    positions[t[0] as usize],
                    positions[t[1] as usize],
                    positions[t[2] as usize],
                ],
                128.0 / scene.span,
            )
        })
        .collect();
    let [width, height] = layout(&mut tiles);
    let mut pixels = vec![255; width * height];
    let mut uv = Vec::with_capacity(indices.len() * 8);
    for (tile, triangle) in tiles.iter().zip(indices.chunks_exact(3)) {
        for v in tile.uv(width, height).into_iter().flatten() {
            uv.extend_from_slice(&v.to_le_bytes())
        }
        let [a, b, c] = tile.corners.map(|i| triangle[i] as usize);
        // Evaluate the covered part of each row, then extend its end values
        // into the unused half and gutter for bilinear texture filtering.
        for y in 0..tile.height {
            if cancel.load(Ordering::Relaxed) {
                return Err("Export cancelled".into());
            }
            let v = ((y as f32 - 1.0) / (tile.height - 3) as f32).clamp(0.0, 1.0);
            let row = tile.origin[0] + (tile.origin[1] + y) * width;
            let left = 1.0 + tile.apex * v * (tile.width - 3) as f32;
            let right = left + (1.0 - v) * (tile.width - 3) as f32;
            let first = left.floor() as usize;
            let last = (right.ceil() as usize).min(tile.width - 1);
            for x in first..=last {
                let u = ((x as f32 - left) / (tile.width - 3) as f32).clamp(0.0, 1.0 - v);
                let w = 1.0 - u - v;
                let p = positions[a] * w + positions[b] * u + positions[c] * v;
                let n = (normals[a] * w + normals[b] * u + normals[c] * v).normalize_or_zero();
                let visibility = if n == Vec3::ZERO {
                    1.0
                } else {
                    scene.ambient(p + n * scene.span * 0.00002, n, 24)
                };
                pixels[row + x] = (visibility * 255.0).round() as u8;
            }
            let start = pixels[row + first];
            let end = pixels[row + last];
            pixels[row..row + first].fill(start);
            pixels[row + last + 1..row + tile.width].fill(end);
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&pixels)
            .map_err(|e| e.to_string())?;
    }
    Ok((png, uv))
}

pub fn bake(bytes: Vec<u8>, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let model = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let scene = Scene::from_export(&model)?;
    let blob = model.blob.as_deref().ok_or("Missing GLB buffer")?;
    let mut glb = gltf::binary::Glb::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut doc: Value = serde_json::from_slice(&glb.json).map_err(|e| e.to_string())?;
    let mut out = Encoder {
        binary: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
        materials: Vec::new(),
    };
    for image in model.images() {
        let gltf::image::Source::View { view, .. } = image.source() else {
            return Err("Expected an embedded material texture".into());
        };
        doc["images"][image.index()]["bufferView"] =
            json!(out.view(&blob[view.offset()..view.offset() + view.length()]));
    }
    for mesh in model.meshes() {
        let mut primitives = Vec::new();
        for prim in mesh.primitives() {
            let reader = prim.reader(|_| Some(blob));
            let pos: Vec<_> = reader
                .read_positions()
                .ok_or("Missing positions")?
                .map(Vec3::from)
                .collect();
            let normals: Vec<_> = reader
                .read_normals()
                .ok_or("Missing normals")?
                .map(Vec3::from)
                .collect();
            let indices: Vec<_> = reader
                .read_indices()
                .ok_or("Missing indices")?
                .into_u32()
                .collect();
            for chunk in indices.chunks(1024 * 3) {
                let (png, uv) = atlas(&scene, chunk, &pos, &normals, cancel)?;
                let image_view = out.view(&png);
                let images = doc["images"].as_array_mut().unwrap();
                let image = images.len();
                images.push(json!({"bufferView":image_view,"mimeType":"image/png","name":"Baked ambient visibility"}));
                let textures = doc["textures"].as_array_mut().unwrap();
                let texture = textures.len();
                textures.push(json!({"source":image,"sampler":0}));
                let set = usize::from(prim.get(&gltf::Semantic::TexCoords(0)).is_some());
                let mut mat =
                    doc["materials"][prim.material().index().ok_or("Missing material")?].clone();
                mat["occlusionTexture"] = json!({"index":texture,"texCoord":set});
                let material = out.materials.len();
                out.materials.push(mat);
                let mut attributes = json!({});
                let original =
                    &doc["meshes"][mesh.index()]["primitives"][prim.index()]["attributes"];
                for (name, id) in original.as_object().unwrap() {
                    let id = id.as_u64().unwrap() as usize;
                    let accessor = model.accessors().nth(id).ok_or("Missing attribute")?;
                    let view = accessor.view().ok_or("Missing attribute view")?;
                    let size = accessor.size();
                    let stride = view.stride().unwrap_or(size);
                    let offset = view.offset() + accessor.offset();
                    let mut values = Vec::with_capacity(size * chunk.len());
                    for &i in chunk {
                        let start = offset + stride * i as usize;
                        values.extend_from_slice(&blob[start..start + size]);
                    }
                    let mut desc = doc["accessors"][id].clone();
                    if name == "POSITION" {
                        let mut min = Vec3::splat(f32::INFINITY);
                        let mut max = -min;
                        for &i in chunk {
                            min = min.min(pos[i as usize]);
                            max = max.max(pos[i as usize]);
                        }
                        desc["min"] = json!(min.to_array());
                        desc["max"] = json!(max.to_array());
                    }
                    attributes[name] = json!(out.attribute(&values, desc, chunk.len()));
                }
                attributes[format!("TEXCOORD_{set}")] = json!(out.attribute(
                    &uv,
                    json!({"componentType":5126,"type":"VEC2"}),
                    chunk.len()
                ));
                primitives.push(json!({"attributes":attributes,"mode":4,"material":material}));
            }
        }
        doc["meshes"][mesh.index()]["primitives"] = Value::Array(primitives);
    }
    doc["bufferViews"] = Value::Array(out.views);
    doc["accessors"] = Value::Array(out.accessors);
    doc["materials"] = Value::Array(out.materials);
    doc["buffers"][0]["byteLength"] = json!(out.binary.len());
    glb.bin = Some(out.binary.into());
    glb.json = serde_json::to_vec(&doc).map_err(|e| e.to_string())?.into();
    glb.to_vec().map_err(|e| e.to_string())
}
