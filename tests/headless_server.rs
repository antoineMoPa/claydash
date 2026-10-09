#![cfg(unix)]
//! Real process tests, opt-in because they require a wgpu adapter.
use base64::Engine;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("claydash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_claydash"));
    // On Linux, any accidental desktop event loop fails without a display.
    command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    command
}
fn output_lines(child: &mut Child) -> Receiver<Value> {
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let value =
                serde_json::from_str(&line.unwrap()).expect("stdout must contain only JSON-RPC");
            if tx.send(value).is_err() {
                break;
            }
        }
    });
    rx
}
fn rpc(child: &mut Child, lines: &Receiver<Value>, method: &str, params: Value) -> Value {
    writeln!(
        child.stdin.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params})
    )
    .unwrap();
    let response = lines
        .recv_timeout(Duration::from_secs(120))
        .expect("MCP response deadline");
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}
fn tool(child: &mut Child, lines: &Receiver<Value>, name: &str, arguments: Value) -> Value {
    let result = rpc(
        child,
        lines,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    );
    assert_ne!(result["isError"], true, "{result}");
    if let Some(text) = result["content"][0]["text"].as_str() {
        serde_json::from_str(text).unwrap()
    } else {
        result
    }
}
fn assert_png(result: &Value, width: u32, height: u32) {
    let encoded = result["content"][0]["data"].as_str().unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (width, height));
    assert!(
        pixels.chunks_exact(4).any(|pixel| pixel != &pixels[..4]),
        "capture should contain scene detail"
    );
}

