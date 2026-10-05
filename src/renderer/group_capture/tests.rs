use super::*;

#[cfg(test)]
mod saved_capture_tests {
    use super::*;

    #[test]
    fn splats_only_bake_on_request_and_edits_require_recompute() {
        let context = egui::Context::default();
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 16;
        let id = root.uuid;
        let mut cache = HashMap::new();
        let idle = prepare_group_scene(&[root.clone()], &mut cache);
        assert!(idle.gaussian_splats.is_empty());
        assert!(cache.is_empty());
        let computed = compute_group_scene(&[root.clone()], &mut cache);
        let atlas = computed.box_depth_atlases[&id].clone();
        publish_depth_accelerator_status(&context, &[root.clone()], &cache, None);
        assert_eq!(
            depth_accelerator_status(&context, &[root.clone()], id, 0),
            DepthAcceleratorStatus::Ready
        );
        let recomputed = compute_group_scene(&[root.clone()], &mut cache);
        assert!(!std::sync::Arc::ptr_eq(
            &atlas,
            &recomputed.box_depth_atlases[&id]
        ));
        for resolution_edit in [false, true] {
            if resolution_edit {
                root.gaussian_splats.resolution = 17;
            } else {
                root.transform.scale *= 1.5;
            }
            let edited = prepare_group_scene(&[root.clone()], &mut cache);
            assert!(edited.gaussian_splats.is_empty());
            publish_depth_accelerator_status(&context, &[root.clone()], &cache, None);
            assert_eq!(
                depth_accelerator_status(
                    &context,
                    &[root.clone()],
                    id,
                    if resolution_edit { 2 } else { 1 }
                ),
                DepthAcceleratorStatus::NotComputedYet
            );
            let computed = compute_group_scene(&[root.clone()], &mut cache);
            assert!(computed.gaussian_splats.contains(&id));
        }
    }

