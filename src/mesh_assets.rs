//! Triangle asset decoding shared by explicit import and declarative scene loading.
use std::sync::Arc;

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
            object.imported_mesh = Some(Arc::new(
                crate::renderer::poisson_mesh::geometry::Mesh::from_parts(
                    positions, triangles, owners,
                ),
            ));
            let color = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_factor();
            object.color = glam::Vec4::from_array(color);
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
