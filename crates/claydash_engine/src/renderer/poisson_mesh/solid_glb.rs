//! Indexed, relightable solid materials. Procedural/custom materials retain baking.
use super::{
    geometry::Mesh,
    textured_glb::{buffer_view, Page},
};
use crate::model::{Material, MaterialKind, SdfObject};
use glam::Vec3;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub fn materials_for_mesh(scene: &[SdfObject], mesh: &Mesh) -> Option<BTreeMap<Uuid, Material>> {
    let mut materials = BTreeMap::new();
    for owner in &mesh.owners {
        if materials.contains_key(owner) {
            continue;
        }
        let object = scene.iter().find(|object| object.uuid == *owner)?;
        if object.material.kind != MaterialKind::Solid
            || object.material_id.is_some()
            || object.image_stencil.is_some()
        {
            return None;
        }
        if object.surface_inlay.is_some() {
            return None;
        }
        let mut ancestor = Some(object);
        while let Some(part) = ancestor {
            if part.softness > 0.0
                && scene
                    .iter()
                    .any(|child| child.boolean_parent == Some(part.uuid))
            {
                return None;
            }
            ancestor = part
                .boolean_parent
                .and_then(|id| scene.iter().find(|o| o.uuid == id));
        }
        let mut material = object.material;
        material.color = object.color;
        materials.insert(*owner, material);
    }
    (!materials.is_empty()).then_some(materials)
}

