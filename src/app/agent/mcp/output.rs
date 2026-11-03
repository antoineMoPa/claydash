use serde_json::Value;

const TEXT_LIMIT: usize = 4096;
const COLLECTION_LIMIT: usize = 256;
const TRUNCATED: &str = "\n[truncated at 4096 characters]";

pub(super) fn error_text(text: String) -> String {
    if text.chars().count() <= TEXT_LIMIT {
        return text;
    }
    let mut result: String = text
        .chars()
        .take(TEXT_LIMIT - TRUNCATED.chars().count())
        .collect();
    result.push_str(TRUNCATED);
    result
}

pub(super) fn text(value: Value, omit_meshes: bool) -> String {
    error_text(compact_value(value, omit_meshes).to_string())
}

pub(in crate::app::agent) fn compact_file(value: Value) -> Value {
    compact_value(value, false)
}

// Map a response snapshot only: the live scene and manual saves are untouched.
fn compact_value(value: Value, omit_meshes: bool) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(COLLECTION_LIMIT)
                .map(|value| compact_value(value, omit_meshes))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .filter(|(key, _)| {
                    !omit_meshes
                        || !matches!(
                            key.as_str(),
                            "saved_mesh" | "imported_mesh" | "imported_splats"
                        )
                })
                .take(COLLECTION_LIMIT)
                .map(|(key, value)| (key, compact_value(value, omit_meshes)))
                .collect(),
        ),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn caps_unicode_text_with_a_notice() {
        let long = "🦆".repeat(5000);
        let capped = error_text(long.clone());
        assert_eq!(capped.chars().count(), TEXT_LIMIT);
        assert!(capped.ends_with(TRUNCATED));
    }

    #[test]
    fn caps_nested_collections_and_omits_scene_meshes() {
        let value = json!({"document":{"objects":[{
            "saved_mesh":{"vertices":[1,2,3]}, "imported_mesh":{"faces":[0]},
            "imported_splats":{}, "values":vec![0; 300]
        }]}});
        let compact = compact_value(value.clone(), true);
        let object = &compact["document"]["objects"][0];
        assert!(object.get("saved_mesh").is_none());
        assert!(object.get("imported_mesh").is_none());
        assert!(object.get("imported_splats").is_none());
        assert_eq!(object["values"].as_array().unwrap().len(), COLLECTION_LIMIT);

        let map = (0..300)
            .map(|index| (index.to_string(), json!({"inner":vec![0; 300]})))
            .collect();
        let capped = compact_value(Value::Object(map), false);
        assert_eq!(capped.as_object().unwrap().len(), COLLECTION_LIMIT);
        assert!(capped
            .as_object()
            .unwrap()
            .values()
            .all(|value| value["inner"].as_array().unwrap().len() == COLLECTION_LIMIT));
    }

    #[test]
    fn dispatch_limits_success_errors_and_capture_metadata() {
        for name in ["get_state", "get_mesh", "capture_comparison", "missing"] {
            let request = json!({"id":1,"method":"tools/call","params":{
                "name":name,"arguments":{}
            }});
            for fail in [false, true] {
                let response = super::super::mcp_response(&request, &mut |_| {
                    if fail {
                        Err("é".repeat(6000))
                    } else {
                        Ok(json!({"data":"image bytes", "details":"é".repeat(6000)}))
                    }
                })
                .unwrap();
                for content in response["result"]["content"].as_array().unwrap() {
                    if let Some(text) = content["text"].as_str() {
                        assert!(text.chars().count() <= TEXT_LIMIT);
                    } else {
                        assert_eq!(content["data"], "image bytes");
                    }
                }
            }
        }
    }

    #[test]
    fn pack_all_data_is_only_advertised_for_save() {
        for tool in super::super::mcp_tools() {
            assert_eq!(
                tool["inputSchema"]["properties"]
                    .get("packAllData")
                    .is_some(),
                tool["name"] == "save"
            );
        }
        let request = json!({"id":1,"method":"tools/call","params":{
            "name":"get_state","arguments":{"packAllData":true}
        }});
        let response =
            super::super::mcp_response(&request, &mut |_| panic!("must reject bypass")).unwrap();
        assert_eq!(response["result"]["isError"], true);
    }

    #[test]
    fn full_file_save_still_caps_the_response() {
        let request = json!({"id":1,"method":"tools/call","params":{
            "name":"save","arguments":{"packAllData":true,"mcp_save":false}
        }});
        let response = super::super::mcp_response(&request, &mut |payload| {
            assert_eq!(payload["args"]["packAllData"], true);
            assert_eq!(payload["args"]["mcp_save"], true);
            Ok(json!({"path":"x".repeat(6000)}))
        })
        .unwrap();
        assert_eq!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            TEXT_LIMIT
        );
    }

    #[test]
    fn mcp_save_caps_file_without_changing_live_scene_and_full_saves_preserve_it() {
        use crate::app::{
            agent::{Inbound, Request},
            App,
        };
        use crate::{document, model};
        let mut app = App::new();
        app.new_document();
        let objects = (0..300)
            .map(|_| model::SdfObject::create_kind(model::PrimitiveKind::Sphere))
            .collect();
        app.tree.set_path(
            "scene.sdf_objects",
            model::ClaydashValue::VecSDFObject(objects),
        );
        let path = std::env::temp_dir().join(format!(
            "claydash-token-cap-{}.claydash",
            uuid::Uuid::new_v4()
        ));
        for (mcp_save, pack_all_data, expected) in
            [(true, false, 256), (true, true, 300), (false, false, 300)]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            app.process_agent_request(Inbound {
                request: Request::Save {
                    path: Some(path.clone()),
                    pack_all_data,
                    mcp_save,
                },
                reply: tx,
            });
            let response = rx.recv().unwrap().unwrap();
            assert_eq!(response["collections_trimmed"], expected == 256);
            assert_eq!(app.document.is_dirty(), expected == 256);
            let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                saved["subtree"]["sdf_objects"]["value"]["VecSDFObject"]
                    .as_array()
                    .unwrap()
                    .len(),
                expected
            );
            assert_eq!(model::objects_ref(&app.tree).len(), 300);
            if expected == 300 {
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    document::serialize_scene(&app.project_tree_for_save()).unwrap()
                );
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}
