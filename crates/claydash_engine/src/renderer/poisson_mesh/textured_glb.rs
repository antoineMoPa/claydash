use super::geometry::Mesh;
use glam::Vec3;
use serde_json::{json, Value};
pub(crate) const TILE: u32 = 16;
pub const PAGE_TRIANGLES: usize = 128 * 128;
pub struct Page {
    pub name: String,
    pub root: uuid::Uuid,
    pub mesh: std::sync::Arc<Mesh>,
    pub first: usize,
    pub count: usize,
    pub size: u32,
    pub png: Vec<u8>,
}
pub fn page_size(count: usize) -> u32 {
    ((count as f32).sqrt().ceil() as u32 * TILE)
        .next_power_of_two()
        .max(64)
}
pub(crate) fn triangle_uv(index: usize, size: u32) -> [[f32; 2]; 3] {
    let columns = size / TILE;
    let x = (index as u32 % columns) * TILE;
    let y = (index as u32 / columns) * TILE;
    [[x + 2, y + 2], [x + 14, y + 2], [x + 2, y + 14]]
        .map(|p| [p[0] as f32 / size as f32, p[1] as f32 / size as f32])
}
fn buffer_view(data: &mut Vec<u8>, views: &mut Vec<Value>, bytes: &[u8]) -> usize {
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let index = views.len();
    views.push(json!({"buffer":0,"byteOffset":data.len(),"byteLength":bytes.len()}));
    data.extend_from_slice(bytes);
    index
}
pub fn encode_textured(pages: &[Page]) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("No meshes to export".into());
    }
    let mut binary = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::<Value>::new();
    let mut object_meshes = std::collections::HashMap::<uuid::Uuid, usize>::new();
    let mut nodes = Vec::new();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut materials = Vec::new();
    for page in pages {
        if page.png.is_empty()
            || page
                .first
                .checked_add(page.count)
                .is_none_or(|end| end > page.mesh.triangles.len())
        {
            return Err("Incomplete texture bake".into());
        }
        let mut positions = Vec::<[f32; 3]>::new();
        let mut normals = Vec::<[f32; 3]>::new();
        let mut uvs = Vec::<[f32; 2]>::new();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = -min;
        for i in 0..page.count {
            let t = page.mesh.triangle(page.first + i);
            for (corner, p) in t.into_iter().enumerate() {
                min = min.min(p);
                max = max.max(p);
                positions.push(p.to_array());
                normals.push(page.mesh.normals[page.first + i][corner].to_array());
            }
            uvs.extend_from_slice(&triangle_uv(i, page.size));
        }
        let p = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&positions));
        let n = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&normals));
        let uv = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&uvs));
        let indices: Vec<u32> = (0..positions.len() as u32).collect();
        let ix = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&indices));
        for view in [p, n, uv] {
            views[view]["target"] = json!(34962);
        }
        views[ix]["target"] = json!(34963);
        let base = accessors.len();
        accessors.push(json!({"bufferView":p,"componentType":5126,"count":positions.len(),"type":"VEC3","min":min.to_array(),"max":max.to_array()}));
        accessors
            .push(json!({"bufferView":n,"componentType":5126,"count":normals.len(),"type":"VEC3"}));
        accessors
            .push(json!({"bufferView":uv,"componentType":5126,"count":uvs.len(),"type":"VEC2"}));
        accessors.push(
            json!({"bufferView":ix,"componentType":5125,"count":indices.len(),"type":"SCALAR"}),
        );
        let img = buffer_view(&mut binary, &mut views, &page.png);
        let index = images.len();
        images.push(json!({"bufferView":img,"mimeType":"image/png"}));
        textures.push(json!({"source":index,"sampler":0}));
        let image = image::load_from_memory(&page.png)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let mut transparent = false;
        // Ignore unused transparent tiles when deciding whether this primitive
        // needs blending in a glTF viewer.
        for i in 0..page.count {
            let columns = page.size / TILE;
            let x = (i as u32 % columns) * TILE;
            let y = (i as u32 / columns) * TILE;
            if image.width() == page.size && image.height() == page.size {
                for row in y..y + TILE {
                    for col in x..x + TILE {
                        transparent |= image.get_pixel(col, row).0[3] < 255;
                    }
                }
            } else {
                transparent = true;
            }
        }
        materials.push(json!({"name":"Baked materials","pbrMetallicRoughness":{"baseColorTexture":{"index":index},"metallicFactor":0,"roughnessFactor":1},"doubleSided":true,"alphaMode":if transparent {"BLEND"} else {"OPAQUE"}}));
        let primitive = json!({"attributes":{"POSITION":base,"NORMAL":base+1,"TEXCOORD_0":base+2},"indices":base+3,"material":index,"mode":4});
        if let Some(&mesh) = object_meshes.get(&page.root) {
            meshes[mesh]["primitives"]
                .as_array_mut()
                .unwrap()
                .push(primitive);
        } else {
            object_meshes.insert(page.root, meshes.len());
            nodes.push(json!({"name":page.name,"mesh":meshes.len()}));
            meshes.push(json!({"name":page.name,"primitives":[primitive]}));
        }
    }
    let mut document=serde_json::to_vec(&json!({"asset":{"version":"2.0","generator":"Claydash"},"scene":0,"scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],"nodes":nodes,"meshes":meshes,"materials":materials,"textures":textures,"images":images,"samplers":[{"magFilter":9729,"minFilter":9729,"wrapS":33071,"wrapT":33071}],"buffers":[{"byteLength":binary.len()}],"bufferViews":views,"accessors":accessors})).map_err(|e|e.to_string())?;
    while document.len() % 4 != 0 {
        document.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let length = 28usize
        .checked_add(document.len())
        .and_then(|v| v.checked_add(binary.len()))
        .filter(|v| *v <= u32::MAX as usize)
        .ok_or("GLB exceeds 4 GiB")?;
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(length as u32).to_le_bytes());
    out.extend_from_slice(&(document.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&document);
    out.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&binary);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn textured_glb_has_uv_image_and_relightable_material() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![255; 64 * 64 * 4]).unwrap();
        }
        let fold = 30f32.to_radians();
        let mesh = std::sync::Arc::new(Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y,
                Vec3::new(0.0, -fold.cos(), -fold.sin())],
            vec![[0, 1, 2], [1, 0, 3]], vec![uuid::Uuid::nil(); 2]));
        let bytes = encode_textured(&[Page { name: "Material test".into(),
            root: uuid::Uuid::nil(), mesh, first: 0, count: 2,
            size: 64, png: png_bytes }]).unwrap();
        if let Some(path) = std::env::var_os("CLAYDASH_TEXTURED_GLB_TEST_OUTPUT") {
            std::fs::write(path, &bytes).unwrap();
        }
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert_eq!(doc["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"], 2);
        assert!(doc["materials"][0]["extensions"]["KHR_materials_unlit"].is_null());
        assert!(doc["extensionsRequired"].is_null());
        assert_eq!(doc["images"][0]["mimeType"], "image/png");
        assert_eq!(doc["textures"][0]["source"], 0);
        let normal_view = &doc["bufferViews"][1];
        let offset = 28 + len + normal_view["byteOffset"].as_u64().unwrap() as usize;
        let read = |corner: usize, axis: usize| {
            let at = offset + corner * 12 + axis * 4;
            f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
        };
        assert!(read(0, 1) < -0.1, "GLB must export smoothed normals");
        assert!((read(0, 1) - read(4, 1)).abs() < 1e-5,
            "shared corners must carry the same smooth normal");
    }
}
