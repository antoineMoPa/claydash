use super::geometry::Mesh;
use glam::Vec3;
use serde_json::{json, Value};

pub struct ObjectMesh {
    pub name: String,
    pub mesh: Mesh,
}

fn view(binary: &mut Vec<u8>, views: &mut Vec<Value>, bytes: &[u8], target: u32) -> usize {
    while binary.len() % 4 != 0 { binary.push(0); }
    let offset = binary.len();
    binary.extend_from_slice(bytes);
    let index = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len(),"target":target}));
    index
}

pub fn encode(objects: &[ObjectMesh]) -> Result<Vec<u8>, String> {
    if objects.is_empty() { return Err("There are no meshes to export".into()); }
    let mut binary = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    for object in objects {
        if object.mesh.owners.len() != object.mesh.triangles.len() {
            return Err("Mesh triangle ownership is incomplete".into());
        }
        if object.mesh.triangles.is_empty() { continue; }
        let mut positions = Vec::<[f32; 3]>::with_capacity(object.mesh.triangles.len() * 3);
        let mut normals = Vec::<[f32; 3]>::with_capacity(positions.capacity());
        let mut indices = Vec::<u32>::with_capacity(positions.capacity());
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for i in 0..object.mesh.triangles.len() {
            let triangle = object.mesh.triangle(i);
            if object.mesh.normals[i].iter().any(|normal| !normal.is_finite()
                || normal.length_squared() < 0.5) { continue; }
            for (corner, point) in triangle.into_iter().enumerate() {
                if !point.is_finite() { return Err("Mesh has a non-finite vertex".into()); }
                minimum = minimum.min(point);
                maximum = maximum.max(point);
                positions.push(point.to_array());
                normals.push(object.mesh.normals[i][corner].to_array());
                indices.push((indices.len()) as u32);
            }
        }
        if positions.is_empty() { continue; }
        let p = view(&mut binary, &mut views, bytemuck::cast_slice(&positions), 34962);
        let n = view(&mut binary, &mut views, bytemuck::cast_slice(&normals), 34962);
        let ix = view(&mut binary, &mut views, bytemuck::cast_slice(&indices), 34963);
        let base = accessors.len();
        accessors.push(json!({"bufferView":p,"componentType":5126,"count":positions.len(),"type":"VEC3","min":minimum.to_array(),"max":maximum.to_array()}));
        accessors.push(json!({"bufferView":n,"componentType":5126,"count":normals.len(),"type":"VEC3"}));
        accessors.push(json!({"bufferView":ix,"componentType":5125,"count":indices.len(),"type":"SCALAR"}));
        meshes.push(json!({"name":object.name,"primitives":[{"attributes":{"POSITION":base,"NORMAL":base+1},"indices":base+2,"material":0,"mode":4}]}));
        nodes.push(json!({"name":object.name,"mesh":meshes.len()-1}));
    }
    if meshes.is_empty() { return Err("There are no nonempty meshes to export".into()); }
    let mut document = serde_json::to_vec(&json!({
        "asset":{"version":"2.0","generator":"Claydash Poisson mesh"},
        "extensionsUsed":["KHR_materials_unlit"],"extensionsRequired":["KHR_materials_unlit"],
        "scene":0,"scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],
        "nodes":nodes,"meshes":meshes,
        "materials":[{"name":"Neutral mesh preview","extensions":{"KHR_materials_unlit":{}},
            "pbrMetallicRoughness":{"baseColorFactor":[0.76,0.78,0.81,1.0],"metallicFactor":0.0,"roughnessFactor":1.0},
            "doubleSided":true}],
        "buffers":[{"byteLength":binary.len()}],"bufferViews":views,"accessors":accessors,
    })).map_err(|error| error.to_string())?;
    while document.len() % 4 != 0 { document.push(b' '); }
    while binary.len() % 4 != 0 { binary.push(0); }
    let length = 28usize.checked_add(document.len()).and_then(|value| value.checked_add(binary.len()))
        .filter(|length| *length <= u32::MAX as usize).ok_or("GLB exceeds 4 GiB")?;
    let mut output = Vec::with_capacity(length);
    output.extend_from_slice(b"glTF");
    output.extend_from_slice(&2u32.to_le_bytes());
    output.extend_from_slice(&(length as u32).to_le_bytes());
    output.extend_from_slice(&(document.len() as u32).to_le_bytes());
    output.extend_from_slice(b"JSON");
    output.extend_from_slice(&document);
    output.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    output.extend_from_slice(b"BIN\0");
    output.extend_from_slice(&binary);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glb_contains_one_node_per_requested_root() {
        let mesh = Mesh::from_parts(vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            vec![[0,1,2]], vec![uuid::Uuid::nil()]);
        let bytes = encode(&[ObjectMesh { name: "A".into(), mesh: mesh.clone() },
            ObjectMesh { name: "B".into(), mesh }]).unwrap();
        if let Some(path) = std::env::var_os("CLAYDASH_GLB_TEST_OUTPUT") {
            std::fs::write(path, &bytes).unwrap();
        }
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20+json_len]).unwrap();
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"], 0);
    }
}
