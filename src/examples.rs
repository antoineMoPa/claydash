//! Example metadata only; scene files are served separately from the web build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Example {
    pub title: &'static str,
    pub file: &'static str,
}

pub const EXAMPLES: &[Example] = &[
    Example {
        title: "Astra car",
        file: "astra_car.claydash",
    },
    Example {
        title: "Concrete tower",
        file: "concrete_tower.claydash",
    },
    Example {
        title: "Flying sedan",
        file: "flying_sedan.claydash",
    },
    Example {
        title: "Fusca",
        file: "fusca.claydash",
    },
    Example {
        title: "Medieval sword",
        file: "sword.claydash",
    },
    Example {
        title: "O’Neill cylinder",
        file: "oneil_cylinder.claydash",
    },
    Example {
        title: "Retro flying car",
        file: "retro_flying_car.claydash",
    },
    Example {
        title: "SSAO duck",
        file: "ssao_duck.claydash",
    },
    Example {
        title: "Single-wheel suspension",
        file: "single_wheel_suspension.claydash",
    },
    Example {
        title: "Text on path",
        file: "text_on_path.claydash",
    },
];

#[cfg(test)]
#[test]
fn fusca_is_one_movable_car_with_original_world_geometry_and_inlays() {
    let scene = crate::document::deserialize_scene(include_bytes!("../examples/fusca.claydash"))
        .expect("bundled Fusca scene");
    let objects = match scene.get_path("sdf_objects") {
        crate::model::ClaydashValue::VecSDFObject(objects) => objects,
        _ => panic!("Fusca objects missing"),
    };
    let master = uuid::Uuid::parse_str("6c0ae608-067e-4d00-955b-fd016eef7d88").unwrap();
    let shell = uuid::Uuid::parse_str("2b03e7e7-2b69-4708-b456-1bc62963e005").unwrap();
    assert_eq!(objects.len(), 78);
    assert_eq!(objects.iter().filter(|object| object.boolean_parent.is_none()).count(), 1);
    let root = objects.iter().find(|object| object.uuid == master).unwrap();
    assert!(root.boolean_parent.is_none());
    assert_eq!(root.name, "1965 Volkswagen Fusca");
    assert_eq!(root.operation, crate::model::BooleanOperation::Union);
    assert_eq!(root.softness, 0.0);
    assert!(root.mirror.is_none() && root.lattice.is_none() && !root.repetition.enabled);
    assert!(root.surface_inlay.is_none());
    assert_eq!(objects.iter().filter(|object| object.boolean_parent == Some(master)).count(), 66);
    let by_id: std::collections::HashMap<_, _> = objects.iter()
        .map(|object| (object.uuid, object)).collect();
    assert_eq!(by_id.len(), objects.len());
    for object in &objects {
        let mut current = object;
        for _ in 0..objects.len() {
            if current.uuid == master { break; }
            current = by_id[&current.boolean_parent.expect("car part has a parent")];
        }
        assert_eq!(current.uuid, master, "{} escaped the car hierarchy", object.name);
        assert_eq!(object.group_transform.matrix(), glam::Mat4::IDENTITY,
            "grouping must not alter a part's world transform");
        let world = crate::model::object_world_matrix(&objects, object.uuid);
        let original = object.transform.matrix();
        for (actual, expected) in world.to_cols_array().into_iter()
            .zip(original.to_cols_array()) {
            assert!((actual - expected).abs() < 1e-5, "{} moved", object.name);
        }
    }
    // These sums pin the saved part transforms independently of the new
    // hierarchy; grouping only changes Boolean parent links.
    let translation_sum = objects.iter().fold(glam::DVec3::ZERO, |sum, object| {
        sum + object.transform.translation.as_dvec3()
    });
    let expected = glam::DVec3::new(23.508, 38.83208, 13.887);
    assert!((translation_sum - expected).length() < 1e-4);
    let inlays: Vec<_> = objects.iter().filter_map(|object| object.surface_inlay.as_ref()).collect();
    assert_eq!(inlays.len(), 40);
    assert!(inlays.iter().all(|inlay| inlay.host == shell));
    let shell = by_id[&shell];
    assert_eq!(shell.boolean_parent, Some(master));
    assert_eq!(shell.softness, 0.13);
    for name in ["Cabin windshield rake cut", "Flat underside trim"] {
        assert!(objects.iter().any(|object| object.name == name
            && object.boolean_parent == Some(shell.uuid)
            && object.operation == crate::model::BooleanOperation::Subtract));
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export async function fetchExample(file) {
    const response = await fetch(`/examples/${encodeURIComponent(file)}`);
    if (!response.ok) throw new Error(`Example download failed (HTTP ${response.status})`);
    return new Uint8Array(await response.arrayBuffer());
}
export function openGuide() {
    window.open('https://claydash.com/docs/', '_blank', 'noopener,noreferrer');
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = fetchExample)]
    pub async fn fetch_example(file: &str) -> Result<js_sys::Uint8Array, wasm_bindgen::JsValue>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = openGuide)]
    pub fn open_guide();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_guide() -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg("https://claydash.com/docs/").spawn()?;
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn flying_sedan_preserves_native_panels_paint_and_assembly_on_save_open() {
        use crate::model::{self, BooleanOperation, SdfParams, SheetCut, SheetStage};
        let scene = crate::document::deserialize_scene(include_bytes!(
            "../examples/flying_sedan.claydash"
        ))
        .expect("bundled flying sedan scene");
        let mut tree = model::DataTree::default();
        tree.set_tree("scene", scene);
        let objects = model::objects(&tree);
        assert_eq!(objects.len(), 349);
        let roots: Vec<_> = objects.iter().filter(|o| o.boolean_parent.is_none()).collect();
        assert_eq!(roots.len(), 1);
        let root = roots[0];
        assert_eq!(root.name, "Flying sedan");
        assert_eq!(root.operation, BooleanOperation::Union);
        assert_eq!(root.softness, 0.0);
        let by_id: std::collections::HashMap<_, _> =
            objects.iter().map(|o| (o.uuid, o)).collect();
        assert_eq!(by_id.len(), objects.len());
        for object in &objects {
            let mut ancestor = object;
            for _ in 0..objects.len() {
                if ancestor.uuid == root.uuid {
                    break;
                }
                ancestor = by_id[&ancestor.boolean_parent.expect("sedan part has a parent")];
            }
            assert_eq!(ancestor.uuid, root.uuid, "{} escaped the sedan", object.name);
        }
        let panels: Vec<_> = objects.iter().filter_map(|o| match &o.params {
            SdfParams::SheetPanelParams(panel) => Some(panel),
            _ => None,
        }).collect();
        assert_eq!(panels.len(), 184);
        assert_eq!(panels.iter().filter(|p| !p.embosses.is_empty()).count(), 26);
        assert_eq!(panels.iter().map(|p| p.embosses.len()).sum::<usize>(), 42);
        assert!(panels.iter().all(|p| p.stage == SheetStage::Formed));
        for side in ["RH", "LH"] {
            let fender = objects.iter().find(|o| o.name == format!(
                "{side} front fender | {side} front fender / nose stamped blank"
            )).expect("native stamped front fender");
            let SdfParams::SheetPanelParams(panel) = &fender.params else {
                panic!("fender must remain a native sheet");
            };
            assert!(panel.cuts.iter().any(|cut| matches!(cut, SheetCut::Circle { .. })));
            assert!(!panel.bends.is_empty());
            assert!(!panel.embosses.is_empty());
            assert_eq!(fender.material.color, glam::Vec4::new(0.115, 0.29, 0.52, 1.0));
            assert_eq!(fender.color, fender.material.color);
            assert_eq!(fender.material.roughness, 0.28);
            assert_eq!(fender.material.metallic, 0.3);
        }
        let spindles: Vec<_> = objects.iter().filter(|o|
            o.name.contains("hover fan / centre spindle")
        ).collect();
        assert_eq!(spindles.len(), 4);
        assert!(spindles.iter().all(|o| (
            model::object_world_matrix(&objects, o.uuid)
                .transform_point3(glam::Vec3::ZERO).y - 0.4
        ).abs() < 1e-6));
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let mut loaded = model::DataTree::default();
        loaded.set_tree("scene", crate::document::deserialize_scene(&bytes).unwrap());
        assert_eq!(serde_json::to_value(model::objects(&loaded)).unwrap(),
            serde_json::to_value(&objects).unwrap(),
            "Save/Open must retain every recipe, transform, material and parent link");
    }

    #[test]
    fn suspension_keeps_rigid_arms_and_articulates_wheel_through_travel() {
        use crate::model::{self, SdfParams};
        let scene = crate::document::deserialize_scene(include_bytes!(
            "../examples/single_wheel_suspension.claydash"
        ))
        .expect("bundled suspension scene");
        let mut tree = model::DataTree::default();
        tree.set_tree("scene", scene);
        let variables = model::scene_variables(&tree);
        assert_eq!(variables.vectors.len(), 10);
        assert_eq!(variables.bindings.len(), 5);
        assert_eq!(variables.rigid_bindings.len(), 28);
        assert_eq!(variables.four_bar_constraints.len(), 1);
        model::set_scene_variables(&mut tree, variables.clone());
        let neutral = model::objects(&tree);
        assert_eq!(neutral.len(), 35);
        assert_eq!(
            model::world(&tree).render_pipeline,
            model::RenderPipelineMode::Deferred
        );
        let root = neutral
            .iter()
            .find(|object| object.boolean_parent.is_none())
            .unwrap();
        assert_eq!(
            root.render_representation,
            model::GroupRenderRepresentation::PoissonMesh
        );
        assert_eq!(root.softness, 0.0);
        assert!(model::scene_cameras(&tree).is_empty());
        assert!(model::active_camera_id(&tree).is_none());
        for binding in &variables.rigid_bindings {
            assert!(neutral.iter().any(|object| object.uuid == binding.object));
            assert!(
                !variables
                    .bindings
                    .iter()
                    .any(|point| point.object == binding.object),
                "rigid bodies must not also deform their control points"
            );
        }
        let world_endpoints = |objects: &[model::SdfObject], name: &str| {
            let object = objects.iter().find(|object| object.name == name).unwrap();
            let SdfParams::BezierCurveParams(curve) = &object.params else {
                panic!("expected rod");
            };
            let matrix = model::object_world_matrix(objects, object.uuid);
            (
                matrix.transform_point3(curve.points[0]),
                matrix.transform_point3(*curve.points.last().unwrap()),
            )
        };
        for travel in [-0.65, -0.4, 0.0, 0.4, 0.65] {
            let mut changed = variables.clone();
            changed
                .vectors
                .iter_mut()
                .find(|v| v.name == "Wheel travel - drag Y")
                .unwrap()
                .value
                .y = travel;
            model::set_scene_variables(&mut tree, changed);
            let moved = model::objects(&tree);
            let solved = model::scene_variables(&tree);
            let point = |name: &str| {
                solved
                    .vectors
                    .iter()
                    .find(|v| v.name == name)
                    .unwrap()
                    .value
            };
            for (name, pivot, joint) in [
                (
                    "Upper wishbone - front",
                    "Upper chassis pivot - front",
                    "Upper ball joint",
                ),
                (
                    "Upper wishbone - rear",
                    "Upper chassis pivot - rear",
                    "Upper ball joint",
                ),
                (
                    "Lower wishbone - front",
                    "Lower chassis pivot - front",
                    "Lower ball joint",
                ),
                (
                    "Lower wishbone - rear",
                    "Lower chassis pivot - rear",
                    "Lower ball joint",
                ),
            ] {
                let (a, b) = world_endpoints(&moved, name);
                let (original_a, original_b) = world_endpoints(&neutral, name);
                assert!(
                    a.distance(point(pivot)) < 1e-5,
                    "{name} chassis pivot moved"
                );
                assert!(b.distance(point(joint)) < 1e-5, "{name} missed ball joint");
                assert!((a.distance(b) - original_a.distance(original_b)).abs() < 1e-5);
            }
            let (upper, lower) = world_endpoints(&moved, "Upright - upper to lower ball joint");
            assert!(upper.distance(point("Upper ball joint")) < 1e-5);
            assert!(lower.distance(point("Lower ball joint")) < 1e-5);
            assert!((upper.distance(lower) - 0.91).abs() < 1e-5);
            for binding in &variables.rigid_bindings {
                let before = neutral.iter().find(|o| o.uuid == binding.object).unwrap();
                let after = moved.iter().find(|o| o.uuid == binding.object).unwrap();
                assert_eq!(
                    serde_json::to_value(&after.params).unwrap(),
                    serde_json::to_value(&before.params).unwrap(),
                    "{} changed rigid geometry",
                    after.name
                );
                assert_eq!(after.transform.scale, before.transform.scale);
            }
            let tire = moved
                .iter()
                .find(|o| o.name == "Wheel - rubber tire")
                .unwrap();
            let original_tire = neutral.iter().find(|o| o.uuid == tire.uuid).unwrap();
            if travel.abs() > 0.3 {
                assert!(
                    tire.transform
                        .rotation
                        .angle_between(original_tire.transform.rotation)
                        > 0.04
                );
            }
            let (shaft_top, shaft_bottom) = world_endpoints(&moved, "Damper - linked piston");
            let (body_bottom, body_top) = world_endpoints(&moved, "Damper - red lower body");
            assert!(shaft_top.distance(point("Shock tower mount")) < 1e-5);
            assert!(body_bottom.distance(lower) < 1e-5);
            assert!((shaft_top.distance(shaft_bottom) - 1.9).abs() < 1e-5);
            assert!((body_bottom.distance(body_top) - 1.5).abs() < 1e-5);
            let damper_axis = shaft_top - body_bottom;
            assert!(damper_axis.cross(body_top - body_bottom).length() < 1e-5);
            assert!(damper_axis.cross(shaft_bottom - body_bottom).length() < 1e-5);
            assert!((body_top - shaft_bottom).dot(damper_axis) > 0.0, "damper pieces must overlap");
            assert_eq!(
                moved
                    .iter()
                    .find(|o| o.uuid == root.uuid)
                    .unwrap()
                    .transform,
                root.transform
            );
        }
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let scene = crate::document::deserialize_scene(&bytes).unwrap();
        let mut loaded = model::DataTree::default();
        loaded.set_tree("scene", scene);
        assert_eq!(
            model::scene_variables(&loaded),
            model::scene_variables(&tree)
        );
        assert_eq!(
            serde_json::to_value(model::objects(&loaded)).unwrap(),
            serde_json::to_value(model::objects(&tree)).unwrap()
        );
    }

    #[test]
    fn menu_covers_all_example_files_and_each_project_loads() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut files: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|file| file.ends_with(".claydash"))
            .collect();
        files.sort();
        let mut menu_files: Vec<_> = EXAMPLES
            .iter()
            .map(|example| example.file.to_owned())
            .collect();
        menu_files.sort();
        assert_eq!(files, menu_files);
        for example in EXAMPLES {
            let scene = crate::document::read_scene(&directory.join(example.file))
                .unwrap_or_else(|error| panic!("{}: {error}", example.file));
            let mut tree = crate::model::DataTree::default();
            tree.set_tree("scene", scene);
            assert!(!crate::model::objects(&tree).is_empty(), "{}", example.file);
        }
    }
}
