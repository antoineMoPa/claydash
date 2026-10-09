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
    /// One linear RGBA color per mesh triangle; these pages skip texture baking.
    pub colors: Option<std::sync::Arc<Vec<[f32; 4]>>>,
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
        if (page.colors.is_none() && page.png.is_empty())
            || page
                .colors
                .as_ref()
                .is_some_and(|colors| colors.len() != page.mesh.triangles.len())
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
        let mut colors = Vec::<[f32; 4]>::new();
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
            if let Some(sampled) = &page.colors {
                colors.extend([sampled[page.first + i]; 3]);
            } else {
                uvs.extend_from_slice(&triangle_uv(i, page.size));
            }
        }
        let p = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&positions));
        let n = buffer_view(&mut binary, &mut views, bytemuck::cast_slice(&normals));
        let uv = buffer_view(
            &mut binary,
            &mut views,
            if page.colors.is_some() {
                bytemuck::cast_slice(&colors)
            } else {
                bytemuck::cast_slice(&uvs)
            },
        );
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
            .push(json!({"bufferView":uv,"componentType":5126,"count":positions.len(),"type":if page.colors.is_some() { "VEC4" } else { "VEC2" }}));
        accessors.push(
            json!({"bufferView":ix,"componentType":5125,"count":indices.len(),"type":"SCALAR"}),
        );
        let material_index = materials.len();
        if page.colors.is_some() {
            let transparent = colors.iter().any(|color| color[3] < 1.0);
            materials.push(json!({"name":"Voxel colors","pbrMetallicRoughness":{
                "baseColorFactor":[1,1,1,1],"metallicFactor":0,"roughnessFactor":1},
                "alphaMode":if transparent {"BLEND"} else {"OPAQUE"}}));
        } else {
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
        }
        let mut attributes = json!({"POSITION":base,"NORMAL":base+1});
        attributes[if page.colors.is_some() {
            "COLOR_0"
        } else {
            "TEXCOORD_0"
        }] = json!(base + 2);
        let primitive =
            json!({"attributes":attributes,"indices":base+3,"material":material_index,"mode":4});
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
            vec![
                Vec3::ZERO,
                Vec3::X,
                Vec3::Y,
                Vec3::new(0.0, -fold.cos(), -fold.sin()),
            ],
            vec![[0, 1, 2], [1, 0, 3]],
            vec![uuid::Uuid::nil(); 2],
        ));
        let bytes = encode_textured(&[Page {
            name: "Material test".into(),
            root: uuid::Uuid::nil(),
            mesh,
            first: 0,
            count: 2,
            size: 64,
            png: png_bytes,
            colors: None,
        }])
        .unwrap();
        if let Some(path) = std::env::var_os("CLAYDASH_TEXTURED_GLB_TEST_OUTPUT") {
            std::fs::write(path, &bytes).unwrap();
        }
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert_eq!(
            doc["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"],
            2
        );
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
        assert!(
            (read(0, 1) - read(4, 1)).abs() < 1e-5,
            "shared corners must carry the same smooth normal"
        );
    }
}

#[cfg(test)]
mod voxel_tests {
    use super::*;
    use crate::model::{PrimitiveKind, SdfObject};
    use std::sync::{
        atomic::{AtomicBool, AtomicU32},
        Arc,
    };

    fn document(bytes: &[u8]) -> (Value, usize) {
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        (
            serde_json::from_slice(&bytes[20..20 + length]).unwrap(),
            28 + length,
        )
    }