    #[test]
    fn numeric_splat_resolution_exceeds_old_caps_and_rejects_impossible_uploads() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 129;
        let mut cache = HashMap::new();
        let prepared = prepare_group_scene_with_budget(
            &[root.clone()],
            &mut cache,
            &HashMap::new(),
            &HashSet::from([root.uuid]),
            1 << 24,
        );
        assert_eq!(prepared.box_depth_atlases[&root.uuid].resolution, 129);
        root.gaussian_splats.resolution = u32::MAX;
        let prepared = prepare_group_scene_with_budget(
            &[root.clone()],
            &mut cache,
            &HashMap::new(),
            &HashSet::from([root.uuid]),
            1 << 24,
        );
        assert!(prepared.box_depth_atlases.is_empty());
        assert!(!prepared.gaussian_splats.contains(&root.uuid));
        assert_eq!(prepared.objects[0].object_type, root.object_type);
    }

    #[test]
    fn splat_resolution_rebuilds_capture_and_unchanged_resolution_reuses_it() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        root.gaussian_splats.resolution = 16;
        let mut cache = HashMap::new();
        let first = compute_group_scene(&[root.clone()], &mut cache);
        let atlas = first.box_depth_atlases[&root.uuid].clone();
        assert_eq!(atlas.resolution, 16);
        let reused = prepare_group_scene(&[root.clone()], &mut cache);
        assert!(std::sync::Arc::ptr_eq(
            &atlas,
            &reused.box_depth_atlases[&root.uuid]
        ));
        root.gaussian_splats.resolution = 32;
        let rebuilt = compute_group_scene(&[root.clone()], &mut cache);
        assert_eq!(rebuilt.box_depth_atlases[&root.uuid].resolution, 32);
        assert!(!std::sync::Arc::ptr_eq(
            &atlas,
            &rebuilt.box_depth_atlases[&root.uuid]
        ));
        let mut saved = [root.clone()];
        objects_with_saved_captures(&[root], &mut saved, &cache);
        let restored: Vec<SdfObject> =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored[0].gaussian_splats.resolution, 32);
        let mut reopened_cache = HashMap::new();
        prepare_group_scene(&restored, &mut reopened_cache);
        match &restored[0].saved_group_capture.as_ref().unwrap().capture {
            CachedCapture::Box(saved_atlas) => match &reopened_cache[&restored[0].uuid].capture {
                CachedCapture::Box(reopened) => {
                    assert!(std::sync::Arc::ptr_eq(saved_atlas, reopened))
                }
                _ => panic!("wrong capture type"),
            },
            _ => panic!("wrong saved capture type"),
        }
    }

    #[test]
    fn depth_captures_compute_on_selection_and_source_edit() {
        for mode in [
            GroupRenderRepresentation::BoxDepthAtlas,
            GroupRenderRepresentation::SphereDepthAtlas,
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
        ] {
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let id = root.uuid;
            let mut source = vec![root];
            let mut cache = HashMap::new();
            let prepared = prepare_group_scene_with_neural(
                &source,
                &mut cache,
                &HashMap::new(),
                &HashSet::new(),
            );
            assert!(cache.contains_key(&id));
            assert_eq!(
                prepared.gaussian_splats.contains(&id),
                mode == GroupRenderRepresentation::GaussianSplats
            );
            let original_key = cache[&id].key;
            source[0].transform.scale *= 2.0;
            let prepared = prepare_group_scene_with_neural(
                &source,
                &mut cache,
                &HashMap::new(),
                &HashSet::new(),
            );
            assert!(cache.contains_key(&id));
            assert_ne!(cache[&id].key, original_key);
            assert_eq!(
                prepared.box_depth_atlases.contains_key(&id),
                matches!(
                    mode,
                    GroupRenderRepresentation::BoxDepthAtlas
                        | GroupRenderRepresentation::BoxAccelerator
                        | GroupRenderRepresentation::GaussianSplats
                )
            );
            assert_eq!(
                prepared.sphere_depth_atlases.contains_key(&id),
                matches!(
                    mode,
                    GroupRenderRepresentation::SphereDepthAtlas
                        | GroupRenderRepresentation::SphereAccelerator
                )
            );
        }
    }

    #[test]
    fn saved_depth_and_gaussian_captures_reopen_without_baking() {
        for mode in [
            GroupRenderRepresentation::BoxDepthAtlas,
            GroupRenderRepresentation::SphereDepthAtlas,
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
            GroupRenderRepresentation::GaussianSplats,
        ] {
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let mut child = SdfObject::create_kind(PrimitiveKind::Sphere);
            child.boolean_parent = Some(root.uuid);
            child.transform.translation.x = 0.2;
            child.color = glam::Vec4::new(0.1, 0.2, 0.9, 1.0);
            let id = root.uuid;
            let source = vec![root, child];
            let mut cache = HashMap::new();
            let original = compute_group_scene(&source, &mut cache);
            let mut objects = source.clone();
            objects_with_saved_captures(&source, &mut objects, &cache);
            assert!(objects[0].saved_group_capture.is_some());
            let mut tree = crate::model::DataTree::default();
            crate::model::set_objects(&mut tree, objects);
            let bytes = crate::document::serialize_scene(&tree).unwrap();
            let scene = crate::document::deserialize_scene(&bytes).unwrap();
            let mut reopened = crate::model::DataTree::default();
            reopened.set_tree("scene", scene);
            let loaded = crate::model::objects_ref(&reopened);
            let mut fresh_cache = HashMap::new();
            let prepared = prepare_group_scene(loaded, &mut fresh_cache);
            let saved = loaded[0].saved_group_capture.as_ref().unwrap();
            match &saved.capture {
                CachedCapture::Box(atlas) => {
                    assert!(std::sync::Arc::ptr_eq(
                        atlas,
                        &prepared.box_depth_atlases[&id]
                    ));
                    let before = &original.box_depth_atlases[&id];
                    assert_eq!(atlas.texels, before.texels);
                    assert_eq!(atlas.owners, before.owners);
                    assert_eq!(atlas.normals, before.normals);
                    assert_eq!(atlas.local_max, before.local_max);
                    assert_eq!(atlas.layers, before.layers);
                }
                CachedCapture::Sphere(atlas) => {
                    assert!(std::sync::Arc::ptr_eq(
                        atlas,
                        &prepared.sphere_depth_atlases[&id]
                    ));
                    let before = &original.sphere_depth_atlases[&id];
                    assert_eq!(atlas.texels, before.texels);
                    assert_eq!(atlas.owners, before.owners);
                    assert_eq!(atlas.radius, before.radius);
                }
            }
            assert_eq!(
                prepared.gaussian_splats.contains(&id),
                mode == GroupRenderRepresentation::GaussianSplats
            );
            assert_eq!(
                prepared.objects.len(),
                if mode.is_depth_accelerator() { 2 } else { 1 }
            );
            let mut edited = loaded.to_vec();
            edited[1].transform.scale *= 2.0;
            let key = capture_key(&edited, &edited[0]);
            assert!(restored_capture(&edited, &edited[0], key).is_none());
            let mut snapshot = edited.clone();
            objects_with_saved_captures(&edited, &mut snapshot, &fresh_cache);
            assert!(snapshot[0].saved_group_capture.is_none());
        }
    }

    #[test]
    fn invalid_saved_atlases_are_rejected() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
        let mut source = vec![root];
        let key = capture_key(&source, &source[0]);
        let original = bake_box_depth_atlas(
            &source,
            source[0].uuid,
            BOX_DEPTH_RESOLUTION,
            BoxCaptureStart::AtBounds,
        )
        .unwrap();
        for case in 0..7 {
            let mut atlas = original.clone();
            let mut version = 1;
            match case {
                0 => version = 2,
                1 => {
                    atlas.texels.pop();
                }
                2 => {
                    atlas.owners.pop();
                }
                3 => {
                    atlas.normals.pop();
                }
                4 => atlas.texels[0][0] = f32::NAN,
                5 => atlas.owners[0] = Some(uuid::Uuid::new_v4()),
                6 => atlas.local_max = Vec3::ZERO,
                _ => unreachable!(),
            }
            source[0].saved_group_capture =
                Some(std::sync::Arc::new(crate::model::SavedGroupCapture {
                    version,
                    source_key: key,
                    capture: CachedCapture::Box(std::sync::Arc::new(atlas)),
                }));
            assert!(
                restored_capture(&source, &source[0], key).is_none(),
                "case {case}"
            );
        }
    }
}