#[test]
#[ignore = "requires a GPU adapter"]
fn private_mcp_edits_captures_saves_and_exits_on_eof() {
    let directory = Directory::new();
    let socket = directory.0.join("unused.sock");
    let mut process = Process(
        command()
            .args(["mcp", "--headless", "--size", "128x128"])
            .env("CLAYDASH_AGENT_SOCKET", &socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let lines = output_lines(&mut process.0);
    let init = rpc(&mut process.0, &lines, "initialize", json!({}));
    assert_eq!(init["serverInfo"]["name"], "claydash");
    let tools = rpc(&mut process.0, &lines, "tools/list", json!({}));
    assert!(tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "capture_viewport"));
    let before = tool(&mut process.0, &lines, "get_state", json!({}));
    let count = before["objects"].as_array().unwrap().len();
    assert_eq!(
        count, 0,
        "new headless sessions must start with an empty document"
    );
    assert_eq!(before["selection"], json!([]));
    assert!(before["document_path"].is_null());
    assert_eq!(before["world"]["background"], "Studio");
    assert_eq!(before["world"]["ambient_light"], 1.0);
    let edit = tool(
        &mut process.0,
        &lines,
        "apply",
        json!({"expected_revision":before["revision"], "actions":[{"type":"CreateObject", "kind":"Sphere", "name":"Headless sphere"}]}),
    );
    let id = edit["created_ids"][0].clone();
    let state = tool(&mut process.0, &lines, "get_state", json!({}));
    assert_eq!(state["objects"].as_array().unwrap().len(), count + 1);
    let stale = rpc(
        &mut process.0,
        &lines,
        "tools/call",
        json!({"name":"apply", "arguments":{"expected_revision":before["revision"], "actions":[]}}),
    );
    assert_eq!(stale["isError"], true);
    tool(&mut process.0, &lines, "undo", json!({}));
    assert_eq!(
        tool(&mut process.0, &lines, "get_state", json!({}))["objects"]
            .as_array()
            .unwrap()
            .len(),
        count
    );
    tool(&mut process.0, &lines, "redo", json!({}));
    for mode in ["simple_shading", "full_material", "outline"] {
        let image = tool(
            &mut process.0,
            &lines,
            "capture_viewport",
            json!({"mode":mode}),
        );
        assert_png(&image, 128, 128);
    }
    let image = tool(
        &mut process.0,
        &lines,
        "capture_orthographic",
        json!({"panel_size":96}),
    );
    assert_png(&image, 288, 120);
    let path = directory.0.join("saved.claydash");
    tool(&mut process.0, &lines, "save", json!({"path":path}));
    tool(&mut process.0, &lines, "open", json!({"path":path}));
    assert_eq!(
        tool(&mut process.0, &lines, "get_state", json!({}))["objects"]
            .as_array()
            .unwrap()
            .len(),
        count + 1
    );
    assert!(id.is_string(), "{edit}");
    let state = tool(&mut process.0, &lines, "get_state", json!({}));
    let mut sphere = state["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["uuid"] == id)
        .unwrap()
        .clone();
    sphere["render_representation"] = json!("poisson_mesh");
    sphere["gaussian_splats"] = json!({"resolution":16});
    sphere["poisson_mesh"] = json!({"resolution":16});
    tool(
        &mut process.0,
        &lines,
        "apply",
        json!({"actions":[{"type":"PutObject", "object":sphere}]}),
    );
    tool(
        &mut process.0,
        &lines,
        "build_poisson_mesh",
        json!({"id":id}),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = tool(
            &mut process.0,
            &lines,
            "get_poisson_mesh_status",
            json!({"id":id}),
        );
        if status["status"] == "ready" {
            assert!(status["triangles"].as_u64().unwrap() > 0);
            break;
        }
        assert_ne!(status["status"], "failed", "{status}");
        assert!(
            Instant::now() < deadline,
            "background mesh deadline: {status}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let subset = tool(
        &mut process.0,
        &lines,
        "capture_viewport",
        json!({"object_ids":[id]}),
    );
    assert_png(&subset, 128, 128);
    tool(
        &mut process.0,
        &lines,
        "apply",
        json!({"actions":[{
            "type":"SetRenderRepresentation", "id":id, "render_representation":"voxels", "voxels":{"resolution":8}
        }]}),
    );
    for geometry in ["current_representation", "smooth", "voxels"] {
        let output = directory.0.join(format!("{geometry}.glb"));
        let mut args = json!({"path":output,"geometry":geometry,"object_ids":[id]});
        if geometry == "voxels" {
            args["voxel_resolution"] = json!(8);
        }
        let export = tool(&mut process.0, &lines, "export_glb", args);
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let status = tool(
                &mut process.0,
                &lines,
                "get_glb_export_status",
                json!({"id":export["id"]}),
            );
            if status["status"] == "completed" {
                break;
            }
            assert_eq!(status["status"], "running", "{status}");
            assert!(Instant::now() < deadline, "GLB export deadline: {status}");
            std::thread::sleep(Duration::from_millis(20));
        }
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 1);
        let attribute = if geometry == "smooth" {
            "TEXCOORD_0"
        } else {
            "COLOR_0"
        };
        assert!(doc["meshes"][0]["primitives"][0]["attributes"][attribute].is_number());
        assert_eq!(
            tool(
                &mut process.0,
                &lines,
                "cancel_glb_export",
                json!({"id":export["id"]})
            )["status"],
            "completed"
        );
    }
    assert!(
        !socket.exists(),
        "private MCP must not bind the shared socket"
    );
    drop(process.0.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "MCP must exit on stdin EOF");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn server_accepts_cli_and_mcp_and_rejects_another_owner() {
    let directory = Directory::new();
    let socket = directory.0.join("agent.sock");
    let parent_permissions = std::fs::metadata(&directory.0)
        .unwrap()
        .permissions()
        .mode();
    // Start with a stale socket to exercise recovery after process termination.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    let mut server = Process(
        command()
            .args([
                "--headless",
                "--size",
                "128x128",
                "--scene",
                "duck.claydash",
            ])
            .env("CLAYDASH_AGENT_SOCKET", &socket)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while UnixStream::connect(&socket).is_err() {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited before binding"
        );
        assert!(Instant::now() < deadline, "server readiness deadline");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&directory.0)
            .unwrap()
            .permissions()
            .mode(),
        parent_permissions
    );
    let state = command()
        .args(["agent", "GetState"])
        .env("CLAYDASH_AGENT_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(
        state.status.success(),
        "{}",
        String::from_utf8_lossy(&state.stderr)
    );
    let state: Value = serde_json::from_slice(&state.stdout).unwrap();
    assert!(!state["objects"].as_array().unwrap().is_empty());
    let duplicate = command()
        .arg("serve")
        .env("CLAYDASH_AGENT_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("another Claydash instance owns"));
    let mut bridge = Process(
        command()
            .arg("mcp")
            .env("CLAYDASH_AGENT_SOCKET", &socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let lines = output_lines(&mut bridge.0);
    let result = tool(&mut bridge.0, &lines, "get_state", json!({}));
    assert_eq!(result["objects"], state["objects"]);
    assert_png(
        &tool(&mut bridge.0, &lines, "capture_viewport", json!({})),
        128,
        128,
    );
}