    #[test]
    fn voxel_glb_keeps_flat_colors_world_pose_and_reflected_winding() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.color = glam::Vec4::new(0.2, 0.4, 0.6, 1.0);
        object.group_transform.translation = Vec3::new(3.0, -2.0, 1.0);
        object.group_transform.rotation = glam::Quat::from_rotation_z(0.4);
        object.group_transform.scale = Vec3::new(-2.0, 1.5, 0.75);
        let source = [object.clone()];
        let local = crate::renderer::voxels::geometry::build(
            &source,
            object.uuid,
            8,
            &AtomicU32::new(0),
            None,
        )
        .unwrap();
        let frame = crate::model::lattice_world_matrix(&source, object.uuid);
        let expected_min = local
            .mesh
            .positions
            .iter()
            .map(|point| frame.transform_point3(*point))
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let expected_max = local
            .mesh
            .positions
            .iter()
            .map(|point| frame.transform_point3(*point))
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        let world = local
            .into_world(&source, object.uuid, &AtomicBool::new(false))
            .unwrap();
        let count = world.mesh.triangles.len();
        let bytes = encode_textured(&[Page {
            name: "Voxel box".into(),
            root: object.uuid,
            mesh: Arc::new(world.mesh),
            first: 0,
            count,
            size: 64,
            png: Vec::new(),
            colors: Some(Arc::new(world.colors)),
        }])
        .unwrap();
        let (doc, binary) = document(&bytes);
        let attributes = &doc["meshes"][0]["primitives"][0]["attributes"];
        assert!(attributes["TEXCOORD_0"].is_null());
        assert!(attributes["COLOR_0"].is_number());
        assert_eq!(doc["images"].as_array().unwrap().len(), 0);
        assert!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"].is_null());
        assert_eq!(doc["accessors"][0]["min"], json!(expected_min.to_array()));
        assert_eq!(doc["accessors"][0]["max"], json!(expected_max.to_array()));
        let values = |attribute: &str, components: usize| -> Vec<Vec<f32>> {
            let accessor = &doc["accessors"][attributes[attribute].as_u64().unwrap() as usize];
            let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
            let offset = binary + view["byteOffset"].as_u64().unwrap() as usize;
            (0..accessor["count"].as_u64().unwrap() as usize)
                .map(|index| {
                    (0..components)
                        .map(|axis| {
                            let at = offset + (index * components + axis) * 4;
                            f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
                        })
                        .collect()
                })
                .collect()
        };
        let positions = values("POSITION", 3);
        let normals = values("NORMAL", 3);
        let colors = values("COLOR_0", 4);
        assert_eq!(positions.len(), count * 3);
        assert!(colors.iter().all(|color| color == &[0.2, 0.4, 0.6, 1.0]));
        for index in (0..positions.len()).step_by(3) {
            let p = |i: usize| Vec3::from_slice(&positions[i]);
            let normal = Vec3::from_slice(&normals[index]);
            assert_eq!(normals[index], normals[index + 1]);
            assert_eq!(normals[index], normals[index + 2]);
            assert!((normal.length() - 1.0).abs() < 1e-5);
            assert!(
                (p(index + 1) - p(index))
                    .cross(p(index + 2) - p(index))
                    .dot(normal)
                    > 0.0
            );
        }
    }

    #[test]
    fn mixed_glb_keeps_texture_and_vertex_color_material_indices_separate() {
        let mesh = Arc::new(Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            vec![[0, 1, 2]],
            vec![uuid::Uuid::nil()],
        ));
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![255; 64 * 64 * 4])
                .unwrap();
        }
        let colored = Page {
            name: "Cubes".into(),
            root: uuid::Uuid::new_v4(),
            mesh: mesh.clone(),
            first: 0,
            count: 1,
            size: 64,
            png: Vec::new(),
            colors: Some(Arc::new(vec![[0.1, 0.2, 0.3, 1.0]])),
        };
        let smooth = Page {
            name: "Smooth".into(),
            root: uuid::Uuid::new_v4(),
            mesh,
            first: 0,
            count: 1,
            size: 64,
            png,
            colors: None,
        };
        let (doc, _) = document(&encode_textured(&[colored, smooth]).unwrap());
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(doc["meshes"][0]["primitives"][0]["material"], 0);
        assert_eq!(doc["meshes"][1]["primitives"][0]["material"], 1);
        assert!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"].is_null());
        assert_eq!(
            doc["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"]["index"],
            0
        );
        assert_eq!(doc["textures"][0]["source"], 0);
    }
}