#[cfg(test)]
mod sphere_accelerator_tests {
    use super::*;

    #[test]
    fn accelerator_bakes_same_sphere_texture_and_retains_exact_subtree() {
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::SphereAccelerator;
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        child.operation = crate::model::BooleanOperation::Subtract;
        child.transform.translation.x = 0.2;
        // A capture on a descendant must not replace the exact finish geometry.
        child.render_representation = GroupRenderRepresentation::BoxDepthAtlas;
        let id = root.uuid;
        let source = [root, child];
        let mut cache = HashMap::new();
        let prepared =
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
        assert_eq!(prepared.objects.len(), 2);
        let atlas = &prepared.sphere_depth_atlases[&id];
        let expected =
            bake_sphere_depth_atlas(&source, id, SPHERE_DEPTH_WIDTH, SPHERE_DEPTH_HEIGHT).unwrap();
        assert_eq!(atlas.texels, expected.texels);
        assert_eq!(atlas.owners, expected.owners);
        assert_eq!(prepared.objects.len(), 2);
        assert!(prepared.box_depth_atlases.is_empty());
        for (actual, original) in prepared.objects.iter().zip(&source) {
            assert_eq!(actual.object_type, original.object_type);
            assert_eq!(actual.boolean_parent, original.boolean_parent);
            assert_eq!(actual.operation, original.operation);
        }
        let mut edited = source.clone();
        edited[0].sphere_accelerator.configurable_epsilon = 0.2;
        let reused =
            prepare_group_scene_with_neural(&edited, &mut cache, &HashMap::new(), &HashSet::new());
        assert!(
            std::sync::Arc::ptr_eq(atlas, &reused.sphere_depth_atlases[&id]),
            "changing refinement distance must not rebake"
        );
    }
}

#[cfg(test)]
mod depth_accelerator_status_tests {
    use super::*;

    #[test]
    fn readiness_tracks_baking_source_edits_and_cache_restoration() {
        for mode in [
            GroupRenderRepresentation::SphereAccelerator,
            GroupRenderRepresentation::BoxAccelerator,
        ] {
            let context = egui::Context::default();
            let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
            root.render_representation = mode;
            let id = root.uuid;
            let mut source = vec![root];
            let mut cache = HashMap::new();
            let mut revision = 0;
            publish_depth_accelerator_status(&context, &source, &cache, None);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::NotComputedYet
            );
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
            publish_depth_accelerator_status(&context, &source, &cache, None);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            match mode {
                GroupRenderRepresentation::SphereAccelerator => {
                    source[0].sphere_accelerator.configurable_epsilon = 0.2
                }
                GroupRenderRepresentation::BoxAccelerator => {
                    source[0].box_accelerator.configurable_epsilon = 0.2
                }
                _ => unreachable!(),
            }
            revision += 1;
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            source[0].transform.scale *= 1.5;
            revision += 1;
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::NotComputedYet,
                "an edit must invalidate readiness before the next render"
            );
            prepare_group_scene_with_neural(&source, &mut cache, &HashMap::new(), &HashSet::new());
            publish_depth_accelerator_status(&context, &source, &cache, None);
            assert_eq!(
                depth_accelerator_status(&context, &source, id, revision),
                DepthAcceleratorStatus::Ready
            );
            let mut saved = source.clone();
            objects_with_saved_captures(&source, &mut saved, &cache);
            let mut restored = HashMap::new();
            prepare_group_scene_with_neural(
                &saved,
                &mut restored,
                &HashMap::new(),
                &HashSet::new(),
            );
            publish_depth_accelerator_status(&context, &saved, &restored, None);
            assert_eq!(
                depth_accelerator_status(&context, &saved, id, revision + 1),
                DepthAcceleratorStatus::Ready
            );
        }
    }
}
