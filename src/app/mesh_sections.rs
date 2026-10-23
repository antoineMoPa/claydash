//! Object-local cross-section templates and connected editable mesh construction.
use crate::model::{self, DataTree};
use crate::renderer::poisson_mesh::geometry::{Mesh, MeshSection, MeshShading};
use glam::{Vec2, Vec3};
use serde_json::{json, Value};
use std::sync::Arc;

struct SectionBuild {
    rings: Vec<Vec<[f32; 3]>>,
    vertices: Vec<[f32; 3]>,
    faces: Vec<[u32; 3]>,
}

fn cross_2d(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab_c = cross_2d(b - a, c - a);
    let ab_d = cross_2d(b - a, d - a);
    let cd_a = cross_2d(d - c, a - c);
    let cd_b = cross_2d(d - c, b - c);
    ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0
}

fn build(sections: &[MeshSection], caps: bool) -> Result<SectionBuild, String> {
    if !(2..=256).contains(&sections.len()) {
        return Err("section loft requires 2–256 ordered sections".into());
    }
    let point_count = sections[0].points.len();
    if !(3..=256).contains(&point_count) {
        return Err("each section requires 3–256 contour points".into());
    }
    let mut rings = Vec::with_capacity(sections.len());
    let mut vertices = Vec::with_capacity(sections.len() * point_count + if caps { 2 } else { 0 });
    let mut winding = 0.0f32;
    for (index, section) in sections.iter().enumerate() {
        if !section.station.is_finite()
            || index > 0 && section.station <= sections[index - 1].station
        {
            return Err(format!(
                "section {index} station must be finite and strictly increasing"
            ));
        }
        if section.points.len() != point_count {
            return Err(format!(
                "section {index} contour point count differs from the first section"
            ));
        }
        let origin = Vec3::from_array(section.origin);
        let right = Vec3::from_array(section.right);
        let up = Vec3::from_array(section.up);
        if !origin.is_finite()
            || !right.is_finite()
            || !up.is_finite()
            || (right.length() - 1.0).abs() > 0.001
            || (up.length() - 1.0).abs() > 0.001
            || right.dot(up).abs() > 0.001
        {
            return Err(format!(
                "section {index} frame needs finite origin and orthonormal unit right/up axes"
            ));
        }
        let mut area = 0.0;
        let mut ring = Vec::with_capacity(point_count);
        for i in 0..point_count {
            let p = Vec2::from_array(section.points[i]);
            let q = Vec2::from_array(section.points[(i + 1) % point_count]);
            if !p.is_finite() || !q.is_finite() || (q - p).length_squared() <= 1e-12 {
                return Err(format!(
                    "section {index} has non-finite or repeated contour points"
                ));
            }
            area += cross_2d(p, q);
            let xyz = origin + right * p.x + up * p.y;
            if !xyz.is_finite() {
                return Err(format!("section {index} vertex exceeds finite range"));
            }
            ring.push(xyz.to_array());
        }
        if area.abs() <= 1e-8 {
            return Err(format!("section {index} contour has zero area"));
        }
        for i in 0..point_count {
            let a = Vec2::from_array(section.points[i]);
            let b = Vec2::from_array(section.points[(i + 1) % point_count]);
            for j in i + 2..point_count {
                if i == 0 && j == point_count - 1 {
                    continue;
                }
                let c = Vec2::from_array(section.points[j]);
                let d = Vec2::from_array(section.points[(j + 1) % point_count]);
                if segments_cross(a, b, c, d) {
                    return Err(format!("section {index} contour crosses itself"));
                }
            }
        }
        if index == 0 {
            winding = area.signum();
        } else if area.signum() != winding {
            return Err(format!("section {index} winding differs from section 0"));
        }
        if caps {
            for i in 0..point_count {
                let a = Vec2::from_array(section.points[i]);
                let b = Vec2::from_array(section.points[(i + 1) % point_count]);
                let c = Vec2::from_array(section.points[(i + 2) % point_count]);
                if cross_2d(b - a, c - b) * winding <= 1e-8 {
                    return Err(format!(
                        "section {index} needs a strictly convex contour for fan caps"
                    ));
                }
            }
        }
        vertices.extend_from_slice(&ring);
        rings.push(ring);
    }
    let first_axis = Vec3::from_array(sections[0].right).cross(Vec3::from_array(sections[0].up));
    for index in 1..sections.len() {
        let previous = Vec3::from_array(sections[index - 1].origin);
        let next = Vec3::from_array(sections[index].origin);
        let axis =
            Vec3::from_array(sections[index].right).cross(Vec3::from_array(sections[index].up));
        if axis.dot(first_axis) < 0.5 || (next - previous).dot(first_axis) <= 1e-6 {
            return Err(format!(
                "section {index} frame reverses or does not advance along the loft axis"
            ));
        }
    }
    let mut faces = Vec::with_capacity(
        (sections.len() - 1) * point_count * 2 + if caps { point_count * 2 } else { 0 },
    );
    for section in 0..sections.len() - 1 {
        for i in 0..point_count {
            let j = (i + 1) % point_count;
            let a = (section * point_count + i) as u32;
            let b = (section * point_count + j) as u32;
            let c = ((section + 1) * point_count + i) as u32;
            let d = ((section + 1) * point_count + j) as u32;
            let side = if winding > 0.0 {
                [[a, b, c], [b, d, c]]
            } else {
                [[a, c, b], [b, c, d]]
            };
            for face in side {
                let [p, q, r] = face.map(|v| Vec3::from_array(vertices[v as usize]));
                if (q - p).cross(r - p).length_squared() <= 1e-16 {
                    return Err(format!(
                        "sections {section} and {} produce a degenerate side face",
                        section + 1
                    ));
                }
                faces.push(face);
            }
        }
    }
    if caps {
        for end in [0, sections.len() - 1] {
            let center = rings[end]
                .iter()
                .fold(Vec3::ZERO, |sum, point| sum + Vec3::from_array(*point))
                / point_count as f32;
            let center_index = vertices.len() as u32;
            vertices.push(center.to_array());
            for i in 0..point_count {
                let a = (end * point_count + i) as u32;
                let b = (end * point_count + (i + 1) % point_count) as u32;
                faces.push(if (end == 0) == (winding > 0.0) {
                    [center_index, b, a]
                } else {
                    [center_index, a, b]
                });
            }
        }
    }
    Ok(SectionBuild {
        rings,
        vertices,
        faces,
    })
}

