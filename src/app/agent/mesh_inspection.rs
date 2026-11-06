//! Bounded, read-only inspection of source triangles in world space.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectMeshFacesArgs {
    pub id: uuid::Uuid,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub bounds: Option<FaceBounds>,
    #[serde(default)]
    pub detail: FaceDetail,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FaceDetail {
    #[default]
    Geometry,
    Indices,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub mode: FaceBoundsMode,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceBoundsMode {
    Centroid,
    AllVertices,
    BoundsOverlap,
}

impl FaceBounds {
    fn validate(&self) -> Result<(), String> {
        if !self
            .min
            .iter()
            .chain(self.max.iter())
            .all(|v| v.is_finite())
            || (0..3).any(|axis| self.min[axis] > self.max[axis])
        {
            return Err(
                "bounds require finite world coordinates with min <= max on each axis".into(),
            );
        }
        Ok(())
    }

    fn contains(&self, point: Vec3) -> bool {
        point.cmpge(Vec3::from_array(self.min)).all()
            && point.cmple(Vec3::from_array(self.max)).all()
    }

    fn matches(&self, vertices: [Vec3; 3], centroid: Vec3, min: Vec3, max: Vec3) -> bool {
        match self.mode {
            FaceBoundsMode::Centroid => self.contains(centroid),
            FaceBoundsMode::AllVertices => vertices.into_iter().all(|v| self.contains(v)),
            FaceBoundsMode::BoundsOverlap => {
                min.cmple(Vec3::from_array(self.max)).all()
                    && max.cmpge(Vec3::from_array(self.min)).all()
            }
        }
    }
}

pub(super) fn inspect(tree: &model::DataTree, args: InspectMeshFacesArgs) -> AgentResult {
    let maximum = if args.detail == FaceDetail::Indices {
        256
    } else {
        8
    };
    let limit = args.limit.unwrap_or(maximum);
    if !(1..=maximum).contains(&limit) {
        return Err(format!(
            "limit must be between 1 and {maximum} for this detail mode"
        ));
    }
    if let Some(bounds) = &args.bounds {
        bounds.validate()?;
    }
    let objects = model::objects_ref(tree);
    let object = objects
        .iter()
        .find(|o| o.uuid == args.id)
        .ok_or_else(|| format!("object {} does not exist", args.id))?;
    let mesh = object
        .imported_mesh
        .as_ref()
        .ok_or_else(|| format!("object {} is not an editable mesh", args.id))?;
    let offset = args.offset.unwrap_or(0);
    if offset > mesh.triangles.len() {
        return Err("offset exceeds the mesh face count".into());
    }
    let matrix = model::object_world_matrix(objects, args.id);
    let mut faces = Vec::new();
    let mut next = offset;
    let mut bytes = 0;
    // A sparse selection must not monopolize the app thread.
    let scan_end = offset.saturating_add(100_000).min(mesh.triangles.len());
    while next < scan_end && faces.len() < limit {
        let index = next;
        let face = mesh.triangles[index];
        let mut vertices = [Vec3::ZERO; 3];
        for corner in 0..3 {
            let point = mesh
                .positions
                .get(face[corner] as usize)
                .ok_or_else(|| format!("mesh face {index} references a missing vertex"))?;
            vertices[corner] = matrix.transform_point3(*point);
        }
        if vertices.iter().any(|point| !point.is_finite()) {
            return Err(format!(
                "mesh face {index} has non-finite world coordinates"
            ));
        }
        let centroid = vertices[0] / 3.0 + vertices[1] / 3.0 + vertices[2] / 3.0;
        let min = vertices[0].min(vertices[1]).min(vertices[2]);
        let max = vertices[0].max(vertices[1]).max(vertices[2]);
        if args
            .bounds
            .as_ref()
            .is_some_and(|b| !b.matches(vertices, centroid, min, max))
        {
            next += 1;
            continue;
        }
        let item = if args.detail == FaceDetail::Indices {
            json!(index)
        } else {
            json!({"face_index":index,"vertex_indices":face,
            "world_vertices":vertices,"world_centroid":centroid,
            "world_bounds":{"min":min,"max":max}})
        };
        let item_bytes = item.to_string().len();
        // Leave room for response metadata and the MCP wrapper under its 4096-character cap.
        if !faces.is_empty() && bytes + item_bytes > 2800 {
            break;
        }
        faces.push(item);
        bytes += item_bytes;
        next += 1;
    }
    let mut result = json!({"id":args.id,"mesh_revision":mesh.revision,"face_count":mesh.triangles.len(),
        "offset":offset,"next_offset":(next < mesh.triangles.len()).then_some(next),
        "scanned_faces":next-offset,"faces":faces,
        "geometry":"source_mesh_before_modifiers","space":"world"});
    if args.detail == FaceDetail::Indices {
        let indices = result.as_object_mut().unwrap().remove("faces").unwrap();
        result["face_indices"] = indices;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(count: usize) -> (model::DataTree, uuid::Uuid) {
        let mut tree = model::DataTree::default();
        let id = mesh::create_mesh(
            &mut tree,
            None,
            vec![[0., 0., 0.], [3., 0., 0.], [0., 3., 0.]],
            vec![[0, 1, 2]; count],
            None,
        )
        .unwrap();
        let mut objects = model::objects(&tree);
        objects[0].transform.translation = Vec3::new(10., 20., 30.);
        model::set_objects(&mut tree, objects);
        (tree, id)
    }

    #[test]
    fn world_faces_are_paged_without_mutation() {
        let (tree, id) = fixture(20);
        let mut offset = 0;
        let mut indices = vec![];
        loop {
            let value = inspect(
                &tree,
                InspectMeshFacesArgs {
                    id,
                    offset: Some(offset),
                    limit: None,
                    bounds: None,
                    detail: FaceDetail::Geometry,
                },
            )
            .unwrap();
            assert!(value.to_string().len() < 3400);
            for face in value["faces"].as_array().unwrap() {
                indices.push(face["face_index"].as_u64().unwrap());
                assert_eq!(face["world_centroid"], json!([11., 21., 30.]));
                assert_eq!(face["world_bounds"]["min"], json!([10., 20., 30.]));
            }
            if value["next_offset"].is_null() {
                break;
            }
            offset = value["next_offset"].as_u64().unwrap() as usize;
        }
        assert_eq!(indices, (0..20).collect::<Vec<_>>());
        assert_eq!(
            model::objects_ref(&tree)[0]
                .imported_mesh
                .as_ref()
                .unwrap()
                .triangles
                .len(),
            20
        );
    }

    #[test]
    fn world_coordinates_include_parent_group_transform() {
        let (mut tree, id) = fixture(1);
        let mut objects = model::objects(&tree);
        let mut parent = SdfObject::create_kind(PrimitiveKind::Box);
        parent.group_transform.translation = Vec3::new(-10., -20., -30.);
        parent.group_transform.scale = Vec3::splat(2.);
        objects[0].boolean_parent = Some(parent.uuid);
        objects.push(parent);
        model::set_objects(&mut tree, objects);
        let value = inspect(&tree, serde_json::from_value(json!({"id":id})).unwrap()).unwrap();
        assert_eq!(
            value["faces"][0]["world_vertices"],
            json!([[10., 20., 30.], [16., 20., 30.], [10., 26., 30.]])
        );
        assert_eq!(value["faces"][0]["world_centroid"], json!([12., 22., 30.]));
    }

    #[test]
    fn indices_mode_scales_and_preserves_pagination() {
        let (tree, id) = fixture(1000);
        let value = inspect(
            &tree,
            serde_json::from_value(json!({"id":id,"detail":"indices","offset":256})).unwrap(),
        )
        .unwrap();
        assert_eq!(value["face_indices"].as_array().unwrap().len(), 256);
        assert_eq!(value["face_indices"][0], 256);
        assert_eq!(value["next_offset"], 512);
        assert!(value.get("faces").is_none());
        assert!(value.to_string().len() < 4096);
        assert!(mcp::mcp_tools()
            .iter()
            .any(|tool| tool["name"] == "inspect_mesh_faces"));
    }

    #[test]
    fn bounds_modes_are_explicit_and_inclusive() {
        let (tree, id) = fixture(1);
        let run = |mode| {
            inspect(
                &tree,
                InspectMeshFacesArgs {
                    id,
                    offset: None,
                    limit: None,
                    detail: FaceDetail::Geometry,
                    bounds: Some(FaceBounds {
                        min: [11., 21., 30.],
                        max: [11., 21., 30.],
                        mode,
                    }),
                },
            )
            .unwrap()
        };
        assert_eq!(
            run(FaceBoundsMode::Centroid)["faces"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            run(FaceBoundsMode::AllVertices)["faces"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            run(FaceBoundsMode::BoundsOverlap)["faces"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rejects_bad_bounds_limits_offsets_and_missing_objects() {
        let (tree, id) = fixture(1);
        for value in [
            json!({"id":id,"limit":9}),
            json!({"id":id,"offset":2}),
            json!({"id":uuid::Uuid::new_v4()}),
            json!({"id":id,"bounds":{"min":[2,0,0],"max":[1,1,1],"mode":"centroid"}}),
        ] {
            assert!(inspect(&tree, serde_json::from_value(value).unwrap()).is_err());
        }
        assert!(
            serde_json::from_value::<InspectMeshFacesArgs>(json!({"id":id,
            "bounds":{"min":[0,0,0],"max":[1,1,1]}}))
            .is_err()
        );
    }
}
