//! Triangle asset decoding shared by explicit import and declarative scene loading.
use std::sync::Arc;

fn embedded_base_color_texture(
    info: gltf::texture::Info<'_>, blob: &[u8], factor: [f32; 4],
) -> Result<crate::renderer::poisson_mesh::geometry::BakedTexture, String> {
    let image = info.texture().source();
    let (view, mime_type) = match image.source() {
        gltf::image::Source::View { view, mime_type } => (view, mime_type),
        gltf::image::Source::Uri { .. } => {
            return Err("GLB base-color texture must be embedded in a buffer view".into());
        }
    };
    if view.buffer().index() != 0 || !matches!(mime_type, "image/png" | "image/jpeg") {
        return Err("GLB base-color texture must be an embedded PNG or JPEG".into());
    }
    let end = view.offset().checked_add(view.length())
        .ok_or("GLB image buffer view exceeds geometry buffer")?;
    let bytes = blob.get(view.offset()..end)
        .ok_or("GLB image buffer view exceeds geometry buffer")?;
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("invalid embedded base-color image: {error}"))?;
    let (width, height) = (image.width(), image.height());
    if width != height || !(64..=4096).contains(&width) || !width.is_power_of_two() {
        return Err("embedded base-color image must be square and power-of-two, 64–4096 pixels (mesh texture atlas limit)".into());
    }
    let mut rgba = image.to_rgba8();
    if factor.iter().any(|component| !component.is_finite()) {
        return Err("GLB base-color factor must be finite".into());
    }
    // The direct textured mesh path samples only the atlas. Apply the glTF
    // factor to the pixels so its RGB appearance survives import and export.
    for pixel in rgba.pixels_mut() {
        for channel in 0..3 {
            let srgb = pixel.0[channel] as f32 / 255.0;
            let linear = if srgb <= 0.04045 { srgb / 12.92 }
                else { ((srgb + 0.055) / 1.055).powf(2.4) };
            let linear = (linear * factor[channel]).clamp(0.0, 1.0);
            let srgb = if linear <= 0.0031308 { linear * 12.92 }
                else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
            pixel.0[channel] = (srgb * 255.0).round() as u8;
        }
        pixel.0[3] = (pixel.0[3] as f32 * factor[3]).clamp(0.0, 255.0).round() as u8;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba).write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("encode embedded base-color image: {error}"))?;
    Ok(crate::renderer::poisson_mesh::geometry::BakedTexture { size: width, png: png.into_inner() })
}

