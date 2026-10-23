use glam::Vec3;
use base64::Engine as _;
use serde_json::json;
use crate::model::{self, GroupRenderRepresentation, PrimitiveKind, SdfObject};
use crate::renderer::poisson_mesh::geometry::{BakedTexture, Mesh, MeshShading};
use std::sync::Arc;

type AgentResult = Result<serde_json::Value, String>;

type MeshResult = Result<(), String>;

fn check_vertices(vertices: &[[f32; 3]]) -> MeshResult {
    if vertices.iter().flatten().any(|value| !value.is_finite()) {
        return Err("mesh vertices must be finite".into());
    }
    Ok(())
}

fn check_faces(vertices: &[Vec3], faces: &[[u32; 3]]) -> MeshResult {
    for (index, &[a, b, c]) in faces.iter().enumerate() {
        if [a, b, c].iter().any(|&corner| corner as usize >= vertices.len()) {
            return Err(format!("mesh face {index} references a missing vertex"));
        }
        if a == b || b == c || c == a {
            return Err(format!("mesh face {index} repeats a vertex"));
        }
        let area = (vertices[b as usize] - vertices[a as usize])
            .cross(vertices[c as usize] - vertices[a as usize]);
        if !area.is_finite() || area.length_squared() <= 1e-20 {
            return Err(format!("mesh face {index} is degenerate"));
        }
    }
    Ok(())
}

fn object_mesh<'a>(objects: &'a mut [SdfObject], id: uuid::Uuid) -> Result<&'a mut Mesh, String> {
    let object = objects.iter_mut().find(|object| object.uuid == id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    let mesh = object.imported_mesh.as_mut()
        .ok_or_else(|| format!("object {id} is not an editable mesh"))?;
    Ok(Arc::make_mut(mesh))
}

pub(crate) fn create_mesh(draft: &mut model::DataTree, name: Option<String>,
    vertices: Vec<[f32; 3]>, faces: Vec<[u32; 3]>, shading: Option<MeshShading>) -> Result<uuid::Uuid, String> {
    check_vertices(&vertices)?;
    if vertices.len() > 10_000_000 || faces.len() > 10_000_000 {
        return Err("mesh exceeds the 10,000,000 vertex or face limit".into());
    }
    let positions = vertices.into_iter().map(Vec3::from_array).collect::<Vec<_>>();
    check_faces(&positions, &faces)?;
    let mut object = SdfObject::create_kind(PrimitiveKind::Box);
    object.name = name.unwrap_or_else(|| "Mesh".into());
    object.render_representation = GroupRenderRepresentation::DirectMesh;
    object.softness = 0.0;
    object.material = model::picked_material(draft);
    object.material_id = model::picked_material_id(draft);
    object.color = object.material.color;
    let id = object.uuid;
    let mut mesh = Mesh::from_parts(positions, faces.clone(), vec![id; faces.len()]);
    if let Some(shading) = shading { mesh.set_shading(shading); }
    object.imported_mesh = Some(Arc::new(mesh));
    let mut objects = model::objects(draft);
    objects.push(object);
    model::set_objects(draft, objects);
    Ok(id)
}

pub(crate) fn add_vertices(draft: &mut model::DataTree, id: uuid::Uuid,
    vertices: Vec<[f32; 3]>) -> MeshResult {
    check_vertices(&vertices)?;
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    if mesh.positions.len().saturating_add(vertices.len()) > 10_000_000 {
        return Err("mesh exceeds the 10,000,000 vertex limit".into());
    }
    mesh.positions.extend(vertices.into_iter().map(Vec3::from_array));
    mesh.revision = mesh.revision.wrapping_add(1);
    model::set_objects(draft, objects);
    Ok(())
}

pub(crate) fn add_faces(draft: &mut model::DataTree, id: uuid::Uuid,
    faces: Vec<[u32; 3]>) -> MeshResult {
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    if mesh.triangles.len().saturating_add(faces.len()) > 10_000_000 {
        return Err("mesh exceeds the 10,000,000 face limit".into());
    }
    check_faces(&mesh.positions, &faces)?;
    mesh.triangles.extend_from_slice(&faces);
    mesh.owners.extend(std::iter::repeat_n(id, faces.len()));
    mesh.regenerate_normals();
    if !mesh.uvs.is_empty() {
        mesh.uvs.extend(std::iter::repeat_n([[0.0; 2]; 3], faces.len()));
    }
    mesh.revision = mesh.revision.wrapping_add(1);
    model::set_objects(draft, objects);
    Ok(())
}

pub(crate) fn set_shading(draft: &mut model::DataTree, id: uuid::Uuid,
    shading: MeshShading) -> MeshResult {
    let mut objects = model::objects(draft);
    object_mesh(&mut objects, id)?.set_shading(shading);
    model::set_objects(draft, objects);
    Ok(())
}

