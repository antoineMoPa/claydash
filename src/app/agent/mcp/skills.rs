//! Embedded workflow resources, shared by socket-backed and private headless MCP.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const EXTENSION: &str = "io.modelcontextprotocol/skills";
pub(super) const MODERN_VERSION: &str = "2026-07-28";
pub(super) const LEGACY_VERSION: &str = "2025-11-25";

struct Skill {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
    text: &'static str,
}

const SKILLS: &[Skill] = &[
    Skill {
        uri: "skill://claydash-basics/SKILL.md",
        name: "claydash-basics",
        description: "Choose primitives, materials, and render modes when creating or editing a Claydash scene through MCP.",
        text: include_str!("../../../../skills/claydash-basics/SKILL.md"),
    },
    Skill {
        uri: "skill://claydash-modeling/SKILL.md",
        name: "claydash-modeling",
        description: "Build a multi-part Claydash model with per-part placement, appearance, and assembly alignment checks.",
        text: include_str!("../../../../skills/claydash-modeling/SKILL.md"),
    },
];

impl Skill {
    fn entry(&self) -> Value {
        json!({
            "uri": self.uri,
            "frontmatter": {"name": self.name, "description": self.description},
            "resources": [{
                "uri": self.uri,
                "digest": format!("sha256:{:x}", Sha256::digest(self.text.as_bytes())),
                "size": self.text.len()
            }]
        })
    }

    fn resource(&self) -> Value {
        json!({"uri": self.uri, "name": self.name, "description": self.description, "mimeType": "text/markdown"})
    }
}

pub(super) fn capabilities() -> Value {
    json!({"tools": {}, "resources": {}, "extensions": {EXTENSION: {}}})
}

pub(super) fn cached(mut result: Value) -> Value {
    result["resultType"] = json!("complete");
    result["ttlMs"] = json!(300_000);
    result["cacheScope"] = json!("public");
    result
}