pub(crate) fn preview(sections: Vec<MeshSection>, caps: bool) -> Result<Value, String> {
    let built = build(&sections, caps)?;
    let projections = built
        .rings
        .iter()
        .map(|ring| {
            json!({
                "xy": ring.iter().map(|p| [p[0],p[1]]).collect::<Vec<_>>(),
                "xz": ring.iter().map(|p| [p[0],p[2]]).collect::<Vec<_>>(),
                "yz": ring.iter().map(|p| [p[1],p[2]]).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"sections": sections, "rings": built.rings, "orthographic": projections,
        "vertices": built.vertices, "faces": built.faces, "temporary": true}),
    )
}

pub(crate) fn create(
    tree: &mut DataTree,
    name: Option<String>,
    sections: Vec<MeshSection>,
    caps: bool,
    shading: Option<MeshShading>,
) -> Result<uuid::Uuid, String> {
    let built = build(&sections, caps)?;
    let id = super::mesh_actions::create_mesh(tree, name, built.vertices, built.faces, shading)?;
    let mut objects = model::objects(tree);
    let mesh = Arc::make_mut(
        objects
            .iter_mut()
            .find(|object| object.uuid == id)
            .unwrap()
            .imported_mesh
            .as_mut()
            .unwrap(),
    );
    mesh.sections = sections;
    mesh.section_caps = caps;
    mesh.regenerate_normals();
    model::set_objects(tree, objects);
    Ok(id)
}

pub(crate) fn rebuild(
    tree: &mut DataTree,
    id: uuid::Uuid,
    sections: Vec<MeshSection>,
    caps: bool,
) -> Result<(), String> {
    let built = build(&sections, caps)?;
    let mut objects = model::objects(tree);
    let object = objects
        .iter_mut()
        .find(|object| object.uuid == id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    let mesh = object
        .imported_mesh
        .as_mut()
        .ok_or_else(|| format!("object {id} is not an editable mesh"))?;
    if mesh.sections.is_empty() {
        return Err("mesh has no authored section templates".into());
    }
    let mesh = Arc::make_mut(mesh);
    let mut replacement = Mesh::from_parts(
        built.vertices.into_iter().map(Vec3::from_array).collect(),
        built.faces,
        vec![
            id;
            (sections.len() - 1) * sections[0].points.len() * 2
                + if caps {
                    sections[0].points.len() * 2
                } else {
                    0
                }
        ],
    );
    replacement.set_shading(mesh.shading);
    replacement.revision = mesh.revision.wrapping_add(1);
    replacement.sections = sections;
    replacement.section_caps = caps;
    replacement.regenerate_normals();
    *mesh = replacement;
    model::set_objects(tree, objects);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        agent::{Action, ApplyArgs, Request},
        App,
    };
    fn section(station: f32, radius: f32) -> MeshSection {
        MeshSection {
            station,
            origin: [station, 0., 0.],
            right: [0., 1., 0.],
            up: [0., 0., 1.],
            points: vec![[radius, 0.], [0., radius], [-radius, 0.], [0., -radius]],
        }
    }
    #[test]
    fn smooth_loft_keeps_flat_caps_after_create_rebuild_and_shading_change() {
        let mut tree = DataTree::default();
        let sections = vec![section(0., 1.), section(1., 1.)];
        let id = create(&mut tree, None, sections.clone(), true, Some(MeshShading::Smooth))
            .unwrap();
        let check = |tree: &DataTree| {
            let mesh = model::objects_ref(tree)[0].imported_mesh.as_ref().unwrap();
            let side_faces = (mesh.sections.len() - 1) * mesh.sections[0].points.len() * 2;
            for normals in &mesh.normals[..side_faces] {
                for normal in normals {
                    assert!(normal.x.abs() < 0.01, "side normal blends with cap: {normal:?}");
                }
            }
            for (face, normals) in mesh.normals[side_faces..].iter().enumerate() {
                let expected = if face < mesh.sections[0].points.len() { -1. } else { 1. };
                for normal in normals {
                    assert!((normal.x - expected).abs() < 0.01 && normal.y.abs() < 0.01
                        && normal.z.abs() < 0.01, "cap normal is not planar: {normal:?}");
                }
            }
        };
        check(&tree);
        let restored: Mesh = serde_json::from_slice(&serde_json::to_vec(
            model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().as_ref()
        ).unwrap()).unwrap();
        assert_eq!(restored.normals.len(), 16);
        let mut edited = sections;
        edited[1].origin[0] = 2.;
        edited[1].station = 2.;
        rebuild(&mut tree, id, edited, true).unwrap();
        check(&tree);
        super::super::mesh_actions::set_shading(&mut tree, id, MeshShading::Auto).unwrap();
        check(&tree);
        super::super::mesh_actions::set_shading(&mut tree, id, MeshShading::Smooth).unwrap();
        check(&tree);
    }
    #[test]
    fn section_preview_is_temporary_and_loft_is_closed_with_caps() {
        use std::collections::HashMap;
        let sections = vec![section(0., 1.), section(1., 0.5), section(2., 0.2)];
        let preview = preview(sections.clone(), true).unwrap();
        assert_eq!(preview["rings"].as_array().unwrap().len(), 3);
        assert_eq!(preview["orthographic"][0]["yz"][0], json!([1., 0.]));
        assert_eq!(preview["faces"].as_array().unwrap().len(), 24);
        let mut tree = DataTree::default();
        let id = create(
            &mut tree,
            None,
            sections.clone(),
            true,
            Some(MeshShading::Smooth),
        )
        .unwrap();
        let mesh = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(mesh.triangles.len(), 24);
        let mut edges = HashMap::<(u32, u32), usize>::new();
        let mut signed_volume = 0.0f32;
        for face in &mesh.triangles {
            for (a, b) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
            let [a, b, c] = face.map(|i| mesh.positions[i as usize]);
            signed_volume += a.dot(b.cross(c)) / 6.0;
        }
        assert!(edges.values().all(|count| *count == 2));
        assert!(
            signed_volume > 0.0,
            "capped loft winding must enclose positive volume"
        );
        assert_eq!(mesh.sections, sections);
        assert!(mesh.section_caps);
        let serialized = serde_json::to_vec(mesh).unwrap();
        let restored: Mesh = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored.sections, sections);
        assert!(restored.section_caps);
        let old = mesh.revision;
        let mut edited = sections;
        edited[1].points[0][0] = 0.7;
        rebuild(&mut tree, id, edited, true).unwrap();
        assert!(
            model::objects_ref(&tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .revision
                > old
        );
    }
    #[test]
    fn rejects_bad_order_count_winding_and_frames_without_edit() {
        let good = vec![section(0., 1.), section(1., 1.)];
        let mut tree = DataTree::default();
        let id = create(&mut tree, None, good.clone(), false, None).unwrap();
        let before = model::objects_ref(&tree)[0]
            .imported_mesh
            .as_ref()
            .unwrap()
            .triangles
            .clone();
        let mut bad = good.clone();
        bad[1].station = 0.;
        assert!(rebuild(&mut tree, id, bad, false)
            .unwrap_err()
            .contains("station"));
        let mut bad = good.clone();
        bad[1].points.pop();
        assert!(preview(bad, false).unwrap_err().contains("count"));
        let mut bad = good.clone();
        bad[1].points.reverse();
        assert!(preview(bad, false).unwrap_err().contains("winding"));
        let mut bad = good;
        bad[1].up = [0., 1., 0.];
        assert!(preview(bad, false).unwrap_err().contains("frame"));
        let mut crossed = vec![section(0., 1.), section(1., 1.)];
        crossed[0].points = vec![[1., 0.], [-1., 1.], [0., -1.], [1., 1.], [-1., 0.]];
        crossed[1].points = crossed[0].points.clone();
        assert!(preview(crossed, false).unwrap_err().contains("crosses"));
        assert_eq!(
            model::objects_ref(&tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .triangles,
            before
        );
    }

    #[test]
    fn preview_does_not_mutate_and_apply_section_edits_are_undoable() {
        let mut app = App::new();
        app.new_document();
        let sections = vec![section(0., 1.), section(1., 0.7)];
        let before = app.scene_revision();
        let request: Request = serde_json::from_value(json!({"op":"PreviewMeshSections",
            "args":{"sections":sections,"caps":true}}))
        .unwrap();
        let Request::PreviewMeshSections {
            sections: preview_sections,
            caps,
        } = request
        else {
            panic!("preview request did not decode")
        };
        let preview = preview(preview_sections, caps).unwrap();
        assert_eq!(preview["temporary"], true);
        assert_eq!(app.scene_revision(), before);
        let result = app
            .agent_apply(ApplyArgs {
                expected_revision: Some(before),
                actions: vec![Action::CreateSectionMesh {
                    name: Some("Hull".into()),
                    sections: sections.clone(),
                    caps: true,
                    shading: Some(MeshShading::Smooth),
                }],
            })
            .unwrap();
        let id: uuid::Uuid = serde_json::from_value(result["created_ids"][0].clone()).unwrap();
        assert_eq!(
            model::objects_ref(&app.tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .sections,
            sections
        );
        let mut edited = sections;
        edited[1].points[0][0] = 0.8;
        app.agent_apply(ApplyArgs {
            expected_revision: Some(app.scene_revision()),
            actions: vec![Action::SetMeshSections {
                id,
                sections: edited,
                caps: true,
            }],
        })
        .unwrap();
        assert_eq!(
            model::objects_ref(&app.tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .sections[1]
                .points[0][0],
            0.8
        );
        app.tree.undo();
        assert_eq!(
            model::objects_ref(&app.tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .sections[1]
                .points[0][0],
            0.7
        );
    }
}
