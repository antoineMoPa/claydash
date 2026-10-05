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
        PrimitiveKind::PolygonPrism => {
            egui::include_image!("../../assets/icons/lucide/pentagon.svg")
        }
        PrimitiveKind::BezierCurve => egui::include_image!("../../assets/icons/lucide/tangent.svg"),
        PrimitiveKind::Loft => egui::include_image!("../../assets/icons/lucide/scan.svg"),
        PrimitiveKind::Text => egui::include_image!("../../assets/icons/lucide/type.svg"),
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
        .fill(Color32::from_black_alpha(165))
        .selected(selected)
        .stroke(Stroke::new(
            1.5,
            if selected {
                Color32::WHITE
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

pub(super) fn request_material_preview(
    ui: &egui::Ui,
    rect: egui::Rect,
    material: Material,
    asset_id: Option<uuid::Uuid>,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let request = crate::renderer::MaterialPreviewRequest { material, asset_id };
    ui.ctx().data_mut(|data| {
        let requests = data.get_temp_mut_or_default::<crate::renderer::MaterialPreviewRequests>(
            crate::renderer::MaterialPreviewRequests::egui_id(),
        );
        if !requests.0.contains(&request) {
            requests.0.push(request);
        }
    });
}

pub(super) fn material_preview_loading(ui: &egui::Ui, rect: egui::Rect, material: Material) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let error = ui.ctx().data(|data| {
        data.get_temp::<crate::renderer::MaterialPreviewIds>(
            crate::renderer::MaterialPreviewIds::egui_id(),
        )
        .and_then(|ids| ids.error_for(material).map(str::to_owned))
    });
    if let Some(error) = error {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "!",
            egui::FontId::proportional(16.0),
            ui.visuals().error_fg_color,
        );
        ui.interact(
            rect,
            ui.id()
                .with(("preview-error", rect.min.x.to_bits(), rect.min.y.to_bits())),
            egui::Sense::hover(),
        )
        .on_hover_text(error);
    } else {
        egui::Spinner::new().paint_at(
            ui,
            egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
        );
    }
}

pub(super) fn material_preview(
    ui: &mut egui::Ui,
    label: &str,
    material: Material,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(82.0, 82.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let highlighted = response.hovered() || response.has_focus();
    ui.painter().rect(
        rect,
        5.0,
        if highlighted {
            visuals.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        },
        if highlighted {
            visuals.bg_stroke
        } else {
            Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    let preview = egui::Rect::from_min_max(
        rect.min + egui::vec2(7.0, 6.0),
        egui::pos2(rect.max.x - 7.0, rect.max.y - 32.0),
    );
    request_material_preview(ui, preview, material, None);
    if let Some(texture) = ui.ctx().data(|data| {
        data.get_temp::<crate::renderer::MaterialPreviewIds>(
            crate::renderer::MaterialPreviewIds::egui_id(),
        )
        .and_then(|ids| ids.for_material(material))
    }) {
        ui.painter().image(
            texture,
            preview,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        material_preview_loading(ui, preview, material);
    }
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 4.0, rect.max.y - 29.0),
        egui::pos2(rect.max.x - 4.0, rect.max.y - 3.0),
    );
    let mut job = egui::text::LayoutJob::simple(
        label.to_owned(),
        egui::FontId::proportional(11.0),
        visuals.text_color(),
        label_rect.width(),
    );
    job.halign = egui::Align::Center;
    job.wrap.max_rows = 2;
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().with_clip_rect(label_rect).galley(
        egui::pos2(
            // Center-aligned galleys already span both sides of their origin.
            label_rect.center().x,
            label_rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        visuals.text_color(),
    );
    response.on_hover_text(label)
}
