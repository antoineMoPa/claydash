use super::*;

#[cfg(not(target_arch = "wasm32"))]
fn guide_face_scene() -> Vec<crate::model::SdfObject> {
    use crate::model::{
        BooleanOperation, BoxParams, PolygonPrismParams, PrimitiveKind, SdfObject, SdfParams,
    };

    let mut cut_source = SdfObject::create_kind(PrimitiveKind::Box);
    cut_source.name = "Face cut source".into();
    cut_source.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::splat(0.55),
    });
    cut_source.transform.translation = glam::Vec3::new(-1.0, 0.0, 0.0);
    cut_source.softness = 0.0;

    let mut cut = SdfObject::create_kind(PrimitiveKind::PolygonPrism);
    cut.name = "Face extrusion".into();
    cut.params = SdfParams::PolygonPrismParams(PolygonPrismParams {
        vertices: vec![
            Vec2::new(-0.35, -0.3),
            Vec2::new(0.35, -0.3),
            Vec2::new(0.0, 0.35),
        ],
        half_depth: 0.3,
    });
    cut.transform.translation = glam::Vec3::new(-1.0, 0.0, 0.85);
    cut.boolean_parent = Some(cut_source.uuid);
    cut.operation = BooleanOperation::Union;

    let mut box_source = SdfObject::create_kind(PrimitiveKind::Box);
    box_source.name = "Box face source".into();
    box_source.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::splat(0.5),
    });
    box_source.transform.translation = glam::Vec3::new(1.0, -0.15, 0.0);

    let mut box_extrusion = SdfObject::create_kind(PrimitiveKind::Box);
    box_extrusion.name = "Box face extrusion".into();
    box_extrusion.params = SdfParams::BoxParams(BoxParams {
        box_q: glam::Vec3::new(0.32, 0.38, 0.5),
    });
    box_extrusion.transform.translation = glam::Vec3::new(1.0, 0.72, 0.0);

    vec![cut_source, cut, box_source, box_extrusion]
}

