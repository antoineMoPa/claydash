use std::{collections::HashMap, sync::Arc};

use glam::Vec3;
use serde::Deserialize;

use crate::{model, renderer::poisson_mesh::geometry::{baked_triangle_uv, Mesh}};

/// Which coordinate of a world-space plane to test.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PlaneAxis { X, Y, Z }

impl PlaneAxis {
    fn component(self, point: Vec3) -> f32 {
        match self { Self::X => point.x, Self::Y => point.y, Self::Z => point.z }
    }
    fn put(self, point: &mut Vec3, value: f32) {
        match self { Self::X => point.x = value, Self::Y => point.y = value, Self::Z => point.z = value }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PlaneSide { Positive, Negative }

impl PlaneSide {
    fn signed_distance(self, coordinate: f32, offset: f32) -> f32 {
        match self { Self::Positive => coordinate - offset, Self::Negative => offset - coordinate }
    }
}

#[derive(Clone, Copy)]
struct Corner {
    vertex: u32,
    generated: bool,
    uv: [f32; 2],
    normal: Vec3,
}

fn intersection(
    a: Corner, b: Corner, da: f32, db: f32, axis: PlaneAxis, offset: f32,
    source: &Mesh, inverse: glam::Mat4, output: &mut Mesh,
    edge_vertices: &mut HashMap<(u32, u32), u32>,
) -> Corner {
    if da == 0.0 { return a; }
    if db == 0.0 { return b; }
    let t = da / (da - db);
    let key = (a.vertex.min(b.vertex), a.vertex.max(b.vertex));
    let vertex = *edge_vertices.entry(key).or_insert_with(|| {
        let local = source.positions[a.vertex as usize].lerp(source.positions[b.vertex as usize], t);
        // Project in world space before returning to object-local coordinates.
        // This keeps a transformed object's cut on the requested world plane.
        let mut world = inverse.inverse().transform_point3(local);
        axis.put(&mut world, offset);
        let index = output.positions.len() as u32;
        output.positions.push(inverse.transform_point3(world));
        if !source.colors.is_empty() {
            let ca = source.colors[a.vertex as usize];
            let cb = source.colors[b.vertex as usize];
            output.colors.push(std::array::from_fn(|i| ca[i] + (cb[i] - ca[i]) * t));
        }
        index
    });
    let n = a.normal.lerp(b.normal, t);
    let normal = if n.length_squared() > 1e-20 { n / n.length() } else { a.normal };
    Corner { vertex, generated: true, uv: std::array::from_fn(|i| a.uv[i] + (b.uv[i] - a.uv[i]) * t), normal }
}

/// Clip an imported mesh by a world-space axis plane. The result is intentionally
/// open along the new boundary, ready for a symmetry modifier.
pub(crate) fn clip_mesh_plane(
    draft: &mut model::DataTree, id: uuid::Uuid, expected_mesh_revision: Option<u64>,
    axis: PlaneAxis, offset: f32, keep: PlaneSide,
) -> Result<(), String> {
    if !offset.is_finite() { return Err("plane offset must be finite".into()); }
    if model::scene_seams(draft).seams.iter().flat_map(|s| &s.points).any(|p|
        matches!(p, model::SeamPoint::Surface { object, .. } if *object == id)) {
        return Err("mesh has surface-attached seam points; detach them before clipping".into());
    }
    let mut objects = model::objects(draft);
    let object_index = objects.iter().position(|o| o.uuid == id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    if objects.iter().any(|o| o.boolean_parent == Some(id)) {
        return Err("mesh has boolean children; detach them before clipping".into());
    }
    let matrix = model::object_world_matrix(&objects, id);
    let determinant = matrix.determinant();
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return Err("object world transform must be finite and invertible".into());
    }
    let inverse = matrix.inverse();
    let source = objects[object_index].imported_mesh.as_ref()
        .ok_or_else(|| format!("object {id} is not an editable mesh"))?;
    if expected_mesh_revision.is_some_and(|expected| expected != source.revision) {
        return Err(format!("mesh revision conflict: expected {}, current {}", expected_mesh_revision.unwrap(), source.revision));
    }
    if source.triangles.is_empty() { return Err("mesh has no faces to clip".into()); }
    if source.normals.len() != source.triangles.len()
        || (!source.uvs.is_empty() && source.uvs.len() != source.triangles.len())
        || (!source.colors.is_empty() && source.colors.len() != source.positions.len())
        || source.triangles.iter().flatten().any(|&v| v as usize >= source.positions.len()) {
        return Err("source mesh has inconsistent geometry or attributes".into());
    }
    let mut output = Mesh::from_parts(Vec::new(), Vec::new(), Vec::new());
    output.shading = source.shading;
    output.baked_texture = source.baked_texture.clone();
    output.revision = source.revision.wrapping_add(1);
    let mut original_vertices = HashMap::<u32, u32>::new();
    let mut edge_vertices = HashMap::<(u32, u32), u32>::new();
    let mut changed = false;
    for (face_index, face) in source.triangles.iter().enumerate() {
        let uv = if !source.uvs.is_empty() { source.uvs[face_index] }
            else if let Some(texture) = &source.baked_texture {
                baked_triangle_uv(face_index, texture.size, source.triangles.len())
            } else { [[0.0; 2]; 3] };
        let mut corners = [Corner { vertex: 0, generated: false, uv: [0.0; 2], normal: Vec3::ZERO }; 3];
        let mut distances = [0.0; 3];
        for i in 0..3 {
            corners[i] = Corner { vertex: face[i], generated: false, uv: uv[i], normal: source.normals[face_index][i] };
            let world = matrix.transform_point3(source.positions[face[i] as usize]);
            if !world.is_finite() { return Err("mesh has non-finite world positions".into()); }
            distances[i] = keep.signed_distance(axis.component(world), offset);
        }
        if distances.iter().all(|d| *d < 0.0) { changed = true; continue; }
        let mut polygon = Vec::with_capacity(4);
        for i in 0..3 {
            let j = (i + 1) % 3;
            let inside_i = distances[i] >= 0.0;
            let inside_j = distances[j] >= 0.0;
            if inside_i { polygon.push(corners[i]); }
            if inside_i != inside_j {
                polygon.push(intersection(corners[i], corners[j], distances[i], distances[j], axis,
                    offset, source, inverse, &mut output, &mut edge_vertices));
                changed = true;
            }
        }
        if polygon.len() < 3 { continue; }
        for i in 1..polygon.len() - 1 {
            let triangle_corners = [polygon[0], polygon[i], polygon[i + 1]];
            let mut mapped = [0; 3];
            for k in 0..3 {
                let c = triangle_corners[k];
                mapped[k] = if c.generated {
                    c.vertex
                } else {
                    *original_vertices.entry(c.vertex).or_insert_with(|| {
                        let index = output.positions.len() as u32;
                        output.positions.push(source.positions[c.vertex as usize]);
                        if !source.colors.is_empty() { output.colors.push(source.colors[c.vertex as usize]); }
                        index
                    })
                };
            }
            let a = output.positions[mapped[0] as usize];
            let b = output.positions[mapped[1] as usize];
            let c = output.positions[mapped[2] as usize];
            if mapped[0] == mapped[1] || mapped[1] == mapped[2] || mapped[2] == mapped[0]
                || (b - a).cross(c - a).length_squared() <= 1e-20 { continue; }
            output.triangles.push(mapped);
            output.owners.push(id);
            output.normals.push(triangle_corners.map(|c| c.normal));
            if !source.uvs.is_empty() || source.baked_texture.is_some() {
                output.uvs.push(triangle_corners.map(|c| c.uv));
            }
            if output.triangles.len() > 10_000_000 || output.positions.len() > 10_000_000 {
                return Err("clipped mesh exceeds the 10,000,000 vertex or face limit".into());
            }
        }
    }
    if !changed { return Err("plane does not remove or split any mesh faces".into()); }
    if output.triangles.is_empty() { return Err("plane would remove all mesh faces".into()); }
    objects[object_index].imported_mesh = Some(Arc::new(output));
    objects[object_index].saved_mesh = None;
    objects[object_index].saved_group_capture = None;
    objects[object_index].saved_neural_field = None;
    model::set_objects(draft, objects);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, agent::{Action, ApplyArgs}};
    use crate::app::mesh_actions::create_mesh;
    use crate::renderer::poisson_mesh::geometry::{BakedTexture, MeshShading};

    fn crossing_mesh() -> (model::DataTree, uuid::Uuid) {
        let mut tree = model::DataTree::default();
        let id = create_mesh(&mut tree, Some("crossing".into()),
            vec![[-1., 0., 0.], [1., 0., 0.], [1., 1., 0.], [-1., 1., 0.]],
            vec![[0, 1, 2], [0, 2, 3]], Some(MeshShading::Smooth)).unwrap();
        let mut objects = model::objects(&tree);
        let mesh = Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.colors = vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        mesh.uvs = vec![[[0., 0.], [1., 0.], [1., 1.]], [[0., 0.], [1., 1.], [0., 1.]]];
        mesh.baked_texture = Some(BakedTexture { size: 64, png: vec![1, 2, 3] });
        objects[0].transform.translation.x = 2.;
        model::set_objects(&mut tree, objects);
        (tree, id)
    }

    #[test]
    fn cuts_crossing_faces_with_welded_world_boundary_and_attributes() {
        let (mut tree, id) = crossing_mesh();
        let revision = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().revision;
        clip_mesh_plane(&mut tree, id, Some(revision), PlaneAxis::X, 2., PlaneSide::Positive).unwrap();
        let objects = model::objects_ref(&tree);
        let mesh = objects[0].imported_mesh.as_ref().unwrap();
        assert_eq!(mesh.revision, revision + 1);
        assert_eq!(mesh.triangles.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        assert_eq!(mesh.normals.len(), 3);
        assert_eq!(mesh.colors.len(), mesh.positions.len());
        assert_eq!(mesh.baked_texture.as_ref().unwrap().png, vec![1, 2, 3]);
        let world = model::object_world_matrix(objects, id);
        let boundary = mesh.positions.iter().enumerate().filter(|(_, p)|
            (world.transform_point3(**p).x - 2.).abs() < 1e-6).collect::<Vec<_>>();
        assert_eq!(boundary.len(), 3, "shared source edge gets one clipped vertex");
        assert!(mesh.positions.iter().all(|p| world.transform_point3(*p).x >= 2. - 1e-6));
        assert!(mesh.colors.iter().any(|c| (c[0] - 0.5).abs() < 1e-6));
        assert!(mesh.uvs.iter().flatten().any(|uv| (uv[0] - 0.5).abs() < 1e-6));
    }

    #[test]
    fn invalid_and_empty_cuts_leave_scene_unchanged() {
        let (mut tree, id) = crossing_mesh();
        let before = model::objects(&tree);
        let revision = before[0].imported_mesh.as_ref().unwrap().revision;
        assert!(clip_mesh_plane(&mut tree, id, Some(revision + 1), PlaneAxis::X, 2., PlaneSide::Positive)
            .unwrap_err().contains("revision conflict"));
        assert!(clip_mesh_plane(&mut tree, id, None, PlaneAxis::X, f32::NAN, PlaneSide::Positive).is_err());
        assert!(clip_mesh_plane(&mut tree, id, None, PlaneAxis::X, 20., PlaneSide::Positive)
            .unwrap_err().contains("remove all"));
        let after = model::objects(&tree);
        assert_eq!(before[0].imported_mesh.as_ref().unwrap().revision,
            after[0].imported_mesh.as_ref().unwrap().revision);
        assert_eq!(before[0].imported_mesh.as_ref().unwrap().triangles,
            after[0].imported_mesh.as_ref().unwrap().triangles);
    }

    #[test]
    fn negative_half_clip_then_mirror_has_visible_export_geometry() {
        let (mut tree, id) = crossing_mesh();
        let mut objects = model::objects(&tree);
        objects[0].transform.translation.x = 0.0;
        model::set_objects(&mut tree, objects);
        let revision = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().revision;
        clip_mesh_plane(&mut tree, id, Some(revision), PlaneAxis::X, 0.0, PlaneSide::Negative).unwrap();
        let mut objects = model::objects(&tree);
        objects[0].mirror = Some(model::Mirror { axes: [true, false, false] });
        let exported = crate::renderer::poisson_mesh::imported_mesh_world(
            &objects, &objects[0], &std::sync::atomic::AtomicBool::new(false),
        ).unwrap().unwrap();
        assert!(!exported.triangles.is_empty());
        let minimum = exported.positions.iter().map(|point| point.x).fold(f32::INFINITY, f32::min);
        let maximum = exported.positions.iter().map(|point| point.x).fold(f32::NEG_INFINITY, f32::max);
        assert!((minimum + 1.0).abs() < 1e-5 && (maximum - 1.0).abs() < 1e-5);
    }

    #[test]
    fn agent_clip_is_undoable() {
        let mut app = App::new();
        app.new_document();
        let id = create_mesh(&mut app.tree, Some("undo".into()),
            vec![[-1., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            vec![[0, 1, 2]], None).unwrap();
        app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![
            Action::ClipMeshPlane { id, expected_mesh_revision: Some(0),
                axis: PlaneAxis::X, offset: 0., keep: PlaneSide::Positive }
        ]}).unwrap();
        assert_eq!(model::objects_ref(&app.tree)[0].imported_mesh.as_ref().unwrap().revision, 1);
        app.tree.undo();
        let mesh = model::objects_ref(&app.tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(mesh.revision, 0);
        assert_eq!(mesh.positions.len(), 3);
    }

    #[test]
    fn focused_mirror_action_updates_imported_object_and_undoes() {
        let mut app = App::new();
        app.new_document();
        let id = create_mesh(&mut app.tree, Some("mirror".into()),
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            vec![[0, 1, 2]], None).unwrap();
        app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![
            Action::SetObjectMirror { id, mirror: Some(model::Mirror { axes: [true, false, false] }) }
        ]}).unwrap();
        assert_eq!(model::objects_ref(&app.tree)[0].mirror.unwrap().axes, [true, false, false]);
        app.tree.undo();
        assert!(model::objects_ref(&app.tree)[0].mirror.is_none());
    }
}