// A fixed catalog has no continuation cursor. Reject forged/stale cursors rather
// than silently repeating a page. URI lookup is exact and never accesses disk.
pub(super) fn handle(method: &str, params: &Value) -> Option<Result<Value, String>> {
    if !matches!(
        method,
        "server/discover"
            | "skills/list"
            | "skills/get"
            | "resources/list"
            | "resources/templates/list"
            | "resources/read"
    ) {
        return None;
    }
    if !params.is_null() && !params.is_object() {
        return Some(Err("params must be an object".into()));
    }
    if params.get("cursor").is_some_and(|cursor| !cursor.is_null()) {
        return Some(Err("invalid cursor: this catalog has only one page".into()));
    }
    Some(match method {
        "server/discover" => Ok(cached(json!({
            "supportedVersions": [MODERN_VERSION, LEGACY_VERSION],
            "capabilities": capabilities(),
            "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "claydash", "version": env!("CARGO_PKG_VERSION")}},
            "instructions": super::MODEL_ORGANIZATION
        }))),
        "skills/list" => Ok(cached(
            json!({"skills": SKILLS.iter().map(Skill::entry).collect::<Vec<_>>()}),
        )),
        "resources/list" => Ok(cached(
            json!({"resources": SKILLS.iter().map(Skill::resource).collect::<Vec<_>>()}),
        )),
        "resources/templates/list" => Ok(cached(json!({"resourceTemplates": []}))),
        "skills/get" | "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str);
            match SKILLS.iter().find(|skill| Some(skill.uri) == uri) {
                Some(skill) if method == "skills/get" => {
                    Ok(cached(json!({"skill": skill.entry()})))
                }
                Some(skill) => Ok(cached(
                    json!({"contents": [{"uri": skill.uri, "mimeType": "text/markdown", "text": skill.text}]}),
                )),
                None => Err("unknown or missing skill resource URI".into()),
            }
        }
        _ => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_metadata_and_manifests_match_served_bytes() {
        let list = handle("skills/list", &json!({})).unwrap().unwrap();
        let resources = handle("resources/list", &json!({})).unwrap().unwrap();
        for (index, entry) in list["skills"].as_array().unwrap().iter().enumerate() {
            let params = json!({"uri": entry["uri"]});
            let get = handle("skills/get", &params).unwrap().unwrap();
            assert_eq!(&get["skill"], entry);
            assert_eq!(resources["resources"][index]["uri"], entry["uri"]);
            let read = handle("resources/read", &params).unwrap().unwrap();
            let text = read["contents"][0]["text"].as_str().unwrap();
            assert_eq!(entry["resources"][0]["size"], text.len());
            assert_eq!(
                entry["resources"][0]["digest"],
                format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
            );
            // These skills deliberately use only two plain YAML scalar fields.
            // Catch metadata edits that would break frontmatter verification.
            let frontmatter = format!(
                "---\nname: {}\ndescription: {}\n---\n",
                entry["frontmatter"]["name"].as_str().unwrap(),
                entry["frontmatter"]["description"].as_str().unwrap()
            );
            assert!(text.starts_with(&frontmatter));
            assert!(entry["uri"].as_str().unwrap().ends_with(&format!(
                "{}/SKILL.md",
                entry["frontmatter"]["name"].as_str().unwrap()
            )));
            assert_eq!(read["cacheScope"], "public");
        }
        assert!(list.get("nextCursor").is_none());
    }

    #[test]
    fn lookup_rejects_missing_unknown_and_traversal_uris() {
        for method in ["skills/get", "resources/read"] {
            for params in [
                json!({}),
                json!({"uri": 42}),
                json!({"uri":"file:///etc/passwd"}),
                json!({"uri":"skill://claydash-basics/../claydash-modeling/SKILL.md"}),
            ] {
                assert!(handle(method, &params).unwrap().is_err());
            }
        }
        assert!(handle("skills/list", &json!({"cursor":"stale"}))
            .unwrap()
            .is_err());
        assert!(handle("tools/list", &json!({})).is_none());
        let discovery = handle("server/discover", &json!({})).unwrap().unwrap();
        assert_eq!(
            discovery["capabilities"]["extensions"][EXTENSION],
            json!({})
        );
        assert!(discovery["capabilities"]["extensions"][EXTENSION]
            .get("directoryRead")
            .is_none());
    }

    #[test]
    fn wire_dispatch_supports_both_client_generations_without_scene_access() {
        let mut no_scene = |_| panic!("discovery and skill reads must not access the scene");
        for modern in [false, true] {
            let mut params = json!({});
            if modern {
                params["_meta"] = json!({
                    "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
                    "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"},
                    "io.modelcontextprotocol/clientCapabilities": {}
                });
            }
            for method in [
                "initialize",
                "server/discover",
                "skills/list",
                "resources/list",
                "resources/templates/list",
                "tools/list",
            ] {
                let response = super::super::mcp_response(
                    &json!({"jsonrpc":"2.0", "id": "probe", "method":method, "params":params}),
                    &mut no_scene,
                )
                .unwrap();
                assert_eq!(response["id"], "probe");
                assert!(response.get("error").is_none(), "{response}");
                if modern {
                    assert_eq!(response["result"]["resultType"], "complete");
                }
            }
            params["uri"] = json!(SKILLS[0].uri);
            let response = super::super::mcp_response(
                &json!({"id":1, "method":"resources/read", "params":params}),
                &mut no_scene,
            )
            .unwrap();
            assert_eq!(response["result"]["contents"][0]["text"], SKILLS[0].text);
        }
        for method in ["skills/get", "resources/read"] {
            let response = super::super::mcp_response(
                &json!({"id":1, "method":method, "params":{"uri":"unknown"}}),
                &mut no_scene,
            )
            .unwrap();
            assert_eq!(response["error"]["code"], -32602);
        }
        let request = json!({"id":1, "method":"tools/call", "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}, "name":"apply"}});
        let response = super::super::mcp_response(&request, &mut no_scene).unwrap();
        assert_eq!(response["error"]["code"], -32022);
        assert_eq!(response["error"]["data"]["requested"], "1900-01-01");
        assert!(super::super::mcp_response(
            &json!({"method":"notifications/initialized"}),
            &mut no_scene
        )
        .is_none());
    }

    #[test]
    fn modern_tool_calls_keep_existing_scene_dispatch_and_error_content() {
        let request = json!({"id":7, "method":"tools/call", "params":{
            "name":"get_scene_summary", "arguments":{},
            "_meta":{"io.modelcontextprotocol/protocolVersion":MODERN_VERSION}
        }});
        let response = super::super::mcp_response(&request, &mut |payload| {
            assert_eq!(payload["op"], "GetSceneSummary");
            Ok(json!({"revision":42}))
        })
        .unwrap();
        assert_eq!(response["result"]["resultType"], "complete");
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap()["revision"], 42);
        let failed =
            super::super::mcp_response(&request, &mut |_| Err("scene unavailable".into())).unwrap();
        assert_eq!(failed["result"]["isError"], true);
        assert_eq!(failed["result"]["content"][0]["text"], "scene unavailable");
    }
}
