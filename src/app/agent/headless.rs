//! Window-free host for the existing agent operations and renderer.
use super::*;
use crate::renderer::{HeadlessOptions, Renderer};
use std::time::Instant;

const USAGE: &str = "usage: claydash serve [--size WIDTHxHEIGHT] [--scene PATH]\n       claydash mcp --headless [--size WIDTHxHEIGHT] [--scene PATH]\n\nserve shares a scene with agent/mcp clients over the local Unix socket.\nmcp --headless owns a private scene for the lifetime of its stdio connection.\nBoth use an offscreen GPU renderer; no window or desktop event loop is created.";

pub(super) struct Options {
    renderer: HeadlessOptions,
    scene: Option<PathBuf>,
}

impl Options {
    pub(super) fn parse(mode: &str, args: &[String]) -> Result<Option<Self>, String> {
        if args.iter().any(|arg| arg == "--help" || arg == "-h") {
            println!("{USAGE}");
            return Ok(None);
        }
        let mut options = Self {
            renderer: HeadlessOptions::default(),
            scene: None,
        };
        let mut headless = mode != "mcp";
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--headless" if mode == "mcp" => headless = true,
                "--size" => {
                    let size = args.next().ok_or("--size requires WIDTHxHEIGHT")?;
                    let (width, height) =
                        size.split_once('x').ok_or("--size requires WIDTHxHEIGHT")?;
                    options.renderer.width = width.parse().map_err(|_| "invalid width")?;
                    options.renderer.height = height.parse().map_err(|_| "invalid height")?;
                    if options.renderer.width == 0 || options.renderer.height == 0 {
                        return Err("render dimensions must be nonzero".into());
                    }
                }
                "--scene" => {
                    options.scene =
                        Some(PathBuf::from(args.next().ok_or("--scene requires a path")?))
                }
                _ => return Err(format!("unknown option: {arg}\n{USAGE}")),
            }
        }
        if !headless {
            return Err(format!("headless options require mcp --headless\n{USAGE}"));
        }
        Ok(Some(options))
    }
}

pub(super) fn run(options: Options, private_mcp: bool) -> Result<(), String> {
    let mut app = App::new();
    if let Some(path) = options.scene {
        let scene = document::read_scene(&path)?;
        app.replace_scene(scene);
        app.document.mark_opened(path);
        app.document.mark_scene_clean(&app.tree);
    } else {
        app.new_document();
    }
    app.renderer = Some(pollster::block_on(Renderer::new_headless(
        options.renderer,
    ))?);
    app.agent_redraw_pending = true;
    app.camera.viewport = app.renderer.as_ref().unwrap().size();
    app.camera.viewport_origin = Vec2::ZERO;
    let requests = if private_mcp {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            run_mcp(|payload| {
                let request = serde_json::from_value(payload).map_err(|error| error.to_string())?;
                let (reply, response) = mpsc::channel();
                tx.send(Inbound { request, reply })
                    .map_err(|_| "Claydash is closing")?;
                response
                    .recv_timeout(Duration::from_secs(115))
                    .map_err(|error| error.to_string())?
            })
        });
        rx
    } else {
        let requests = listen(None)?;
        eprintln!("Claydash server listening on {}", socket_path()?.display());
        requests
    };
    run_requests(&mut app, requests);
    app.cancel_document_work();
    Ok(())
}

fn run_requests(app: &mut App, requests: Receiver<Inbound>) {
    let mut busy = true;
    let mut capture_started = None;
    loop {
        // Sleep until a request when idle; tick only while GPU/CPU work is pending.
        let inbound = if busy {
            match requests.recv_timeout(Duration::from_millis(10)) {
                Ok(request) => Some(request),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match requests.recv() {
                Ok(request) => Some(request),
                Err(_) => break,
            }
        };
        if let Some(inbound) = inbound {
            app.process_agent_request(inbound);
        }
        if app.agent_redraw_pending || app.agent_capture.is_some() {
            let error = app
                .renderer
                .as_mut()
                .unwrap()
                .sync_custom_materials(&model::material_assets(&app.tree))
                .err();
            app.egui.data_mut(|data| {
                data.insert_temp(egui::Id::new("custom-material-render-error"), error)
            });
        }
        if app.agent_capture.is_some() {
            let started = capture_started.get_or_insert_with(Instant::now);
            if started.elapsed() >= Duration::from_secs(110) {
                let capture = app.agent_capture.take().unwrap();
                app.renderer.as_mut().unwrap().discard_pending_capture();
                let _ = capture
                    .reply
                    .send(Err("offscreen capture timed out".into()));
            } else {
                app.render_agent_offscreen();
            }
        }
        if app.agent_capture.is_none() {
            capture_started = None;
        }
        // Edits also trigger automatic representations (voxels/depth atlases).
        if app.agent_redraw_pending {
            let objects = model::objects_ref(&app.tree);
            app.renderer.as_mut().unwrap().advance_computations(
                &app.camera,
                objects,
                &[],
                super::super::rendering::scene_render_versions(&app.tree, false),
                super::super::rendering::mesh_source_revision(objects),
                model::world(&app.tree),
                &app.egui,
                true,
            );
            app.agent_redraw_pending = false;
        }
        busy = app.advance_background_computations() || app.agent_capture.is_some();
    }
}

impl App {
    fn render_agent_offscreen(&mut self) {
        let capture = self.agent_capture.as_ref().unwrap();
        let camera = capture.camera(&self.camera);
        let objects = capture
            .objects
            .as_deref()
            .unwrap_or_else(|| model::objects_ref(&self.tree));
        let renderer = self.renderer.as_mut().unwrap();
        renderer.poll_background_gpu();
        if !renderer.capture_pending() {
            self.egui.begin_pass(egui::RawInput::default());
            let mut output = self.egui.end_pass();
            renderer.render(
                &camera,
                objects,
                &[],
                super::super::rendering::scene_render_versions(&self.tree, true),
                super::super::rendering::mesh_source_revision(objects),
                model::world(&self.tree),
                model::post_processing_ref(&self.tree),
                &self.egui,
                &mut output,
                true,
                false,
                true,
                capture.mode == CaptureMode::FullMaterial,
                false,
                false,
                capture.mode == CaptureMode::Outline,
                false,
            );
        }
        if let Some(frame) = renderer.take_capture() {
            let mut capture = self.agent_capture.take().unwrap();
            let result = frame.and_then(|frame| capture.accept_frame(frame, &camera));
            if matches!(result, Ok(Value::Null)) {
                self.agent_capture = Some(capture);
                renderer.invalidate_scene();
            } else {
                let _ = capture.reply.send(result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(mode: &str, args: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(
            mode,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }
    #[test]
    fn server_options_validate_before_starting_runtime() {
        let options = parse(
            "mcp",
            &[
                "--headless",
                "--size",
                "800x600",
                "--scene",
                "duck.claydash",
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (options.renderer.width, options.renderer.height),
            (800, 600)
        );
        assert_eq!(options.scene.unwrap(), PathBuf::from("duck.claydash"));
        for args in [
            &["--size"][..],
            &["--size", "0x10"],
            &["--size", "2x-1"],
            &["--scene"],
            &["--typo"],
        ] {
            assert!(parse("serve", args).is_err());
        }
        assert!(parse("mcp", &["--size", "640x480"]).is_err());
        assert!(parse("--agent-headless", &[]).unwrap().is_some());
    }
}