pub(super) fn append(
    page: &Page,
    binary: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    materials: &mut Vec<Value>,
) -> Result<Vec<Value>, String> {
    let source = page.materials.as_ref().ok_or("Missing solid materials")?;
    if page.mesh.normals.len() != page.mesh.triangles.len()
        || page.mesh.owners.len() != page.mesh.triangles.len()
        || (!page.mesh.uvs.is_empty() && page.mesh.uvs.len() != page.mesh.triangles.len())
    {
        return Err("Incomplete solid mesh normals/owners".into());
    }
    let mut by_owner = BTreeMap::<Uuid, Vec<usize>>::new();
    for i in page.first..page.first + page.count {
        by_owner.entry(page.mesh.owners[i]).or_default().push(i);
    }
    let mut primitives = Vec::new();
    for (owner, faces) in by_owner {
        let material = source.get(&owner).ok_or("Missing solid owner material")?;
        let mut positions = Vec::<[f32; 3]>::new();
        let mut normals = Vec::<[f32; 3]>::new();
        let mut uvs = Vec::<[f32; 2]>::new();
        let mut indices = Vec::<u32>::new();
        let mut shared = HashMap::new();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = -min;
        let has_uvs = !page.mesh.uvs.is_empty();
        for face in faces {
            for (corner, vertex) in page.mesh.triangles[face].into_iter().enumerate() {
                let p = page.mesh.positions[vertex as usize];
                let normal = page.mesh.normals[face][corner];
                let uv = if has_uvs { page.mesh.uvs[face][corner] } else { [0.0; 2] };
                if !p.is_finite() || !normal.is_finite() || uv.iter().any(|value| !value.is_finite()) {
                    return Err("Nonfinite solid mesh".into());
                }
                let key = (
                    vertex,
                    normal
                        .to_array()
                        .map(|v| if v == 0.0 { 0 } else { v.to_bits() }),
                    uv.map(|v| if v == 0.0 { 0 } else { v.to_bits() }),
                );
                let index = *shared.entry(key).or_insert_with(|| {
                    let index = positions.len() as u32;
                    positions.push(p.to_array());
                    normals.push(normal.to_array());
                    if has_uvs { uvs.push(uv); }
                    min = min.min(p);
                    max = max.max(p);
                    index
                });
                indices.push(index);
            }
        }
        let pos = buffer_view(binary, views, bytemuck::cast_slice(&positions));
        views[pos]["target"] = json!(34962);
        let norm = buffer_view(binary, views, bytemuck::cast_slice(&normals));
        views[norm]["target"] = json!(34962);
        let uv_view = has_uvs.then(|| {
            let view = buffer_view(binary, views, bytemuck::cast_slice(&uvs));
            views[view]["target"] = json!(34962);
            view
        });
        let short = positions.len() <= 65536;
        let ix = if short {
            let indices: Vec<u16> = indices.iter().map(|i| *i as u16).collect();
            buffer_view(binary, views, bytemuck::cast_slice(&indices))
        } else {
            buffer_view(binary, views, bytemuck::cast_slice(&indices))
        };
        views[ix]["target"] = json!(34963);
        let base = accessors.len();
        accessors.push(json!({"bufferView":pos,"componentType":5126,"count":positions.len(),"type":"VEC3","min":min.to_array(),"max":max.to_array()}));
        accessors.push(
            json!({"bufferView":norm,"componentType":5126,"count":normals.len(),"type":"VEC3"}),
        );
        accessors.push(json!({"bufferView":ix,"componentType":if short{5123}else{5125},"count":indices.len(),"type":"SCALAR"}));
        if let Some(view) = uv_view {
            accessors.push(json!({"bufferView":view,"componentType":5126,"count":uvs.len(),"type":"VEC2"}));
        }
        let alpha = (material.color.w * material.opacity).clamp(0.0, 1.0);
        let mut mat = json!({"name":"Solid material","pbrMetallicRoughness":{"baseColorFactor":[material.color.x,material.color.y,material.color.z,alpha],"metallicFactor":material.metallic.clamp(0.0,1.0),"roughnessFactor":material.roughness.clamp(0.0,1.0)},"doubleSided":true,"alphaMode":if alpha<1.0{"BLEND"}else{"OPAQUE"}});
        if material.emits_light() {
            mat["emissiveFactor"] = json!(material.emission_color.to_array());
            mat["extensions"] = json!({"KHR_materials_emissive_strength":{"emissiveStrength":material.emission_intensity}});
        }
        let m = materials
            .iter()
            .position(|old| *old == mat)
            .unwrap_or_else(|| {
                let i = materials.len();
                materials.push(mat);
                i
            });
        let mut attributes = json!({"POSITION":base,"NORMAL":base+1});
        if has_uvs { attributes["TEXCOORD_0"] = json!(base + 3); }
        primitives.push(json!({"attributes":attributes,"indices":base+2,"material":m,"mode":4}));
    }
    Ok(primitives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn indexed_solid_export_shares_vertices_and_retains_material_properties() {
        let mut object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.material.color = glam::Vec4::new(0.7, 0.12, 0.03, 0.8);
        object.material.opacity = 0.5;
        object.color = object.material.color;
        object.material.metallic = 0.6;
        object.material.roughness = 0.25;
        object.material.emission_intensity = 2.;
        let mesh = Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X + Vec3::Y],
            vec![[0, 1, 2], [1, 3, 2]],
            vec![object.uuid; 2],
        );
        let materials = materials_for_mesh(&[object.clone()], &mesh).unwrap();
        let page = Page { detail_maps: None,
            name: "Solid".into(),
            root: object.uuid,
            mesh: Arc::new(mesh),
            first: 0,
            count: 2,
            size: 64,
            png: vec![],
            colors: None,
            materials: Some(Arc::new(materials)),
        };
        let bytes = super::super::textured_glb::encode_textured(&[page]).unwrap();
        let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
        assert_eq!(doc["accessors"][0]["count"], 4);
        assert_eq!(doc["accessors"][2]["componentType"], 5123);
        assert_eq!(doc["images"].as_array().unwrap().len(), 0);
        assert!(doc["meshes"][0]["primitives"][0]["attributes"]
            .get("TEXCOORD_0")
            .is_none());
        let m = &doc["materials"][0];
        assert_eq!(m["alphaMode"], "BLEND");
        assert!(
            (m["pbrMetallicRoughness"]["metallicFactor"]
                .as_f64()
                .unwrap()
                - 0.6)
                .abs()
                < 1e-6
        );
        assert_eq!(m["pbrMetallicRoughness"]["roughnessFactor"], 0.25);
        assert_eq!(
            m["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"],
            2.
        );
    }
    #[test]
    fn procedural_and_image_materials_still_require_baking() {
        let mut o = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        let mesh = Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            vec![[0, 1, 2]],
            vec![o.uuid],
        );
        o.material.kind = MaterialKind::Wood;
        assert!(materials_for_mesh(&[o.clone()], &mesh).is_none());
        o.material.kind = MaterialKind::Solid;
        o.material_id = Some(Uuid::new_v4());
        assert!(materials_for_mesh(&[o], &mesh).is_none());
    }

    #[test]
    fn solid_export_keeps_explicit_uv_seams() {
        let object = SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        let mut mesh = Mesh::from_parts(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X + Vec3::Y],
            vec![[0, 1, 2], [1, 3, 2]], vec![object.uuid; 2]);
        mesh.uvs = vec![[[0., 0.], [1., 0.], [0., 1.]],
            [[2., 0.], [2., 1.], [1., 1.]]];
        let materials = materials_for_mesh(&[object.clone()], &mesh).unwrap();
        let page = Page { detail_maps: None, name: "Patch".into(), root: object.uuid,
            mesh: Arc::new(mesh), first: 0, count: 2, size: 64,
            png: vec![], colors: None, materials: Some(Arc::new(materials)) };
        let bytes = super::super::textured_glb::encode_textured(&[page]).unwrap();
        let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
        let primitive = &doc["meshes"][0]["primitives"][0];
        let uv_accessor = primitive["attributes"]["TEXCOORD_0"].as_u64().unwrap() as usize;
        assert_eq!(doc["accessors"][uv_accessor]["count"], 6,
            "the shared positions must split at the UV seam");
    }
}
