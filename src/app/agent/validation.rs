use super::*;

pub(super) fn params_match(kind: PrimitiveKind, params: &SdfParams) -> bool {
    matches!(
        (kind, params),
        (PrimitiveKind::Box, SdfParams::BoxParams(_))
            | (PrimitiveKind::Sphere, SdfParams::SphereParams(_))
            | (PrimitiveKind::Cylinder, SdfParams::CylinderParams { .. })
            | (PrimitiveKind::Torus, SdfParams::TorusParams { .. })
            | (
                PrimitiveKind::PolygonPrism,
                SdfParams::PolygonPrismParams(_)
            )
            | (PrimitiveKind::BezierCurve, SdfParams::BezierCurveParams(_))
            | (PrimitiveKind::Text, SdfParams::TextParams(_))
            | (PrimitiveKind::Loft, SdfParams::LoftParams(_))
    )
}

pub(super) fn validate_scene(tree: &model::DataTree) -> Result<(), String> {
    crate::renderer::post_processing::validate_passes(model::post_processing_ref(tree))?;
    let assets = model::material_assets(tree);
    crate::renderer::validate_custom_materials(&assets)?;
    let objects = model::objects_ref(tree);
    let mut ids = std::collections::HashSet::new();
    for object in objects {
        if object.material.kind == model::MaterialKind::Custom
            && !object.material_id.is_some_and(|id| {
                assets.iter().any(|asset| {
                    asset.uuid == id && asset.material.kind == model::MaterialKind::Custom
                })
            })
        {
            return Err(format!("custom material asset missing on {}", object.uuid));
        }
        if !object.voxels.is_valid() {
            return Err(format!("invalid voxel resolution on {}", object.uuid));
        }
        if !object.sphere_accelerator.is_valid() || !object.box_accelerator.is_valid() {
            return Err(format!(
                "invalid depth accelerator distance on {}",
                object.uuid
            ));
        }
        if !ids.insert(object.uuid) {
            return Err(format!("duplicate object id {}", object.uuid));
        }
        let kind = match object.object_type {
            sdf_consts::TYPE_BOX => PrimitiveKind::Box,
            sdf_consts::TYPE_SPHERE => PrimitiveKind::Sphere,
            sdf_consts::TYPE_CYLINDER => PrimitiveKind::Cylinder,
            sdf_consts::TYPE_TORUS => PrimitiveKind::Torus,
            sdf_consts::TYPE_POLYGON_PRISM => PrimitiveKind::PolygonPrism,
            sdf_consts::TYPE_BEZIER_CURVE => PrimitiveKind::BezierCurve,
            sdf_consts::TYPE_LOFT => PrimitiveKind::Loft,
            sdf_consts::TYPE_TEXT => PrimitiveKind::Text,
            _ => return Err(format!("unknown primitive type on {}", object.uuid)),
        };
        if !params_match(kind, &object.params) {
            return Err(format!("primitive parameters do not match {}", object.uuid));
        }
        if let SdfParams::TextParams(text) = &object.params {
            text.validate()
                .map_err(|error| format!("{error} on {}", object.uuid))?;
            model::prepared_scene_text(objects, object)
                .map_err(|error| format!("{error} on {}", object.uuid))?;
        }
        if let SdfParams::BoxParams(box_params) = &object.params {
            if !box_params.corner_radius.is_finite() || box_params.corner_radius < 0.0 {
                return Err(format!("invalid box corner radius on {}", object.uuid));
            }
        }
        if let SdfParams::LoftParams(loft) = &object.params {
            let profile_count = loft.profile_count();
            if !(2..=model::LoftParams::MAX_SECTIONS).contains(&loft.sections.len())
                || loft.sections.iter().any(|section| {
                    !section.x.is_finite()
                        || !section.center_y.is_finite()
                        || !section.center_z.is_finite()
                        || !section.half_height.is_finite()
                        || !section.half_width.is_finite()
                        || section.half_height <= 0.0
                        || section.half_width <= 0.0
                        || section.profile.as_ref().is_some_and(|profile| {
                            !(3..=model::LoftParams::MAX_PROFILE_POINTS).contains(&profile.len())
                                || profile.len() != profile_count
                                || profile.iter().any(|point| !point.is_finite())
                        })
                })
                || loft.sections.windows(2).any(|pair| pair[0].x >= pair[1].x)
            {
                return Err(format!("invalid loft sections on {}", object.uuid));
            }
        }
        if let Some(inlay) = object.surface_inlay {
            let Some(host) = objects
                .iter()
                .find(|candidate| candidate.uuid == inlay.host)
            else {
                return Err(format!("surface inlay host missing on {}", object.uuid));
            };
            let host_subtree = commands::selected_subtree_ids(objects, &[host.uuid]);
            if host.uuid == object.uuid
                || !inlay.offset.is_finite()
                || !inlay.thickness.is_finite()
                || inlay.thickness <= 0.0
                || matches!(object.params, SdfParams::BezierCurveParams(_))
                || object.boolean_parent.is_some()
                || object.lattice.is_some()
                || object.mirror.is_some()
                || object.repetition.enabled
                || host.boolean_parent.is_some()
                || host_subtree.contains(&object.uuid)
                || host_subtree.iter().any(|id| {
                    objects
                        .iter()
                        .find(|candidate| candidate.uuid == *id)
                        .is_none_or(|member| {
                            member.surface_inlay.is_some()
                                || member.lattice.is_some()
                                || member.mirror.is_some()
                                || member.repetition.enabled
                                || matches!(member.params, SdfParams::BezierCurveParams(_))
                        })
                })
            {
                return Err(format!("invalid surface inlay on {}", object.uuid));
            }
        }
        if !object.transform.translation.is_finite()
            || !object.transform.scale.is_finite()
            || !object.transform.rotation.is_finite()
            || !object.group_transform.translation.is_finite()
            || !object.group_transform.scale.is_finite()
            || !object.group_transform.rotation.is_finite()
        {
            return Err(format!("nonfinite transform on {}", object.uuid));
        }
    }
    model::validate_scene_text_budget(objects)?;
    for object in objects {
        let mut cursor = object.boolean_parent;
        let mut visited = std::collections::HashSet::new();
        while let Some(parent) = cursor {
            if !visited.insert(parent) || parent == object.uuid {
                return Err(format!("boolean cycle at {}", object.uuid));
            }
            cursor = objects
                .iter()
                .find(|other| other.uuid == parent)
                .ok_or_else(|| format!("missing boolean parent {parent}"))?
                .boolean_parent;
        }
    }
    let variables = model::scene_variables(tree);
    let mut variable_ids = std::collections::HashSet::new();
    for variable in &variables.vectors {
        if !variable_ids.insert(variable.id) {
            return Err(format!("duplicate variable id {}", variable.id));
        }
        if !variable.value.is_finite() {
            return Err(format!("nonfinite variable value {}", variable.id));
        }
    }
    let mut targets = Vec::new();
    for binding in &variables.bindings {
        if targets.contains(&(binding.object, binding.target)) {
            return Err(format!("duplicate binding target on {}", binding.object));
        }
        targets.push((binding.object, binding.target));
        let variable = variables
            .vectors
            .iter()
            .find(|variable| variable.id == binding.variable)
            .ok_or_else(|| format!("binding variable {} does not exist", binding.variable))?;
        let object = objects
            .iter()
            .find(|object| object.uuid == binding.object)
            .ok_or_else(|| format!("binding object {} does not exist", binding.object))?;
        if !binding.offset.is_finite() || !(variable.value + binding.offset).is_finite() {
            return Err(format!(
                "nonfinite binding offset or evaluated value on {}",
                binding.object
            ));
        }
        if let model::VectorBindingTarget::BezierPoint(index) = binding.target {
            match &object.params {
                SdfParams::BezierCurveParams(params) if index < params.points.len() => {}
                _ => return Err(format!("invalid curve point {index} on {}", binding.object)),
            }
        }
        if variable.space == model::VariableSpace::World {
            let inverse =
                model::vector_binding_frame(objects, binding.object, binding.target).inverse();
            if !inverse.is_finite()
                || !inverse
                    .transform_point3(variable.value + binding.offset)
                    .is_finite()
            {
                return Err(format!(
                    "invalid world binding coordinate frame on {}",
                    binding.object
                ));
            }
        }
    }
    model::four_bar_constraint_order(&variables)?;
    let mut drivers = std::collections::HashSet::new();
    for constraint in &variables.four_bar_constraints {
        if !drivers.insert(constraint.driver) {
            return Err("four-bar constraints must have distinct driver points".into());
        }
        if model::is_variable_derived(&variables, constraint.driver) {
            return Err("four-bar driver cannot also be a derived point".into());
        }
        let point = |id| {
            variables
                .vectors
                .iter()
                .find(|v| v.id == id && v.space == model::VariableSpace::World)
                .map(|v| v.value)
                .ok_or_else(|| format!("four-bar point {id} must exist in World space"))
        };
        let driver = point(constraint.driver)?;
        let lower = point(constraint.lower_pivot)?;
        let upper = point(constraint.upper_pivot)?;
        point(constraint.lower_joint)?;
        point(constraint.upper_joint)?;
        let pivot_delta = upper - lower;
        let pivot_distance = pivot_delta.truncate().length();
        let mut travel_checks = vec![constraint.travel_min, constraint.travel_max, driver.y];
        if pivot_distance > 1e-6 && pivot_delta.x != 0.0 {
            let critical = lower.y
                + pivot_delta.x.signum() * pivot_delta.y * constraint.lower_length / pivot_distance
                - constraint.driver_y_offset;
            if critical >= constraint.travel_min && critical <= constraint.travel_max {
                travel_checks.push(critical);
            }
        }
        for y in travel_checks {
            if model::solve_planar_four_bar(
                constraint,
                Vec3::new(driver.x, y, driver.z),
                lower,
                upper,
            )
            .is_none()
            {
                return Err("four-bar travel is nonfinite or outside the feasible fixed-length linkage range".into());
            }
        }
    }
    let mut rigid_targets = Vec::new();
    for binding in &variables.rigid_bindings {
        if rigid_targets.contains(&(binding.object, binding.target)) {
            return Err(format!(
                "duplicate rigid binding target on {}",
                binding.object
            ));
        }
        rigid_targets.push((binding.object, binding.target));
        let position_target = match binding.target {
            model::RigidBindingTarget::Object => model::VectorBindingTarget::Position,
            model::RigidBindingTarget::Group => model::VectorBindingTarget::GroupPosition,
        };
        if variables
            .bindings
            .iter()
            .any(|b| b.object == binding.object && b.target == position_target)
        {
            return Err("rigid pose conflicts with a position binding".into());
        }
        if !binding.local_origin.is_finite()
            || !binding.local_aim.is_finite()
            || !binding.local_up.is_finite()
        {
            return Err("nonfinite rigid local anchors".into());
        }
        model::rigid_binding_pose(&variables, objects, binding)?;
    }
    Ok(())
}