pub(crate) fn decode_glb(name: &str, bytes: &[u8]) -> Result<Vec<crate::model::SdfObject>, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|error| error.to_string())?;
    let blob = gltf
        .blob
        .as_deref()
        .ok_or("GLB has no embedded geometry buffer")?;
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or("GLB has no scene")?;
    let mut imported = Vec::new();
    let mut pending = scene
        .nodes()
        .map(|node| (node, glam::Mat4::IDENTITY))
        .collect::<Vec<_>>();
    while let Some((node, parent)) = pending.pop() {
        let transform = parent * glam::Mat4::from_cols_array_2d(&node.transform().matrix());
        pending.extend(node.children().map(|child| (child, transform)));
        let Some(mesh) = node.mesh() else { continue };
        for (primitive_index, primitive) in mesh.primitives().enumerate() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(blob));
            let mut positions = reader
                .read_positions()
                .ok_or("GLB mesh has no positions")?
                .map(|point| transform.transform_point3(glam::Vec3::from_array(point)))
                .collect::<Vec<_>>();
            let indices = reader
                .read_indices()
                .map(|indices| indices.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            if positions.len() > 10_000_000
                || indices.len() / 3 > 10_000_000
                || indices.len() % 3 != 0
                || indices
                    .iter()
                    .any(|index| *index as usize >= positions.len())
                || positions.iter().any(|position| !position.is_finite())
            {
                return Err("GLB has invalid triangle geometry".into());
            }
            if indices.is_empty() {
                continue;
            }
            let (minimum, maximum) = positions.iter().fold(
                (
                    glam::Vec3::splat(f32::INFINITY),
                    glam::Vec3::splat(f32::NEG_INFINITY),
                ),
                |(minimum, maximum), point| (minimum.min(*point), maximum.max(*point)),
            );
            let center = (minimum + maximum) * 0.5;
            for position in &mut positions {
                *position -= center;
            }
            let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
            object.transform.translation = center;
            object.name = mesh
                .name()
                .or_else(|| node.name())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{name} mesh {}", primitive_index + 1));
            object.render_representation = crate::model::GroupRenderRepresentation::DirectMesh;
            let reflected = transform.determinant() < 0.0;
            let triangles = indices
                .chunks_exact(3)
                .map(|face| {
                    if reflected {
                        [face[0], face[2], face[1]]
                    } else {
                        [face[0], face[1], face[2]]
                    }
                })
                .collect::<Vec<_>>();
            let owners = vec![object.uuid; triangles.len()];
            let mut imported_mesh = crate::renderer::poisson_mesh::geometry::Mesh::from_parts(
                positions, triangles, owners,
            );
            if let Some(normals) = reader.read_normals() {
                let normal_transform = transform.inverse().transpose();
                let normals = normals.map(|normal| {
                    let world = normal_transform.transform_vector3(glam::Vec3::from_array(normal));
                    let length = world.length();
                    if length > 1e-12 { world / length } else { glam::Vec3::ZERO }
                }).collect::<Vec<_>>();
                if normals.len() != imported_mesh.positions.len()
                    || normals.iter().any(|normal| !normal.is_finite() || normal.length_squared() < 0.5) {
                    return Err("GLB vertex normals are invalid".into());
                }
                imported_mesh.normals = imported_mesh.triangles.iter().map(|face|
                    face.map(|index| normals[index as usize])).collect();
            }
            let pbr = primitive.material().pbr_metallic_roughness();
            let color = pbr.base_color_factor();
            if let Some(info) = pbr.base_color_texture() {
                if info.tex_coord() != 0 {
                    return Err("GLB base-color texture must use TEXCOORD_0".into());
                }
                let coordinates = reader.read_tex_coords(0)
                    .ok_or("GLB textured mesh has no TEXCOORD_0")?
                    .into_f32().collect::<Vec<_>>();
                if coordinates.len() != imported_mesh.positions.len()
                    || coordinates.iter().flatten().any(|value| !value.is_finite()) {
                    return Err("GLB TEXCOORD_0 has invalid coordinates".into());
                }
                let sampler = info.texture().sampler();
                let extends_u = coordinates.iter().any(|uv| uv[0] < 0.0 || uv[0] > 1.0);
                let extends_v = coordinates.iter().any(|uv| uv[1] < 0.0 || uv[1] > 1.0);
                if (extends_u && sampler.wrap_s() != gltf::texture::WrappingMode::ClampToEdge)
                    || (extends_v && sampler.wrap_t() != gltf::texture::WrappingMode::ClampToEdge) {
                    return Err("GLB tiled base-color UVs require wrap sampling, which imported mesh textures do not support".into());
                }
                imported_mesh.uvs = imported_mesh.triangles.iter().map(|face|
                    face.map(|index| coordinates[index as usize])).collect();
                imported_mesh.baked_texture = Some(embedded_base_color_texture(info, blob, color)?);
            }
            object.imported_mesh = Some(Arc::new(imported_mesh));
            object.color = if object.imported_mesh.as_ref().unwrap().baked_texture.is_some() {
                glam::Vec4::ONE
            } else { glam::Vec4::from_array(color) };
            object.material.color = object.color;
            imported.push(object);
        }
    }
    if imported.is_empty() {
        return Err("GLB contains no triangle meshes".into());
    }
    Ok(imported)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_glb(path: &std::path::Path) -> Result<Vec<crate::model::SdfObject>, String> {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(path).map_err(|e| format!("read mesh asset {}: {e}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let mut objects = decode_glb(
        path.file_name()
            .and_then(|p| p.to_str())
            .unwrap_or("asset.glb"),
        &bytes,
    )?;
    for object in &mut objects {
        Arc::make_mut(object.imported_mesh.as_mut().unwrap()).revision =
            u64::from_le_bytes(digest[..8].try_into().unwrap());
    }
    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_glb(uv_scale: f32) -> Vec<u8> {
        let mut image_bytes = std::io::Cursor::new(Vec::new());
        let image = image::RgbaImage::from_pixel(64, 64, image::Rgba([128, 64, 32, 255]));
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut image_bytes, image::ImageFormat::Png).unwrap();
        let mut bin = Vec::new();
        let positions = [[0.0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        for position in positions { for value in position { bin.extend_from_slice(&value.to_le_bytes()); } }
        let uv_offset = bin.len();
        for uv in [[0.0f32, 0.], [uv_scale, 0.], [0., uv_scale]] {
            for value in uv { bin.extend_from_slice(&value.to_le_bytes()); }
        }
        let image_offset = bin.len();
        bin.extend_from_slice(image_bytes.get_ref());
        while bin.len() % 4 != 0 { bin.push(0); }
        let json = serde_json::json!({
            "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
            "nodes":[{"mesh":0}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"material":0}]}],
            "buffers":[{"byteLength":bin.len()}],
            "bufferViews":[
                {"buffer":0,"byteOffset":0,"byteLength":uv_offset},
                {"buffer":0,"byteOffset":uv_offset,"byteLength":image_offset-uv_offset},
                {"buffer":0,"byteOffset":image_offset,"byteLength":image_bytes.get_ref().len()}],
            "accessors":[
                {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                {"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}],
            "images":[{"bufferView":2,"mimeType":"image/png"}],
            "textures":[{"source":0}],
            "materials":[{"pbrMetallicRoughness":{"baseColorTexture":{"index":0},
                "baseColorFactor":[0.5,1,1,1]}}]
        });
        let mut json = serde_json::to_vec(&json).unwrap();
        while json.len() % 4 != 0 { json.push(b' '); }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&bin);
        glb
    }

    #[test]
    fn imports_embedded_color_texture_uvs_and_linear_factor() {
        let bytes = fixture_glb(1.0);
        let objects = decode_glb("fixture", &bytes).unwrap();
        let textured = objects.iter().filter_map(|object| object.imported_mesh.as_ref())
            .find(|mesh| mesh.baked_texture.is_some()).expect("textured source mesh");
        assert_eq!(textured.uvs.len(), textured.triangles.len());
        let texture = textured.baked_texture.as_ref().unwrap();
        assert_eq!(texture.size, 64);
        let rgba = image::load_from_memory(&texture.png).unwrap().to_rgba8();
        assert_eq!((rgba.width(), rgba.height()), (64, 64));
        assert!((rgba.get_pixel(0, 0)[0] as i32 - 92).abs() <= 1);
        assert!(textured.uvs.iter().flatten().any(|uv| uv[0] != 0. || uv[1] != 0.));
    }

    #[test]
    fn rejects_tiled_uvs_when_glb_default_sampler_repeats() {
        let error = decode_glb("fixture", &fixture_glb(2.0)).err().unwrap();
        assert!(error.contains("tiled base-color UVs"), "{error}");
    }
}
