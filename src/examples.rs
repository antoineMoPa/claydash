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