pub(crate) fn project_uvs(draft: &mut model::DataTree, id: uuid::Uuid,
    direction: [f32; 3], up: [f32; 3], scale: [f32; 2], offset: [f32; 2]) -> MeshResult {
    if direction.iter().chain(up.iter()).chain(scale.iter()).chain(offset.iter())
        .any(|value| !value.is_finite()) || scale.contains(&0.0) {
        return Err("UV projection vectors, scale, and offset must be finite; scale must be nonzero".into());
    }
    let direction = Vec3::from_array(direction);
    let up = Vec3::from_array(up);
    let view_length = direction.length();
    if view_length <= 1e-8 { return Err("UV projection direction must be nonzero".into()); }
    let forward = direction / view_length;
    let right = up.cross(forward);
    let right_length = right.length();
    if right_length <= 1e-8 { return Err("UV projection up must not be parallel to direction".into()); }
    let right = right / right_length;
    let vertical = forward.cross(right);
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    let uvs = mesh.triangles.iter().map(|face| face.map(|index| {
        let position = mesh.positions[index as usize];
        [position.dot(right) * scale[0] + offset[0],
            position.dot(vertical) * scale[1] + offset[1]]
    })).collect::<Vec<_>>();
    if uvs.iter().flatten().flatten().any(|coordinate| !coordinate.is_finite()) {
        return Err("UV projection produced non-finite coordinates".into());
    }
    mesh.uvs = uvs;
    mesh.revision = mesh.revision.wrapping_add(1);
    model::set_objects(draft, objects);
    Ok(())
}

pub(crate) fn set_uvs(draft: &mut model::DataTree, id: uuid::Uuid,
    uvs: Vec<[[f32; 2]; 3]>) -> MeshResult {
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    if !uvs.is_empty() && uvs.len() != mesh.triangles.len() {
        return Err("UV data must have one entry per mesh face, or be empty to clear".into());
    }
    if uvs.iter().flatten().flatten().any(|coordinate| !coordinate.is_finite()) {
        return Err("UV coordinates must be finite".into());
    }
    mesh.uvs = uvs;
    mesh.revision = mesh.revision.wrapping_add(1);
    model::set_objects(draft, objects);
    Ok(())
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn set_texture(draft: &mut model::DataTree, id: uuid::Uuid,
    png_base64: String) -> MeshResult {
    if png_base64.len() > 90_000_000 {
        return Err("PNG texture exceeds the 64 MiB encoded limit".into());
    }
    let png = base64::engine::general_purpose::STANDARD.decode(png_base64)
        .map_err(|error| format!("invalid PNG base64: {error}"))?;
    if png.len() > 64 * 1024 * 1024 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("texture must be a PNG of at most 64 MiB".into());
    }
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&png))
        .with_guessed_format().map_err(|error| format!("invalid PNG texture: {error}"))?
        .into_dimensions()
        .map_err(|error| format!("invalid PNG texture: {error}"))?;
    if width != height || !(64..=4096).contains(&width) || !width.is_power_of_two() {
        return Err("PNG texture must be square and power-of-two, 64–4096 pixels".into());
    }
    image::load_from_memory(&png).map_err(|error| format!("invalid PNG texture: {error}"))?;
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    if mesh.uvs.len() != mesh.triangles.len() {
        return Err("set per-corner UVs for every face before attaching a texture".into());
    }
    mesh.baked_texture = Some(BakedTexture { size: width, png });
    mesh.revision = mesh.revision.wrapping_add(1);
    model::set_objects(draft, objects);
    Ok(())
}

