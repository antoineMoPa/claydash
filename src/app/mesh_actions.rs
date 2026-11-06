use glam::Vec3;
use base64::Engine as _;
use serde_json::json;
use crate::model::{self, GroupRenderRepresentation, PrimitiveKind, SdfObject};
use crate::renderer::poisson_mesh::geometry::{BakedTexture, Mesh, MeshShading};
use std::sync::Arc;
use std::collections::HashMap;
use serde::Deserialize;

type AgentResult = Result<serde_json::Value, String>;

type MeshResult = Result<(), String>;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FairMeshRegionArgs {
    pub id: uuid::Uuid,
    pub face_indices: Vec<usize>,
    pub expected_mesh_revision: u64,
    pub max_displacement: f32,
    #[serde(default = "default_fair_iterations")]
    pub iterations: u8,
    #[serde(default = "default_fair_strength")]
    pub strength: f32,
    #[serde(default = "default_crease_angle")]
    pub crease_angle_degrees: f32,
    #[serde(default)]
    pub coincident_position_policy: CoincidentPositionPolicy,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoincidentPositionPolicy {
    #[default]
    Indexed,
    Exact,
}

pub(crate) fn default_fair_iterations() -> u8 { 5 }
pub(crate) fn default_fair_strength() -> f32 { 0.45 }
pub(crate) fn default_crease_angle() -> f32 { 75.0 }

fn fair_mesh(mesh: &Mesh, args: &FairMeshRegionArgs) -> Result<(Vec<Vec3>, serde_json::Value), String> {
    if mesh.positions.len() > 250_000 || mesh.triangles.len() > 250_000 {
        return Err("fairing source exceeds 250,000 vertices or faces; separate a smaller part first".into());
    }
    if matches!(args.coincident_position_policy, CoincidentPositionPolicy::Indexed) {
        let (positions, mut stats) = fair_mesh_indexed(mesh, args)?;
        stats["coincident_position_policy"] = json!("indexed");
        return Ok((positions, stats));
    }
    if mesh.positions.iter().any(|point| !point.is_finite()) {
        return Err("mesh positions must be finite for exact coincident-position fairing".into());
    }
    if mesh.triangles.iter().flatten().any(|&vertex| vertex as usize >= mesh.positions.len()) {
        return Err("mesh has a face referencing a missing vertex".into());
    }
    let mut groups = HashMap::<(u32,u32,u32), u32>::new();
    let mut positions = Vec::new();
    let mut original_to_group = Vec::with_capacity(mesh.positions.len());
    for point in &mesh.positions {
        let key = (
            if point.x == 0.0 { 0 } else { point.x.to_bits() },
            if point.y == 0.0 { 0 } else { point.y.to_bits() },
            if point.z == 0.0 { 0 } else { point.z.to_bits() },
        );
        let group = *groups.entry(key).or_insert_with(|| {
            let index = positions.len() as u32;
            positions.push(*point);
            index
        });
        original_to_group.push(group);
    }
    let mut grouped = mesh.clone();
    grouped.positions = positions;
    grouped.triangles = mesh.triangles.iter().map(|face| face.map(|vertex| {
        original_to_group[vertex as usize]
    })).collect();
    let (result, mut stats) = fair_mesh_indexed(&grouped, args)?;
    let expanded = original_to_group.iter().map(|&group| result[group as usize]).collect::<Vec<_>>();
    stats["coincident_position_policy"] = json!("exact");
    stats["coincident_vertex_copies"] = json!(mesh.positions.len() - grouped.positions.len());
    stats["moved_vertex_copies"] = json!(expanded.iter().zip(&mesh.positions)
        .filter(|(after,before)| after.distance(**before) > 1e-7).count());
    stats["count_unit"] = json!("coincident_position_groups");
    Ok((expanded, stats))
}

fn fair_mesh_indexed(mesh: &Mesh, args: &FairMeshRegionArgs) -> Result<(Vec<Vec3>, serde_json::Value), String> {
    if mesh.revision != args.expected_mesh_revision {
        return Err(format!("mesh revision conflict: expected {}, current {}", args.expected_mesh_revision, mesh.revision));
    }
    if args.face_indices.is_empty() || args.face_indices.len() > 100_000 {
        return Err("face_indices must contain 1–100,000 faces".into());
    }
    if !(1..=20).contains(&args.iterations) || !args.strength.is_finite() || !(0.05..=0.8).contains(&args.strength)
        || !args.max_displacement.is_finite() || args.max_displacement <= 0.0
        || !args.crease_angle_degrees.is_finite() || !(0.0..=180.0).contains(&args.crease_angle_degrees) {
        return Err("iterations must be 1–20, strength 0.05–0.8, max_displacement positive, crease_angle_degrees 0–180".into());
    }
    let mut selected = vec![false; mesh.triangles.len()];
    for &index in &args.face_indices {
        let slot = selected.get_mut(index).ok_or_else(|| format!("face index {index} is out of range"))?;
        if *slot { return Err(format!("face index {index} is repeated")); }
        *slot = true;
    }
    let count = mesh.positions.len();
    let mut neighbors = vec![Vec::<usize>::new(); count];
    let mut incident = vec![Vec::<usize>::new(); count];
    let mut edges = HashMap::<(usize,usize), Vec<usize>>::new();
    let mut normals = Vec::with_capacity(mesh.triangles.len());
    for (fi, face) in mesh.triangles.iter().enumerate() {
        let [a,b,c] = face.map(|v| v as usize);
        if [a,b,c].iter().any(|&v| v >= count) { return Err(format!("face {fi} references a missing vertex")); }
        let area = (mesh.positions[b]-mesh.positions[a]).cross(mesh.positions[c]-mesh.positions[a]);
        if !area.is_finite() || area.length_squared() <= 1e-20 { return Err(format!("face {fi} is degenerate")); }
        normals.push(area.normalize());
        for &v in &[a,b,c] { incident[v].push(fi); }
        for (u,v) in [(a,b),(b,c),(c,a)] {
            neighbors[u].push(v); neighbors[v].push(u);
            edges.entry((u.min(v),u.max(v))).or_default().push(fi);
        }
    }
    for around in &mut neighbors { around.sort_unstable(); around.dedup(); }
    let mut movable = vec![false; count];
    for v in 0..count {
        movable[v] = !incident[v].is_empty() && incident[v].iter().all(|&fi| selected[fi]);
    }
    let selection_boundary_vertices = incident.iter().enumerate()
        .filter(|(v, faces)| !movable[*v] && faces.iter().any(|&fi| selected[fi])).count();
    let cosine = args.crease_angle_degrees.to_radians().cos();
    let mut mesh_edge_pins = vec![false; count];
    let mut crease_pins = vec![false; count];
    for ((a,b), faces) in &edges {
        if faces.len() != 2 {
            mesh_edge_pins[*a] = true; mesh_edge_pins[*b] = true;
        } else if normals[faces[0]].dot(normals[faces[1]]) < cosine {
            crease_pins[*a] = true; crease_pins[*b] = true;
        }
    }
    let selected_mesh_edge_vertices = (0..count).filter(|&v| mesh_edge_pins[v] && incident[v].iter().any(|&fi| selected[fi])).count();
    let selected_crease_vertices = (0..count).filter(|&v| crease_pins[v] && incident[v].iter().any(|&fi| selected[fi])).count();
    for v in 0..count { if mesh_edge_pins[v] || crease_pins[v] { movable[v] = false; } }
    let selected_vertices = incident.iter().filter(|faces| faces.iter().any(|&fi| selected[fi])).count();
    let movable_count = movable.iter().filter(|&&yes| yes).count();
    if movable_count == 0 { return Err("region has no movable interior vertices; select a wider patch or raise crease_angle_degrees".into()); }
    let origin = &mesh.positions;
    let mut positions = origin.clone();
    let roughness = |points: &[Vec3]| -> f32 {
        let mut sum = 0.0;
        for v in 0..count {
            if !movable[v] { continue; }
            let average = neighbors[v].iter().map(|&n| points[n]).sum::<Vec3>() / neighbors[v].len() as f32;
            let residual = points[v] - average;
            sum += residual.length_squared();
        }
        (sum / movable_count as f32).sqrt()
    };
    let before = roughness(origin);
    // Positive relaxation removes short-wave dents. The displacement cap and
    // pinned boundaries keep the local shape change bounded.
    for _ in 0..args.iterations {
        let previous = positions.clone();
        for v in 0..count {
            if !movable[v] { continue; }
            let average = neighbors[v].iter().map(|&n| previous[n]).sum::<Vec3>() / neighbors[v].len() as f32;
            let target = previous[v] + args.strength * (average - previous[v]);
            let offset = target - origin[v];
            positions[v] = origin[v] + offset.clamp_length_max(args.max_displacement);
        }
    }
    let candidate = positions;
    let mut safety_scale = 1.0f32;
    let mut accepted = None;
    for _ in 0..16 {
        let trial = origin.iter().zip(&candidate).map(|(&old,&new)| old + (new-old)*safety_scale).collect::<Vec<_>>();
        let valid = mesh.triangles.iter().all(|face| {
            if !face.iter().any(|&v| movable[v as usize]) { return true; }
            let [a,b,c] = face.map(|v| v as usize);
            let old_area = (origin[b]-origin[a]).cross(origin[c]-origin[a]);
            let new_area = (trial[b]-trial[a]).cross(trial[c]-trial[a]);
            new_area.is_finite() && new_area.length() >= old_area.length() * 0.2
                && old_area.dot(new_area) > 0.0
        });
        if valid && roughness(&trial) < before {
            accepted = Some(trial);
            break;
        }
        safety_scale *= 0.5;
    }
    let positions = accepted.ok_or("fairing cannot reduce local roughness without collapsing or inverting faces; choose another patch or repair its topology")?;
    let mut moved = 0usize;
    let mut maximum = 0.0f32;
    for v in 0..count {
        let distance = positions[v].distance(origin[v]);
        if distance > 1e-7 { moved += 1; maximum = maximum.max(distance); }
    }
    if moved == 0 { return Err("fairing produced no vertex movement".into()); }
    let stats = json!({"id":args.id,"mesh_revision":mesh.revision,"selected_faces":args.face_indices.len(),
        "selected_vertices":selected_vertices,"movable_vertices":movable_count,
        "pinned_vertices":selected_vertices-movable_count,"moved_vertices":moved,
        "selection_boundary_vertices":selection_boundary_vertices,
        "mesh_boundary_or_nonmanifold_vertices":selected_mesh_edge_vertices,
        "crease_vertices":selected_crease_vertices,
        "maximum_displacement":maximum,"safety_scale":safety_scale,
        "roughness_before":before,"roughness_after":roughness(&positions),
        "roughness_metric":"rms_vertex_to_neighbor_average_local_units",
        "space":"object_local_before_modifiers"});
    Ok((positions,stats))
}

pub(crate) fn preview_fair_region(tree: &model::DataTree, args: FairMeshRegionArgs) -> AgentResult {
    if model::scene_seams(tree).seams.iter().flat_map(|seam| &seam.points)
        .any(|point| matches!(point, model::SeamPoint::Surface { object, .. } if *object == args.id)) {
        return Err("mesh has surface-attached seam points; detach them before fairing".into());
    }
    let objects = model::objects_ref(tree);
    let mesh = objects.iter().find(|object| object.uuid == args.id)
        .ok_or_else(|| format!("object {} does not exist", args.id))?
        .imported_mesh.as_ref().ok_or_else(|| format!("object {} is not an editable mesh", args.id))?;
    let (_, mut stats) = fair_mesh(mesh, &args)?;
    stats["preview"] = json!(true);
    Ok(stats)
}

pub(crate) fn fair_region(tree: &mut model::DataTree, args: FairMeshRegionArgs) -> AgentResult {
    if model::scene_seams(tree).seams.iter().flat_map(|seam| &seam.points)
        .any(|point| matches!(point, model::SeamPoint::Surface { object, .. } if *object == args.id)) {
        return Err("mesh has surface-attached seam points; detach them before fairing".into());
    }
    let mut objects = model::objects(tree);
    let mesh = object_mesh(&mut objects, args.id)?;
    let (positions, mut stats) = fair_mesh(mesh, &args)?;
    mesh.positions = positions;
    mesh.regenerate_normals();
    mesh.revision = mesh.revision.wrapping_add(1);
    stats["mesh_revision"] = json!(mesh.revision);
    let object = objects.iter_mut().find(|object| object.uuid == args.id).unwrap();
    object.saved_mesh = None;
    object.saved_group_capture = None;
    object.saved_neural_field = None;
    model::set_objects(tree, objects);
    Ok(stats)
}

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

/// Partition source triangles in their original order. Both outputs get compact local
/// vertex indices; explicit corner attributes are copied, never regenerated.
pub(crate) fn separate_faces(
    draft: &mut model::DataTree,
    id: uuid::Uuid,
    face_indices: Vec<usize>,
    name: Option<String>,
    expected_mesh_revision: Option<u64>,
) -> Result<uuid::Uuid, String> {
    if model::scene_seams(draft)
        .seams
        .iter()
        .flat_map(|seam| &seam.points)
        .any(|point| matches!(point, model::SeamPoint::Surface { object, .. } if *object == id))
    {
        return Err(
            "mesh has surface-attached seam points; detach them before separating faces".into(),
        );
    }
    let mut objects = model::objects(draft);
    let source_index = objects
        .iter()
        .position(|object| object.uuid == id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    let source = objects[source_index]
        .imported_mesh
        .as_ref()
        .ok_or_else(|| format!("object {id} is not an editable mesh"))?;
    if let Some(expected) = expected_mesh_revision {
        if expected != source.revision {
            return Err(format!(
                "mesh revision conflict: expected {expected}, current {}",
                source.revision
            ));
        }
    }
    if objects
        .iter()
        .any(|object| object.boolean_parent == Some(id))
    {
        return Err("mesh has boolean children; detach them before separating faces".into());
    }
    if face_indices.is_empty() {
        return Err("face_indices must not be empty".into());
    }
    let mut selected = vec![false; source.triangles.len()];
    for index in face_indices {
        let slot = selected
            .get_mut(index)
            .ok_or_else(|| format!("mesh face index {index} is out of range"))?;
        if *slot {
            return Err(format!("mesh face index {index} is repeated"));
        }
        *slot = true;
    }
    if selected.iter().all(|selected| *selected) {
        return Err("separation must leave at least one face in the source mesh".into());
    }
    if source.normals.len() != source.triangles.len()
        || (!source.uvs.is_empty() && source.uvs.len() != source.triangles.len())
        || (!source.colors.is_empty() && source.colors.len() != source.positions.len())
        || source
            .triangles
            .iter()
            .flatten()
            .any(|&i| i as usize >= source.positions.len())
    {
        return Err("source mesh has inconsistent geometry or attributes".into());
    }
    let new_id = uuid::Uuid::new_v4();
    let partition = |take_selected: bool, owner: uuid::Uuid| {
        let mut mesh = Mesh::from_parts(Vec::new(), Vec::new(), Vec::new());
        mesh.shading = source.shading;
        mesh.baked_texture = source.baked_texture.clone();
        mesh.revision = source.revision.wrapping_add(1);
        let mut vertices = vec![None; source.positions.len()];
        for (face_index, triangle) in source.triangles.iter().enumerate() {
            if selected[face_index] != take_selected {
                continue;
            }
            mesh.triangles.push(triangle.map(|index| {
                *vertices[index as usize].get_or_insert_with(|| {
                    let mapped = mesh.positions.len() as u32;
                    mesh.positions.push(source.positions[index as usize]);
                    if !source.colors.is_empty() {
                        mesh.colors.push(source.colors[index as usize]);
                    }
                    mapped
                })
            }));
            mesh.owners.push(owner);
            mesh.normals.push(source.normals[face_index]);
            if !source.uvs.is_empty() {
                mesh.uvs.push(source.uvs[face_index]);
            } else if let Some(texture) = &source.baked_texture {
                // Legacy atlases derive UVs from face order. Freeze the original
                // coordinates before partitioning changes that order and count.
                mesh.uvs
                    .push(crate::renderer::poisson_mesh::geometry::baked_triangle_uv(
                        face_index,
                        texture.size,
                        source.triangles.len(),
                    ));
            }
        }
        mesh
    };
    let remainder = partition(false, id);
    let extracted = partition(true, new_id);
    let mut object = objects[source_index].clone();
    object.uuid = new_id;
    object.name = name.unwrap_or_else(|| format!("{} part", object.name));
    object.imported_mesh = Some(Arc::new(extracted));
    object.saved_mesh = None;
    object.saved_group_capture = None;
    object.saved_neural_field = None;
    objects[source_index].imported_mesh = Some(Arc::new(remainder));
    objects[source_index].saved_mesh = None;
    objects[source_index].saved_group_capture = None;
    objects[source_index].saved_neural_field = None;
    objects.push(object);
    model::set_objects(draft, objects);
    Ok(new_id)
}

/// Explicitly discard imported appearance so object materials can replace it.
pub(crate) fn clear_appearance(draft: &mut model::DataTree, id: uuid::Uuid) -> MeshResult {
    let mut objects = model::objects(draft);
    let mesh = object_mesh(&mut objects, id)?;
    mesh.baked_texture = None;
    mesh.colors.clear();
    mesh.uvs.clear();
    mesh.revision = mesh.revision.wrapping_add(1);
    let object = objects.iter_mut().find(|object| object.uuid == id).unwrap();
    object.saved_mesh = None;
    object.saved_group_capture = None;
    object.saved_neural_field = None;
    model::set_objects(draft, objects);
    Ok(())
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
        "normals": mesh.normals,
        "texture": mesh.baked_texture.as_ref().map(|texture| json!({
            "size": texture.size, "png_bytes": texture.png.len()
        }))}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, agent::{Action, ApplyArgs, Request}};

    #[test]
    fn separation_compacts_and_preserves_attributes_and_legacy_atlas() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(
            &mut tree,
            Some("Body".into()),
            vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.],
                [0., 0., 1.],
                [9., 9., 9.],
            ],
            vec![[0, 1, 2], [0, 3, 1]],
            Some(MeshShading::Smooth),
        )
        .unwrap();
        let mut objects = model::objects(&tree);
        objects[0].transform.translation = Vec3::new(2., 3., 4.);
        let mesh = Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.colors = vec![[0.1; 3], [0.2; 3], [0.3; 3], [0.4; 3], [0.5; 3]];
        mesh.baked_texture = Some(BakedTexture {
            size: 64,
            png: vec![1, 2, 3],
        });
        let normals = mesh.normals.clone();
        model::set_objects(&mut tree, objects);
        let new_id = separate_faces(&mut tree, id, vec![1], Some("Glass".into()), None).unwrap();
        let objects = model::objects_ref(&tree);
        assert_eq!(
            objects[0].transform.translation,
            objects[1].transform.translation
        );
        assert_eq!(objects[0].material_id, objects[1].material_id);
        assert_eq!(objects[1].name, "Glass");
        for (i, object) in objects.iter().enumerate() {
            let mesh = object.imported_mesh.as_ref().unwrap();
            assert_eq!(mesh.positions.len(), 3);
            assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
            assert_eq!(mesh.normals, vec![normals[i]]);
            assert_eq!(mesh.owners, vec![object.uuid]);
            assert_eq!(
                mesh.uvs,
                vec![crate::renderer::poisson_mesh::geometry::baked_triangle_uv(
                    i, 64, 2
                )]
            );
            assert_eq!(mesh.baked_texture.as_ref().unwrap().png, vec![1, 2, 3]);
        }
        assert_eq!(
            objects[1].imported_mesh.as_ref().unwrap().colors,
            vec![[0.1; 3], [0.4; 3], [0.2; 3]]
        );
        let mut objects = model::objects(&tree);
        let cached_mesh = objects[1].imported_mesh.as_ref().unwrap().clone();
        objects[1].saved_mesh = Some(Arc::new(model::SavedMesh {
            version: 1,
            source_revision: 0,
            representation: GroupRenderRepresentation::DirectMesh,
            sample_resolution: 64,
            mesh_resolution: 64,
            baked_frame_inverse: glam::Mat4::IDENTITY,
            mesh: cached_mesh,
        }));
        model::set_objects(&mut tree, objects);
        clear_appearance(&mut tree, new_id).unwrap();
        let objects = model::objects_ref(&tree);
        let mesh = objects[1].imported_mesh.as_ref().unwrap();
        assert!(mesh.colors.is_empty() && mesh.uvs.is_empty() && mesh.baked_texture.is_none());
        assert!(objects[1].saved_mesh.is_none());
        assert_eq!(mesh.normals, vec![normals[1]]);
        assert!(objects[0]
            .imported_mesh
            .as_ref()
            .unwrap()
            .baked_texture
            .is_some());
    }

    #[test]
    fn separation_rejects_stale_indices_and_attached_seams() {
        let mut tree = model::DataTree::default();
        let id = create_mesh(
            &mut tree,
            None,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            vec![[0, 1, 2], [0, 3, 1]],
            None,
        )
        .unwrap();
        let before = get_mesh(&tree, id).unwrap();
        let revision = before["revision"].as_u64().unwrap();
        assert!(
            separate_faces(&mut tree, id, vec![1], None, Some(revision + 1))
                .unwrap_err()
                .contains("revision conflict")
        );
        let seams = model::SceneSeams {
            seams: vec![model::Seam {
                id: uuid::Uuid::new_v4(),
                name: "Edge".into(),
                closed: false,
                points: vec![model::SeamPoint::Surface {
                    object: id,
                    triangle: 1,
                    barycentric: Vec3::X,
                }],
            }],
            bindings: vec![],
        };
        tree.set_path("scene.seams", model::ClaydashValue::Seams(seams));
        assert!(separate_faces(&mut tree, id, vec![1], None, Some(revision))
            .unwrap_err()
            .contains("surface-attached"));
        assert_eq!(get_mesh(&tree, id).unwrap(), before);
        assert_eq!(model::objects_ref(&tree).len(), 1);
    }

    #[test]
    fn separation_apply_is_atomic_and_undoable() {
        let mut app = App::new();
        app.new_document();
        let created = app
            .agent_apply(ApplyArgs {
                expected_revision: None,
                actions: vec![Action::CreateMesh {
                    name: None,
                    vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                    faces: vec![[0, 1, 2], [0, 3, 1]],
                    shading: None,
                }],
            })
            .unwrap();
        let id: uuid::Uuid = serde_json::from_value(created["created_ids"][0].clone()).unwrap();
        let before = serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
        for face_indices in [vec![], vec![2], vec![0, 0], vec![0, 1]] {
            assert!(app
                .agent_apply(ApplyArgs {
                    expected_revision: None,
                    actions: vec![
                        Action::ClearMeshAppearance { id },
                        Action::SeparateMeshFaces {
                            id,
                            face_indices,
                            name: None,
                            expected_mesh_revision: None
                        }
                    ]
                })
                .is_err());
            assert_eq!(
                serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),
                before
            );
        }
        let response = app
            .agent_apply(ApplyArgs {
                expected_revision: None,
                actions: vec![Action::SeparateMeshFaces {
                    id,
                    face_indices: vec![1],
                    name: None,
                    expected_mesh_revision: None,
                }],
            })
            .unwrap();
        assert_eq!(response["created_ids"].as_array().unwrap().len(), 1);
        assert_eq!(model::objects_ref(&app.tree).len(), 2);
        app.tree.undo();
        assert_eq!(
            serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),
            before
        );
    }

    #[test]
    fn separating_newly_imported_mesh_keeps_import_as_separate_undo_step() {
        // Two triangles in a self-contained GLB keep this regression independent
        // of model files supplied to an individual task.
        let mut geometry = Vec::new();
        for position in [[0.0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.],
            [0., 0., 1.], [1., 0., 1.], [0., 1., 1.]] {
            for component in position { geometry.extend_from_slice(&component.to_le_bytes()); }
        }
        let description = serde_json::json!({
            "asset":{"version":"2.0"}, "scene":0, "scenes":[{"nodes":[0]}],
            "nodes":[{"mesh":0}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
            "buffers":[{"byteLength":geometry.len()}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":geometry.len()}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":6,
                "type":"VEC3","min":[0,0,0],"max":[1,1,1]}]
        });
        let mut json = serde_json::to_vec(&description).unwrap();
        while json.len() % 4 != 0 { json.push(b' '); }
        let total = 12 + 8 + json.len() + 8 + geometry.len();
        let mut glb = Vec::with_capacity(total);
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&(geometry.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&geometry);

        let mut app = App::new();
        app.new_document();
        let ids = app.import_glb("source", &glb).unwrap();
        assert_eq!(ids.len(), 1);
        let original_faces = model::objects_ref(&app.tree)[0].imported_mesh.as_ref().unwrap().triangles.len();
        assert_eq!(original_faces, 2);
        let revision = model::objects_ref(&app.tree)[0].imported_mesh.as_ref().unwrap().revision;
        app.agent_apply(ApplyArgs { expected_revision: None, actions: vec![
            Action::SeparateMeshFaces { id: ids[0], face_indices: vec![0],
                name: None, expected_mesh_revision: Some(revision) }
        ]}).unwrap();
        assert_eq!(model::objects_ref(&app.tree).len(), 2);
        app.agent_apply(ApplyArgs { expected_revision: None,
            actions: vec![Action::ClearMeshAppearance { id: ids[0] }] }).unwrap();
        app.tree.undo();
        assert_eq!(model::objects_ref(&app.tree).len(), 2);
        app.tree.undo();
        let objects = model::objects_ref(&app.tree);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].uuid, ids[0]);
        assert_eq!(objects[0].imported_mesh.as_ref().unwrap().triangles.len(), original_faces);
    }

    #[test]
    fn put_object_rejects_malformed_imported_mesh_before_commit() {
        let mut app = App::new();
        app.new_document();
        let created = app
            .agent_apply(ApplyArgs {
                expected_revision: None,
                actions: vec![Action::CreateMesh {
                    name: None,
                    vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                    faces: vec![[0, 1, 2]],
                    shading: None,
                }],
            })
            .unwrap();
        let id: uuid::Uuid = serde_json::from_value(created["created_ids"][0].clone()).unwrap();
        let original = model::objects_ref(&app.tree)[0].clone();
        let before = serde_json::to_value(model::objects_ref(&app.tree)).unwrap();
        for case in 0..5 {
            let mut object = original.clone();
            let mesh = Arc::make_mut(object.imported_mesh.as_mut().unwrap());
            match case {
                0 => mesh.owners.clear(),
                1 => mesh.normals.clear(),
                2 => mesh.uvs = vec![[[0.; 2]; 3]; 2],
                3 => mesh.colors = vec![[1.; 3]; 2],
                _ => mesh.triangles[0][0] = 100,
            }
            assert!(app
                .agent_apply(ApplyArgs {
                    expected_revision: None,
                    actions: vec![Action::PutObject { object }]
                })
                .unwrap_err()
                .contains("inconsistent"));
            assert_eq!(
                serde_json::to_value(model::objects_ref(&app.tree)).unwrap(),
                before
            );
        }
        assert_eq!(model::objects_ref(&app.tree)[0].uuid, id);
    }

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

    #[test]
    fn fairing_reduces_interior_dent_and_preserves_boundary_and_revision_preview() {
        let mut tree = model::DataTree::default();
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                vertices.push([x as f32, y as f32, if x == 2 && y == 2 { 0.2 } else { 0.0 }]);
            }
        }
        for y in 0..4 {
            for x in 0..4 {
                let a = (y * 5 + x) as u32;
                faces.extend([[a,a+1,a+5],[a+1,a+6,a+5]]);
            }
        }
        let id = create_mesh(&mut tree, None, vertices, faces.clone(), None).unwrap();
        let args = FairMeshRegionArgs { id, face_indices: (0..faces.len()).collect(),
            expected_mesh_revision: 0, max_displacement: 0.08, iterations: 5,
            strength: 0.45, crease_angle_degrees: 75.0,
            coincident_position_policy: CoincidentPositionPolicy::Indexed };
        let source = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().clone();
        let preview = preview_fair_region(&tree, args.clone()).unwrap();
        assert_eq!(preview["preview"], true);
        assert!(preview["roughness_after"].as_f64().unwrap() < preview["roughness_before"].as_f64().unwrap());
        assert_eq!(model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().revision, 0);
        let result = fair_region(&mut tree, args.clone()).unwrap();
        let edited = model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap();
        assert_eq!(edited.revision, 1);
        assert!(edited.positions[12].z < source.positions[12].z);
        assert!(result["maximum_displacement"].as_f64().unwrap() <= 0.080001);
        for v in 0..25 {
            if v < 5 || v >= 20 || v % 5 == 0 || v % 5 == 4 {
                assert_eq!(edited.positions[v], source.positions[v]);
            }
        }
        assert!(fair_region(&mut tree, args).is_err());
    }

    #[test]
    fn fairing_pins_selected_patch_edge_and_rejects_duplicate_faces() {
        let mut tree = model::DataTree::default();
        let vertices = (0..5).flat_map(|y| (0..5).map(move |x| [x as f32,y as f32,0.0])).collect();
        let mut faces = Vec::new();
        for y in 0..4 { for x in 0..4 {
            let a = (y*5+x) as u32;
            faces.extend([[a,a+1,a+5],[a+1,a+6,a+5]]);
        }}
        let id = create_mesh(&mut tree, None, vertices, faces, None).unwrap();
        let mut args = FairMeshRegionArgs { id, face_indices: vec![10,11],
            expected_mesh_revision: 0, max_displacement: 0.1, iterations: 2,
            strength: 0.4, crease_angle_degrees: 180.0,
            coincident_position_policy: CoincidentPositionPolicy::Indexed };
        assert!(preview_fair_region(&tree, args.clone()).unwrap_err().contains("no movable"));
        args.face_indices = vec![10,10];
        assert!(preview_fair_region(&tree, args).unwrap_err().contains("repeated"));
    }

    #[test]
    fn fairing_preview_is_read_only_and_apply_undo_restores_mesh_appearance() {
        let mut app = App::new();
        app.new_document();
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for y in 0..5 { for x in 0..5 {
            vertices.push([x as f32,y as f32,if x == 2 && y == 2 { 0.15 } else { 0.0 }]);
        }}
        for y in 0..4 { for x in 0..4 {
            let a = (y*5+x) as u32;
            faces.extend([[a,a+1,a+5],[a+1,a+6,a+5]]);
        }}
        let id = create_mesh(&mut app.tree, None, vertices, faces.clone(), None).unwrap();
        let mut objects = model::objects(&app.tree);
        let mesh = Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.colors = vec![[0.3,0.4,0.5]; mesh.positions.len()];
        mesh.uvs = vec![[[0.0,0.0],[1.0,0.0],[0.0,1.0]]; mesh.triangles.len()];
        model::set_objects(&mut app.tree, objects);
        app.tree.make_undo_redo_snapshot();
        let before = get_mesh(&app.tree, id).unwrap();
        let revision = app.scene_revision();
        let args = FairMeshRegionArgs { id, face_indices: (0..faces.len()).collect(),
            expected_mesh_revision: 0, max_displacement: 0.06, iterations: 5,
            strength: 0.45, crease_angle_degrees: 75.0,
            coincident_position_policy: CoincidentPositionPolicy::Indexed };
        let _ = preview_fair_region(&app.tree, args.clone()).unwrap();
        assert_eq!(app.scene_revision(), revision);
        assert_eq!(get_mesh(&app.tree, id).unwrap(), before);
        let applied = app.agent_apply(ApplyArgs { expected_revision: Some(revision),
            actions: vec![Action::FairMeshRegion { id,
                face_indices: args.face_indices, expected_mesh_revision: 0,
                max_displacement: args.max_displacement, iterations: args.iterations,
                strength: args.strength, crease_angle_degrees: args.crease_angle_degrees,
                coincident_position_policy: CoincidentPositionPolicy::Indexed }] }).unwrap();
        assert_eq!(applied["mesh_edits"][0]["moved_vertices"].as_u64().unwrap() > 0, true);
        let after = get_mesh(&app.tree, id).unwrap();
        assert_ne!(after["vertices"], before["vertices"]);
        assert_eq!(after["uvs"], before["uvs"]);
        app.tree.undo();
        assert_eq!(get_mesh(&app.tree, id).unwrap(), before);
    }

    #[test]
    fn exact_coincident_fairing_moves_split_copies_together_without_changing_uvs() {
        let mut tree = model::DataTree::default();
        let grid = (0..5).flat_map(|y| (0..5).map(move |x| {
            [x as f32, y as f32, if x == 2 && y == 2 { 0.2 } else { 0.0 }]
        })).collect::<Vec<_>>();
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for y in 0..4 { for x in 0..4 {
            let a = y*5+x;
            for triangle in [[a,a+1,a+5],[a+1,a+6,a+5]] {
                let start = vertices.len() as u32;
                vertices.extend(triangle.map(|index| grid[index]));
                faces.push([start,start+1,start+2]);
            }
        }}
        let id = create_mesh(&mut tree, None, vertices, faces.clone(), None).unwrap();
        let mut objects = model::objects(&tree);
        let mesh = Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.uvs = vec![[[0.,0.],[1.,0.],[0.,1.]]; faces.len()];
        model::set_objects(&mut tree, objects);
        let before = get_mesh(&tree,id).unwrap();
        let mut args = FairMeshRegionArgs { id, face_indices:(0..faces.len()).collect(),
            expected_mesh_revision:0, max_displacement:0.08, iterations:5,
            strength:0.45, crease_angle_degrees:75.,
            coincident_position_policy:CoincidentPositionPolicy::Indexed };
        assert!(preview_fair_region(&tree,args.clone()).unwrap_err().contains("no movable"));
        args.coincident_position_policy = CoincidentPositionPolicy::Exact;
        let preview = preview_fair_region(&tree,args.clone()).unwrap();
        assert!(preview["movable_vertices"].as_u64().unwrap()>0);
        assert!(preview["coincident_vertex_copies"].as_u64().unwrap()>0);
        fair_region(&mut tree,args).unwrap();
        let after = get_mesh(&tree,id).unwrap();
        assert_eq!(after["uvs"],before["uvs"]);
        assert_eq!(after["faces"],before["faces"]);
        assert!(after["vertices"].as_array().unwrap().iter().filter(|point|
            point[0]==2. && point[1]==2.).all(|point| point[2].as_f64().unwrap()<0.2));
    }

    #[test]
    fn fairing_rejects_oversized_source_before_building_adjacency() {
        let id = uuid::Uuid::new_v4();
        let mesh = Mesh::from_parts(vec![Vec3::ZERO; 250_001], vec![[0,1,2]], vec![id]);
        let args = FairMeshRegionArgs { id, face_indices:vec![0], expected_mesh_revision:0,
            max_displacement:0.01, iterations:1, strength:0.45,
            crease_angle_degrees:75., coincident_position_policy:CoincidentPositionPolicy::Exact };
        assert!(fair_mesh(&mesh,&args).unwrap_err().contains("separate a smaller part first"));
    }
}
