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