pub(crate) fn get_mesh(tree: &model::DataTree, id: uuid::Uuid) -> AgentResult {
    let object = model::objects_ref(tree).iter().find(|object| object.uuid == id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    let mesh = object.imported_mesh.as_ref()
        .ok_or_else(|| format!("object {id} is not an editable mesh"))?;
    Ok(json!({"id": id, "revision": mesh.revision, "vertices": mesh.positions,
        "faces": mesh.triangles, "uvs": mesh.uvs, "shading": mesh.shading,
        "normals": mesh.normals, "sections": mesh.sections, "section_caps": mesh.section_caps,
        "texture": mesh.baked_texture.as_ref().map(|texture| json!({
            "size": texture.size, "png_bytes": texture.png.len()
        }))}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, agent::{Action, ApplyArgs, Request}};

    #[test]
    fn mesh_texture_requires_uvs_and_bounded_square_png() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(&mut tree, None,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], vec![[0, 1, 2]], None).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(64, 64)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        assert!(set_texture(&mut tree, id, encoded.clone()).is_err());
        set_uvs(&mut tree, id, vec![[[0., 0.], [1., 0.], [0., 1.]]]).unwrap();
        set_texture(&mut tree, id, encoded).unwrap();
        assert_eq!(get_mesh(&tree, id).unwrap()["texture"]["size"], 64);
        assert!(set_texture(&mut tree, id, "not a png".into()).is_err());
        assert_eq!(get_mesh(&tree, id).unwrap()["texture"]["size"], 64);
    }

    #[test]
    fn shading_modes_update_shared_corner_normals_and_survive_face_edits() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(&mut tree, Some("Fold".into()),
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            vec![[0, 1, 2], [0, 3, 1]], None).unwrap();
        let original = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_ne!(original.normals[0][0], original.normals[1][0]);

        set_shading(&mut tree, id, MeshShading::Smooth).unwrap();
        let smooth = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(smooth.normals[0][0], smooth.normals[1][0]);
        assert!(smooth.normals[0][0].y > 0.5 && smooth.normals[0][0].z > 0.5);
        assert_eq!(get_mesh(&tree, id).unwrap()["shading"], "Smooth");
        let encoded = serde_json::to_vec(model::objects_ref(&tree)).unwrap();
        let decoded: Vec<SdfObject> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded[0].imported_mesh.as_ref().unwrap().shading, MeshShading::Smooth);

        add_faces(&mut tree, id, vec![[0, 2, 3]]).unwrap();
        let edited = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(edited.shading, MeshShading::Smooth);
        assert_eq!(edited.normals[0][0], edited.normals[2][0]);

        set_shading(&mut tree, id, MeshShading::Flat).unwrap();
        let flat = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(flat.normals[0][0], flat.normals[0][1]);
        assert_ne!(flat.normals[0][0], flat.normals[1][0]);
    }

    #[test]
    fn mesh_actions_build_faces_and_edit_seam_uvs() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(&mut tree, Some("Patch".into()),
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            vec![[0, 1, 2]], None).unwrap();
        add_vertices(&mut tree, id, vec![[1., 1., 0.]]).unwrap();
        add_faces(&mut tree, id, vec![[1, 3, 2]]).unwrap();
        project_uvs(&mut tree, id, [0., 0., 1.], [0., 1., 0.], [2., 3.], [0.5, 0.25]).unwrap();
        let mesh = get_mesh(&tree, id).unwrap();
        assert_eq!(mesh["faces"].as_array().unwrap().len(), 2);
        assert_eq!(mesh["uvs"][0][1], json!([2.5, 0.25]));
        let seam = vec![[[0., 0.], [1., 0.], [0., 1.]],
            [[2., 0.], [2., 1.], [1., 1.]]];
        set_uvs(&mut tree, id, seam.clone()).unwrap();
        assert_eq!(get_mesh(&tree, id).unwrap()["uvs"], json!(seam));
        let encoded = serde_json::to_vec(model::objects_ref(&tree)).unwrap();
        let decoded: Vec<SdfObject> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded[0].imported_mesh.as_ref().unwrap().uvs, seam);
        assert_eq!(decoded[0].imported_mesh.as_ref().unwrap().owners, vec![id; 2]);
    }

    #[test]
    fn invalid_mesh_edits_leave_draft_unchanged() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(&mut tree, None,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], vec![[0, 1, 2]], None).unwrap();
        let before = get_mesh(&tree, id).unwrap();
        assert!(add_faces(&mut tree, id, vec![[0, 1, 99]]).is_err());
        assert!(set_uvs(&mut tree, id, vec![[[0.; 2]; 3]; 2]).is_err());
        assert!(project_uvs(&mut tree, id, [0., 0., 1.], [0., 0., 2.], [1.; 2], [0.; 2]).is_err());
        assert_eq!(get_mesh(&tree, id).unwrap(), before);
    }

    #[test]
    fn apply_mesh_actions_refresh_viewport_source_and_get_mesh() {
        let mut app = App::new();
        app.new_document();
        let created = app.agent_apply(ApplyArgs { expected_revision: None,
            actions: vec![Action::CreateMesh { name: Some("Agent patch".into()),
                vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                faces: vec![], shading: None }] }).unwrap();
        let id: uuid::Uuid = serde_json::from_value(created["created_ids"][0].clone()).unwrap();
        let before_source = crate::app::rendering::mesh_source_revision(model::objects_ref(&app.tree));
        let before_view = crate::app::rendering::scene_render_versions(&app.tree, false);
        app.agent_apply(ApplyArgs { expected_revision: Some(app.scene_revision()),
            actions: vec![Action::AddMeshFaces { id, faces: vec![[0, 1, 2]] },
                Action::SetMeshShading { id, shading: MeshShading::Smooth },
                Action::ProjectMeshUvs { id, direction: [0., 0., 1.], up: [0., 1., 0.],
                    scale: [1., 1.], offset: [0., 0.] }] }).unwrap();
        assert_ne!(crate::app::rendering::mesh_source_revision(model::objects_ref(&app.tree)), before_source);
        assert_ne!(crate::app::rendering::scene_render_versions(&app.tree, false), before_view);
        let request: Request = serde_json::from_value(json!({"op":"GetMesh","args":{"id":id}})).unwrap();
        let Request::GetMesh { id: requested } = request else { panic!("GetMesh did not decode") };
        assert_eq!(requested, id);
        let mesh = get_mesh(&app.tree, requested).unwrap();
        assert_eq!(mesh["faces"][0], json!([0, 1, 2]));
        assert_eq!(mesh["shading"], "Smooth");
        assert_eq!(mesh["uvs"][0], json!([[0., 0.], [1., 0.], [0., 1.]]));
    }
}
