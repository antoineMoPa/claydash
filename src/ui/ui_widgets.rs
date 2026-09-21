use super::*;

pub(super) fn orientation_axes(ui: &mut egui::Ui, tree: &mut DataTree, camera: &mut Camera) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 82.0), egui::Sense::hover());
    let center = rect.center();
    let view = camera.view();
    let axes = [
        (Vec3::X, "X", axis_color(0), ViewAngle::Right),
        (Vec3::NEG_X, "", axis_color(0), ViewAngle::Left),
        (Vec3::Y, "Y", axis_color(1), ViewAngle::Top),
        (Vec3::NEG_Y, "", axis_color(1), ViewAngle::Bottom),
        (Vec3::Z, "Z", axis_color(2), ViewAngle::Front),
        (Vec3::NEG_Z, "", axis_color(2), ViewAngle::Back),
    ];
    for (axis, label, color, angle) in axes {
        let direction = view.transform_vector3(axis);
        let end = center + egui::vec2(direction.x, -direction.y) * 31.0;
        let hit = egui::Rect::from_center_size(end, egui::vec2(18.0, 18.0));
        let response = ui.interact(
            hit,
            egui::Id::new(("view-axis", label, angle)),
            egui::Sense::click(),
        );
        let muted = color.gamma_multiply(if direction.z < 0.0 { 0.42 } else { 1.0 });
        ui.painter()
            .line_segment([center, end], Stroke::new(2.0, muted));
        ui.painter()
            .circle_filled(end, if response.hovered() { 7.0 } else { 5.0 }, muted);
        if !label.is_empty() {
            ui.painter().text(
                end,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(9.0),
                Color32::BLACK,
            );
        }
        if response.clicked() {
            exit_camera_view(tree);
            camera.snap(angle);
        }
    }
}

pub(super) fn primitive_icon(ui: &mut egui::Ui, kind: PrimitiveKind) {
    ui.add(icon_image(
        primitive_icon_source(kind),
        ui.visuals().text_color(),
    ));
}

pub(super) fn primitive_icon_source(kind: PrimitiveKind) -> egui::ImageSource<'static> {
    match kind {
        PrimitiveKind::Sphere => egui::include_image!("../../assets/icons/lucide/globe.svg"),
        PrimitiveKind::Box => egui::include_image!("../../assets/icons/lucide/box.svg"),
        PrimitiveKind::Cylinder => egui::include_image!("../../assets/icons/lucide/cylinder.svg"),
        PrimitiveKind::Torus => egui::include_image!("../../assets/icons/lucide/torus.svg"),
    }
}

pub(super) fn icon_image(
    source: egui::ImageSource<'static>,
    tint: Color32,
) -> egui::Image<'static> {
    egui::Image::new(source)
        .fit_to_exact_size(egui::vec2(18.0, 18.0))
        .tint(tint)
}

pub(super) fn view_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tooltip: &str,
) -> egui::Response {
    selectable_view_button(ui, source, tooltip, false)
}

pub(super) fn selectable_view_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tooltip: &str,
    selected: bool,
) -> egui::Response {
    ui.scope(|ui| {
        style_view_buttons(ui);
        ui.add(view_icon_button(source, selected))
    })
    .inner
    .on_hover_text(tooltip)
}

// egui derives frame margins from theme strokes before applying Button::stroke.
// Keep those inputs fixed so hover/focus cannot change the allocated size.
pub(super) fn style_view_buttons(ui: &mut egui::Ui) {
    ui.spacing_mut().button_padding = egui::vec2(3.0, 3.0);
    let widgets = &mut ui.style_mut().visuals.widgets;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.bg_stroke.width = 1.5;
        state.expansion = 0.0;
    }
}

pub(super) fn view_icon_button(
    source: egui::ImageSource<'static>,
    selected: bool,
) -> egui::Button<'static> {
    egui::Button::image(icon_image(source, Color32::WHITE))
        .fill(if selected {
            Color32::from_rgba_unmultiplied(151, 73, 235, 155)
        } else {
            Color32::from_black_alpha(165)
        })
        .selected(selected)
        .stroke(Stroke::new(
            1.5,
            if selected {
                Color32::from_rgb(232, 183, 255)
            } else {
                Color32::TRANSPARENT
            },
        ))
        .corner_radius(CornerRadius::same(255))
        .min_size(egui::vec2(26.0, 26.0))
}

pub(super) fn axis_label(axis: usize) -> &'static str {
    match axis {
        0 => "X",
        1 => "Y",
        _ => "Z",
    }
}

pub(super) fn axis_color(axis: usize) -> Color32 {
    match axis {
        0 => Color32::from_rgb(244, 88, 91),
        1 => Color32::from_rgb(91, 218, 119),
        _ => Color32::from_rgb(86, 149, 255),
    }
}

pub(super) fn color32(color: Vec4) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.x * 255.0) as u8,
        (color.y * 255.0) as u8,
        (color.z * 255.0) as u8,
        (color.w * 255.0) as u8,
    )
}

pub(super) fn material_preview(
    ui: &mut egui::Ui,
    kind: MaterialKind,
    material: Material,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(82.0, 70.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect(
        rect,
        5.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let preview = egui::Rect::from_min_max(
        rect.min + egui::vec2(7.0, 6.0),
        egui::pos2(rect.max.x - 7.0, rect.max.y - 22.0),
    );
    if kind == MaterialKind::Transparent {
        let size = 8.0;
        for row in 0..5 {
            for column in 0..9 {
                let tile = egui::Rect::from_min_size(
                    preview.min + egui::vec2(column as f32 * size, row as f32 * size),
                    egui::vec2(size, size),
                )
                .intersect(preview);
                let fill = if (row + column) % 2 == 0 {
                    Color32::from_gray(75)
                } else {
                    Color32::from_gray(42)
                };
                ui.painter().rect_filled(tile, 0.0, fill);
            }
        }
    }
    let center = preview.center();
    let radius = preview.height().min(preview.width()) * 0.37;
    ui.painter()
        .circle_filled(center, radius, color32(material.color));
    if kind == MaterialKind::Wood {
        for offset in -5..=5 {
            let x = offset as f32 * radius / 6.0;
            let height = (radius * radius - x * x).sqrt();
            ui.painter().line_segment(
                [
                    center + egui::vec2(x, -height),
                    center + egui::vec2(x, height),
                ],
                Stroke::new(1.2, Color32::from_black_alpha(65)),
            );
        }
    }
    ui.painter().circle_filled(
        center - egui::vec2(radius * 0.28, radius * 0.32),
        radius * (0.18 + material.reflectivity * 0.12),
        Color32::from_white_alpha((100.0 + material.reflectivity * 155.0) as u8),
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.max.y - 10.0),
        egui::Align2::CENTER_CENTER,
        kind.label(),
        egui::FontId::proportional(11.0),
        visuals.text_color(),
    );
    response
}