impl App {
    pub fn new() -> Self {
        let scene = serde_json::from_str(duck::DEFAULT_DUCK).expect("parse default scene");
        #[allow(unused_mut)]
        let mut tree = data_tree_with_scene(scene);

        #[cfg(not(target_arch = "wasm32"))]
        let brute_force_benchmark =
            std::env::args().any(|argument| argument == "--stress-benchmark-brute-force");
        #[cfg(not(target_arch = "wasm32"))]
        let benchmark = brute_force_benchmark
            || std::env::args().any(|argument| {
                argument == "--stress-benchmark" || argument == "--stress-benchmark-suite"
            });
        #[cfg(not(target_arch = "wasm32"))]
        if benchmark || std::env::args().any(|arg| arg == "--stress-ui-benchmark") {
            if let Some(path) = std::env::args()
                .find_map(|arg| arg.strip_prefix("--benchmark-scene=").map(str::to_owned))
            {
                let scene = crate::document::read_scene(std::path::Path::new(&path))
                    .unwrap_or_else(|error| panic!("read benchmark scene {path}: {error}"));
                tree.set_tree("scene", scene);
            } else {
                let case = std::env::args()
                    .find_map(|arg| arg.strip_prefix("--benchmark-case=").map(str::to_owned));
                let scene = case
                    .and_then(|case| {
                        crate::model::renderer_benchmark_scenes()
                            .into_iter()
                            .find(|(name, _)| *name == case)
                            .map(|(_, scene)| scene)
                    })
                    .unwrap_or_else(crate::model::renderer_stress_scene);
                crate::model::set_objects(&mut tree, scene);
            }
            crate::model::set_selected(&mut tree, Vec::new());
        }
        let camera = Camera::new();
        #[cfg(not(target_arch = "wasm32"))]
        let camera = if std::env::args().any(|arg| arg == "--benchmark-orthographic") {
            Camera {
                projection_mode: crate::camera::ProjectionMode::Orthographic,
                ..camera
            }
        } else {
            camera
        };
        #[cfg(not(target_arch = "wasm32"))]
        let camera = if std::env::args().any(|argument| argument == "--ui-preview") {
            let face_preview =
                std::env::args().any(|argument| argument == "--guide-panel=face-cut");
            let repeat_preview =
                std::env::args().any(|argument| argument == "--guide-panel=repeat");
            let mirror_preview =
                std::env::args().any(|argument| argument == "--guide-panel=mirror");
            let simple_kind = std::env::args().find_map(|argument| match argument.as_str() {
                "--guide-panel=shapes" => Some(crate::model::PrimitiveKind::Box),
                "--guide-panel=gizmos" => Some(crate::model::PrimitiveKind::Cylinder),
                "--guide-panel=modifiers" => Some(crate::model::PrimitiveKind::Sphere),
                _ => None,
            });
            let mut scene = if mirror_preview {
                let mut center =
                    crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
                center.name = "Mirrored group".into();
                center.params = crate::model::SdfParams::BoxParams(crate::model::BoxParams {
                    box_q: glam::Vec3::new(0.2, 0.28, 0.28),
                });
                center.mirror = Some(crate::model::Mirror::default());
                let mut side =
                    crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
                side.name = "Side sphere".into();
                side.params = crate::model::SdfParams::SphereParams(crate::model::SphereParams {
                    radius: 0.42,
                });
                side.transform.translation.x = 0.8;
                side.boolean_parent = Some(center.uuid);
                vec![center, side]
            } else if repeat_preview {
                let duck: DataTree =
                    serde_json::from_str(duck::DEFAULT_DUCK).expect("parse default duck preview");
                let mut scene = crate::model::objects(&data_tree_with_scene(duck));
                let root = scene[0].uuid;
                scene.retain(|object| object.uuid == root || object.boolean_parent == Some(root));
                scene
            } else if face_preview {
                guide_face_scene()
            } else if let Some(kind) = simple_kind {
                let mut shape = crate::model::SdfObject::create_kind(kind);
                shape.name = format!("{} example", kind.label());
                shape.params = match kind {
                    crate::model::PrimitiveKind::Box => {
                        crate::model::SdfParams::BoxParams(crate::model::BoxParams {
                            box_q: glam::Vec3::splat(0.55),
                        })
                    }
                    crate::model::PrimitiveKind::Cylinder => {
                        crate::model::SdfParams::CylinderParams {
                            radius: 0.45,
                            half_height: 0.65,
                        }
                    }
                    crate::model::PrimitiveKind::Sphere => {
                        crate::model::SdfParams::SphereParams(crate::model::SphereParams {
                            radius: 0.5,
                        })
                    }
                    _ => shape.params,
                };
                vec![shape]
            } else {
                crate::model::ui_preview_scene()
            };
            let operand_preview =
                std::env::args().any(|argument| argument == "--guide-panel=operand");
            if std::env::args().any(|argument| argument == "--guide-panel=modifiers") {
                if let Some((min, max)) = crate::model::lattice_bounds(&scene, scene[0].uuid) {
                    scene[0].lattice = Some(crate::model::Lattice::new(min, max, 3));
                }
            }
            if repeat_preview {
                let (min, max) =
                    crate::model::lattice_bounds(&scene, scene[0].uuid).expect("duck bounds");
                scene[0].repetition.enabled = true;
                scene[0].repetition.spacing = (max - min) * 1.1;
            }
            let selected = scene[if face_preview {
                1
            } else if operand_preview {
                7
            } else if simple_kind.is_some() || repeat_preview || mirror_preview {
                0
            } else {
                2
            }]
            .uuid;
            if face_preview {
                crate::model::set_selected_exact(&mut tree, vec![selected]);
            } else {
                crate::model::set_selected(&mut tree, vec![selected]);
            }
            crate::model::set_objects(&mut tree, scene);
            if face_preview {
                crate::model::set_selected_modeling_face(
                    &mut tree,
                    Some(crate::model::ModelingFaceSelection::PolygonPrism(
                        crate::model::PolygonPrismFaceSelection {
                            object: selected,
                            face: crate::model::PolygonPrismFace::Cap { positive: true },
                        },
                    )),
                );
            }
            if std::env::args().any(|argument| argument == "--guide-panel=animation") {
                let binding = crate::model::AnimationBinding {
                    object: selected,
                    property: crate::model::AnimatableProperty::Position(
                        crate::model::VectorAxis::X,
                    ),
                };
                crate::animation::insert_keyframe(&mut tree, binding, 0, 1.35);
                crate::animation::insert_keyframe(&mut tree, binding, 30, -1.35);
            }
            tree.make_undo_redo_snapshot();
            Camera {
                position: if simple_kind.is_some() || mirror_preview {
                    glam::Vec3::new(0.0, 1.3, 4.5)
                } else {
                    glam::Vec3::new(0.0, 1.6, 8.5)
                },
                ..camera
            }
        } else {
            camera
        };
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::args().any(|argument| argument == "--guide-panel=camera") {
            let scene_camera = crate::camera::SceneCamera::from_view("Camera 1", &camera);
            crate::model::set_selected(&mut tree, vec![scene_camera.uuid]);
            crate::model::set_scene_cameras(&mut tree, vec![scene_camera]);
        }
        let mut commands = Commands::new();
        commands::register_all(&mut commands);
        #[cfg(target_arch = "wasm32")]
        let (renderer_tx, renderer_rx) = channel();
        #[cfg(target_arch = "wasm32")]
        let (document_tx, document_rx) = channel();
        let document = DocumentState::default();
        let egui = egui::Context::default();
        egui.style_mut_of(egui::Theme::Light, |style| {
            let widgets = &mut style.visuals.widgets;
            widgets.inactive.weak_bg_fill = egui::Color32::WHITE;
            widgets.inactive.bg_fill = egui::Color32::WHITE;
            widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_rgb(205, 212, 224));
            widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(231, 239, 251);
            widgets.hovered.bg_fill = widgets.hovered.weak_bg_fill;
            widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_rgb(136, 166, 216));
            widgets.active.weak_bg_fill = egui::Color32::from_rgb(211, 226, 249);
            widgets.active.bg_fill = widgets.active.weak_bg_fill;
            widgets.open.weak_bg_fill = egui::Color32::from_rgb(231, 239, 251);
            widgets.open.bg_fill = widgets.open.weak_bg_fill;
            for widget in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
                &mut widgets.open,
            ] {
                widget.corner_radius = egui::CornerRadius::same(5);
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        let guide_theme = std::env::args().find_map(|argument| match argument.as_str() {
            "--guide-theme=dark" => Some(ColorTheme::Dark),
            "--guide-theme=light" => Some(ColorTheme::Light),
            _ => None,
        });
        #[cfg(target_arch = "wasm32")]
        let guide_theme: Option<ColorTheme> = None;
        egui.set_theme(
            guide_theme
                .unwrap_or_else(|| document.color_theme())
                .egui_theme(),
        );
        let mut ui = UiState::default();
        ui.set_animation_timeline_open(document.animation_timeline_open());
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::args().any(|argument| argument.starts_with("--guide-screenshot=")) {
            let show_timeline = !std::env::args().any(|argument| {
                argument.starts_with("--guide-panel=") && argument != "--guide-panel=animation"
            });
            ui.set_animation_timeline_open(show_timeline);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(panel) = std::env::args()
            .find_map(|argument| argument.strip_prefix("--guide-panel=").map(str::to_owned))
        {
            ui.focus_guide_panel(&panel);
        }
        Self {
            window: None,
            renderer: None,
            egui,
            #[cfg(not(target_arch = "wasm32"))]
            egui_state: None,
            #[cfg(target_arch = "wasm32")]
            egui_state: crate::web_input::WebInput::default(),
            tree,
            commands,
            camera,
            interactions: InteractionState::default(),
            ui,
            document,
            #[cfg(all(not(target_arch = "wasm32"), unix))]
            agent_requests: None,
            #[cfg(all(not(target_arch = "wasm32"), unix))]
            agent_capture: None,
            #[cfg(all(not(target_arch = "wasm32"), unix))]
            agent_revision: 0,
            #[cfg(not(target_arch = "wasm32"))]
            pending_render: None,
            #[cfg(not(target_arch = "wasm32"))]
            encoding: None,
            #[cfg(target_arch = "wasm32")]
            pending_render: None,
            #[cfg(target_arch = "wasm32")]
            encoding: None,
            #[cfg(target_arch = "wasm32")]
            next_render_id: 0,
            discard_capture: false,
            #[cfg(not(target_arch = "wasm32"))]
            guide_screenshot: std::env::args().find_map(|argument| {
                argument
                    .strip_prefix("--guide-screenshot=")
                    .map(std::path::PathBuf::from)
            }),
            #[cfg(not(target_arch = "wasm32"))]
            guide_capture_done: false,
            #[cfg(all(not(target_arch = "wasm32"), unix))]
            agent_headless: std::env::args().any(|argument| argument == "--agent-headless"),
            window_focused: true,
            window_occluded: false,
            #[cfg(not(target_arch = "wasm32"))]
            use_bvh: !brute_force_benchmark,
            #[cfg(not(target_arch = "wasm32"))]
            benchmark,
            #[cfg(not(target_arch = "wasm32"))]
            ui_benchmark: std::env::args()
                .any(|arg| arg == "--stress-ui-benchmark")
                .then(|| UiBenchmark {
                    edit_objects: std::env::args().any(|arg| arg == "--benchmark-edit"),
                    selection_click: std::env::args()
                        .any(|arg| arg == "--benchmark-selection-click"),
                    preview_pixels: Vec::new(),
                    frames: 0,
                    last_frame: None,
                    samples: Vec::new(),
                }),
            #[cfg(target_arch = "wasm32")]
            renderer_tx,
            #[cfg(target_arch = "wasm32")]
            renderer_rx,
            #[cfg(target_arch = "wasm32")]
            document_tx,
            #[cfg(target_arch = "wasm32")]
            document_rx,
        }
    }
}

pub(super) fn data_tree_with_scene(scene: DataTree) -> DataTree {
    let mut tree = ObservableKVTree::default();
    tree.set_tree("scene", scene);
    tree.set_path(
        "editor.state",
        ClaydashValue::EditorState(EditorState::Start),
    );
    tree.set_path(
        "editor.color",
        ClaydashValue::Vec4(Vec4::new(0.8, 0.0, 0.3, 1.0)),
    );
    tree.make_undo_redo_snapshot();
    tree
}
