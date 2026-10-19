use std::sync::Arc;

use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::{
    camera::Camera,
    model::{self, CloudEdit, DataTree},
};
use claydash_engine::gaussian_ply::GaussianCloud;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode { #[default] Replace, Add, Subtract }

pub fn center(tree: &DataTree) -> Option<Vec3> {
    let edit = model::cloud_edit(tree)?;
    let scene = model::objects_ref(tree);
    let cloud = scene
        .iter()
        .find(|object| object.uuid == edit.object)?
        .imported_splats
        .as_ref()?;
    let matrix = model::object_world_matrix(scene, edit.object);
    let (sum, count) = edit
        .indices
        .iter()
        .filter_map(|index| cloud.splats.get(*index))
        .fold((Vec3::ZERO, 0usize), |(sum, count), splat| {
            (sum + matrix.transform_point3(splat.position), count + 1)
        });
    (count > 0).then_some(sum / count as f32)
}

pub fn pick(tree: &DataTree, camera: &Camera, pointer: Vec2) -> Option<usize> {
    let edit = model::cloud_edit(tree)?;
    let scene = model::objects_ref(tree);
    let cloud = scene
        .iter()
        .find(|object| object.uuid == edit.object)?
        .imported_splats
        .as_ref()?;
    let matrix = model::object_world_matrix(scene, edit.object);
    let mut best: Option<(usize, f32, f32)> = None;
    for (index, splat) in cloud.splats.iter().enumerate() {
        let world = matrix.transform_point3(splat.position);
        let Some(projected) = camera.project(world, 1.0) else {
            continue;
        };
        let distance = Vec2::new(projected.x, projected.y).distance(pointer);
        if distance > 12.0 {
            continue;
        }
        let depth = world.distance(camera.position);
        if best.is_none_or(|(_, old_distance, old_depth)| {
            distance < old_distance - 2.0
                || ((distance - old_distance).abs() <= 2.0 && depth < old_depth)
        }) {
            best = Some((index, distance, depth));
        }
    }
    best.map(|(index, _, _)| index)
}

pub fn select_at(tree: &mut DataTree, camera: &Camera, pointer: Vec2, additive: bool) {
    let Some(mut edit) = model::cloud_edit(tree) else {
        return;
    };
    match pick(tree, camera, pointer) {
        Some(index) if additive => {
            if edit.indices.contains(&index) {
                edit.indices.retain(|value| *value != index);
            } else {
                edit.indices.push(index);
            }
        }
        Some(index) => edit.indices = vec![index],
        None if !additive => edit.indices.clear(),
        None => {}
    }
    model::set_cloud_edit(tree, Some(edit));
}

pub fn select_rect(
    tree: &mut DataTree,
    camera: &Camera,
    rect: egui::Rect,
    scale: f32,
    mode: SelectionMode,
) {
    let Some(mut edit) = model::cloud_edit(tree) else {
        return;
    };
    let scene = model::objects_ref(tree);
    let Some(cloud) = scene
        .iter()
        .find(|object| object.uuid == edit.object)
        .and_then(|object| object.imported_splats.as_ref())
    else {
        return;
    };
    let matrix = model::object_world_matrix(scene, edit.object);
    let view_projection = camera.projection() * camera.view();
    if mode == SelectionMode::Subtract {
        edit.indices.retain(|index| cloud.splats.get(*index).is_none_or(|splat|
            !gaussian_overlaps_rect(camera, view_projection, matrix, splat, rect, scale)));
        model::set_cloud_edit(tree, Some(edit));
        return;
    }
    if mode == SelectionMode::Replace {
        edit.indices.clear();
    }
    let mut included = vec![false; cloud.splats.len()];
    for index in &edit.indices {
        if let Some(slot) = included.get_mut(*index) {
            *slot = true;
        }
    }
    for (index, splat) in cloud.splats.iter().enumerate() {
        if !included[index]
            && gaussian_overlaps_rect(camera, view_projection, matrix, splat, rect, scale)
        {
            edit.indices.push(index);
        }
    }
    model::set_cloud_edit(tree, Some(edit));
}

pub fn select_circle_stroke(
    tree: &mut DataTree,
    camera: &Camera,
    start: egui::Pos2,
    end: egui::Pos2,
    radius: f32,
    scale: f32,
    mode: SelectionMode,
) {
    let Some(mut edit) = model::cloud_edit(tree) else { return };
    let scene = model::objects_ref(tree);
    let Some(cloud) = scene.iter().find(|object| object.uuid == edit.object)
        .and_then(|object| object.imported_splats.as_ref()) else { return };
    let matrix = model::object_world_matrix(scene, edit.object);
    let view_projection = camera.projection() * camera.view();
    if mode == SelectionMode::Subtract {
        edit.indices.retain(|index| cloud.splats.get(*index).is_none_or(|splat|
            !gaussian_overlaps_capsule(camera, view_projection, matrix, splat,
                start, end, radius, scale)));
        model::set_cloud_edit(tree, Some(edit));
        return;
    }
    let mut included = vec![false; cloud.splats.len()];
    for index in &edit.indices {
        if let Some(slot) = included.get_mut(*index) { *slot = true; }
    }
    for (index, splat) in cloud.splats.iter().enumerate() {
        if !included[index] && gaussian_overlaps_capsule(camera, view_projection, matrix,
            splat, start, end, radius, scale) {
            edit.indices.push(index);
        }
    }
    model::set_cloud_edit(tree, Some(edit));
}

fn gaussian_overlaps_capsule(
    camera: &Camera,
    view_projection: Mat4,
    object_world: Mat4,
    splat: &claydash_engine::gaussian_ply::Gaussian,
    start: egui::Pos2,
    end: egui::Pos2,
    radius: f32,
    pixels_per_point: f32,
) -> bool {
    let center = view_projection * object_world.transform_point3(splat.position).extend(1.0);
    if center.w <= 0.0 { return false; }
    let ndc = center.truncate() / center.w;
    let screen = egui::pos2(
        (camera.viewport_origin.x + (ndc.x * 0.5 + 0.5) * camera.viewport.x) / pixels_per_point,
        (camera.viewport_origin.y + (0.5 - ndc.y * 0.5) * camera.viewport.y) / pixels_per_point,
    );
    let basis = Mat3::from_quat(splat.rotation);
    let axes = [basis.x_axis * splat.scale.x, basis.y_axis * splat.scale.y,
        basis.z_axis * splat.scale.z];
    let mut xx = 0.0;
    let mut xy = 0.0;
    let mut yy = 0.0;
    for axis in axes {
        let projected = view_projection * object_world.transform_vector3(axis).extend(0.0);
        let sigma = Vec2::new(
            (projected.x - ndc.x * projected.w) * camera.viewport.x / (2.0 * center.w * pixels_per_point),
            -(projected.y - ndc.y * projected.w) * camera.viewport.y / (2.0 * center.w * pixels_per_point),
        );
        xx += sigma.x * sigma.x;
        xy += sigma.x * sigma.y;
        yy += sigma.y * sigma.y;
    }
    let support = 2.5;
    let extent_x = support * xx.sqrt();
    let extent_y = support * yy.sqrt();
    if screen.x + extent_x < start.x.min(end.x) - radius
        || screen.x - extent_x > start.x.max(end.x) + radius
        || screen.y + extent_y < start.y.min(end.y) - radius
        || screen.y - extent_y > start.y.max(end.y) + radius {
        return false;
    }
    if distance_to_segment(screen, start, end) <= radius { return true; }
    let determinant = xx * yy - xy * xy;
    if determinant <= 1e-12 { return false; }
    let inside = |point: egui::Pos2| {
        let delta = point - screen;
        (yy * delta.x * delta.x - 2.0 * xy * delta.x * delta.y
            + xx * delta.y * delta.y) / determinant <= support * support
    };
    if inside(start) || inside(end) { return true; }
    let a = xx.sqrt();
    let b = xy / a;
    let c = (yy - b * b).max(0.0).sqrt();
    let boundary = |index: usize| {
        let angle = std::f32::consts::TAU * index as f32 / 32.0;
        screen + egui::vec2(
            support * a * angle.cos(),
            support * (b * angle.cos() + c * angle.sin()),
        )
    };
    let mut previous = boundary(0);
    for index in 1..=32 {
        let current = boundary(index);
        if segment_distance(start, end, previous, current) <= radius { return true; }
        previous = current;
    }
    false
}

fn distance_to_segment(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON { return point.distance(start); }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn segment_distance(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2, d: egui::Pos2) -> f32 {
    let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
    let ab = b - a;
    let cd = d - c;
    let denominator = cross(ab, cd);
    if denominator.abs() > 1e-8 {
        let ac = c - a;
        let t = cross(ac, cd) / denominator;
        let u = cross(ac, ab) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) { return 0.0; }
    }
    distance_to_segment(a, c, d).min(distance_to_segment(b, c, d))
        .min(distance_to_segment(c, a, b)).min(distance_to_segment(d, a, b))
}

fn gaussian_overlaps_rect(
    camera: &Camera,
    view_projection: Mat4,
    object_world: Mat4,
    splat: &claydash_engine::gaussian_ply::Gaussian,
    rect: egui::Rect,
    pixels_per_point: f32,
) -> bool {
    // Match the imported-splat vertex shader's projected covariance and
    // 2.5-sigma support. Work in egui points, like the drag rectangle.
    let center = view_projection * object_world.transform_point3(splat.position).extend(1.0);
    if center.w <= 0.0 {
        return false;
    }
    let ndc = center.truncate() / center.w;
    let screen = Vec2::new(
        (camera.viewport_origin.x + (ndc.x * 0.5 + 0.5) * camera.viewport.x) / pixels_per_point,
        (camera.viewport_origin.y + (0.5 - ndc.y * 0.5) * camera.viewport.y) / pixels_per_point,
    );
    let basis = Mat3::from_quat(splat.rotation);
    let axes = [
        basis.x_axis * splat.scale.x,
        basis.y_axis * splat.scale.y,
        basis.z_axis * splat.scale.z,
    ];
    let mut xx = 0.0;
    let mut xy = 0.0;
    let mut yy = 0.0;
    for axis in axes {
        let projected = view_projection * object_world.transform_vector3(axis).extend(0.0);
        let sigma = Vec2::new(
            (projected.x - ndc.x * projected.w) * camera.viewport.x / (2.0 * center.w * pixels_per_point),
            -(projected.y - ndc.y * projected.w) * camera.viewport.y / (2.0 * center.w * pixels_per_point),
        );
        xx += sigma.x * sigma.x;
        xy += sigma.x * sigma.y;
        yy += sigma.y * sigma.y;
    }
    let min_x = rect.min.x - screen.x;
    let max_x = rect.max.x - screen.x;
    let min_y = rect.min.y - screen.y;
    let max_y = rect.max.y - screen.y;
    if min_x <= 0.0 && max_x >= 0.0 && min_y <= 0.0 && max_y >= 0.0 {
        return true;
    }
    let support = 2.5;
    if min_x > support * xx.sqrt() || max_x < -support * xx.sqrt()
        || min_y > support * yy.sqrt() || max_y < -support * yy.sqrt()
    {
        return false;
    }
    let determinant = xx * yy - xy * xy;
    if determinant <= 1e-12 {
        return false;
    }
    let distance_squared = |x: f32, y: f32| (yy * x * x - 2.0 * xy * x * y + xx * y * y) / determinant;
    let closest_on_vertical = |x: f32| distance_squared(x, (xy * x / xx).clamp(min_y, max_y));
    let closest_on_horizontal = |y: f32| distance_squared((xy * y / yy).clamp(min_x, max_x), y);
    [
        closest_on_vertical(min_x),
        closest_on_vertical(max_x),
        closest_on_horizontal(min_y),
        closest_on_horizontal(max_y),
    ].into_iter().any(|distance| distance <= support * support)
}

pub fn delete_selected(tree: &mut DataTree) -> bool {
    let Some(mut edit) = model::cloud_edit(tree) else {
        return false;
    };
    if edit.indices.is_empty() {
        return true;
    }
    let mut scene = model::objects(tree);
    let Some(object) = scene.iter_mut().find(|object| object.uuid == edit.object) else {
        return false;
    };
    let Some(cloud) = object.imported_splats.as_mut() else {
        return false;
    };
    let cloud = Arc::make_mut(cloud);
    let mut selected = vec![false; cloud.splats.len()];
    for index in &edit.indices {
        if let Some(slot) = selected.get_mut(*index) {
            *slot = true;
        }
    }
    cloud.splats = cloud
        .splats
        .drain(..)
        .enumerate()
        .filter_map(|(index, splat)| (!selected[index]).then_some(splat))
        .collect();
    edit.indices.clear();
    if cloud.splats.is_empty() {
        scene.retain(|object| object.uuid != edit.object);
        model::set_objects(tree, scene);
        model::set_selected(tree, Vec::new());
        tree.make_undo_redo_snapshot();
        return true;
    }
    model::set_objects(tree, scene);
    model::set_cloud_edit(tree, Some(edit));
    tree.make_undo_redo_snapshot();
    true
}

pub fn source(tree: &DataTree) -> Option<(CloudEdit, Arc<GaussianCloud>)> {
    let edit = model::cloud_edit(tree)?;
    let cloud = model::objects_ref(tree)
        .iter()
        .find(|object| object.uuid == edit.object)?
        .imported_splats
        .clone()?;
    (!edit.indices.is_empty()).then_some((edit, cloud))
}

pub fn apply(
    tree: &mut DataTree,
    edit: &CloudEdit,
    original: &GaussianCloud,
    world_operation: Mat4,
) {
    let mut scene = model::objects(tree);
    let world = model::object_world_matrix(&scene, edit.object);
    let local_operation = world.inverse() * world_operation * world;
    let (scale, rotation, _) = local_operation.to_scale_rotation_translation();
    let Some(object) = scene.iter_mut().find(|object| object.uuid == edit.object) else {
        return;
    };
    let Some(cloud) = object.imported_splats.as_mut() else {
        return;
    };
    let cloud = Arc::make_mut(cloud);
    for index in &edit.indices {
        if let (Some(splat), Some(initial)) =
            (cloud.splats.get_mut(*index), original.splats.get(*index))
        {
            splat.position = local_operation.transform_point3(initial.position);
            splat.scale = initial.scale * scale.abs();
            splat.rotation = rotation * initial.rotation;
        }
    }
    model::set_objects(tree, scene);
}

#[cfg(test)]
mod tests {
    use super::*;
    use claydash_engine::gaussian_ply::Gaussian;
    use glam::Quat;

    fn scene() -> (DataTree, uuid::Uuid) {
        let mut object = model::SdfObject::create_kind(model::PrimitiveKind::Box);
        object.imported_splats = Some(Arc::new(GaussianCloud {
            splats: [0., 1., 2.]
                .map(|x| Gaussian {
                    position: Vec3::new(x, 0., 0.),
                    scale: Vec3::splat(0.2),
                    rotation: Quat::IDENTITY,
                    color: [1., 0., 0.],
                    opacity: 1.,
                    sh_rest: Vec::new(),
                })
                .to_vec(),
        }));
        let id = object.uuid;
        let mut tree = DataTree::default();
        model::set_objects(&mut tree, vec![object]);
        model::set_selected_exact(&mut tree, vec![id]);
        model::set_cloud_edit(
            &mut tree,
            Some(CloudEdit {
                object: id,
                indices: vec![1],
            }),
        );
        (tree, id)
    }

    #[test]
    fn box_selection_includes_an_overlapping_gaussian_with_its_center_outside() {
        let (mut tree, _) = scene();
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let point = camera.project(Vec3::X, 1.).unwrap();
        let overlap = egui::Rect::from_min_max(
            egui::pos2(point.x + 30., point.y - 5.),
            egui::pos2(point.x + 45., point.y + 5.),
        );
        assert!(!overlap.contains(point));
        select_rect(&mut tree, &camera, overlap, 1., SelectionMode::Replace);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![1]);
        let miss = egui::Rect::from_min_max(
            egui::pos2(point.x + 210., point.y - 5.),
            egui::pos2(point.x + 225., point.y + 5.),
        );
        select_rect(&mut tree, &camera, miss, 1., SelectionMode::Replace);
        assert!(model::cloud_edit(&tree).unwrap().indices.is_empty());
    }

    #[test]
    fn circle_selection_includes_visible_splat_footprint_and_stroke_path() {
        let (mut tree, _) = scene();
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let point = camera.project(Vec3::X, 1.).unwrap();
        let edge = egui::pos2(point.x + 40., point.y);
        select_circle_stroke(&mut tree, &camera, edge, edge, 10., 1., SelectionMode::Replace);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![1]);
        let mut edit = model::cloud_edit(&tree).unwrap();
        edit.indices.clear();
        model::set_cloud_edit(&mut tree, Some(edit));
        select_circle_stroke(&mut tree, &camera,
            egui::pos2(point.x - 90., point.y), egui::pos2(point.x + 90., point.y), 5., 1., SelectionMode::Replace);
        assert!(model::cloud_edit(&tree).unwrap().indices.contains(&1));
    }

    #[test]
    fn box_and_circle_subtract_only_overlapping_splats() {
        let (mut tree, _) = scene();
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let mut edit = model::cloud_edit(&tree).unwrap();
        edit.indices = vec![0, 1, 2];
        model::set_cloud_edit(&mut tree, Some(edit));
        let middle = camera.project(Vec3::X, 1.).unwrap();
        let rect = egui::Rect::from_center_size(middle, egui::vec2(12., 12.));
        select_rect(&mut tree, &camera, rect, 1., SelectionMode::Subtract);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![0, 2]);
        let right = camera.project(Vec3::X * 2., 1.).unwrap();
        select_circle_stroke(&mut tree, &camera, right, right, 5., 1., SelectionMode::Subtract);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![0]);
    }

    #[test]
    fn box_selection_uses_rotated_ellipse_instead_of_its_bounding_square() {
        let (mut tree, id) = scene();
        let mut objects = model::objects(&tree);
        let cloud = Arc::make_mut(objects[0].imported_splats.as_mut().unwrap());
        cloud.splats[1].scale = Vec3::new(0.6, 0.05, 0.05);
        cloud.splats[1].rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
        model::set_objects(&mut tree, objects);
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let point = camera.project(Vec3::X, 1.).unwrap();
        let opposite_corner = egui::Rect::from_min_max(
            egui::pos2(point.x + 90., point.y + 90.),
            egui::pos2(point.x + 100., point.y + 100.),
        );
        select_rect(&mut tree, &camera, opposite_corner, 1., SelectionMode::Replace);
        assert!(model::cloud_edit(&tree).unwrap().indices.is_empty());
        let along_major_axis = egui::Rect::from_min_max(
            egui::pos2(point.x + 80., point.y - 90.),
            egui::pos2(point.x + 100., point.y - 70.),
        );
        select_rect(&mut tree, &camera, along_major_axis, 1., SelectionMode::Replace);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![1]);
        assert_eq!(model::selected(&tree), vec![id]);
    }

    #[test]
    fn selected_splats_transform_without_changing_neighbors_and_delete_with_undo() {
        let (mut tree, id) = scene();
        let (edit, initial) = source(&tree).unwrap();
        tree.make_undo_redo_snapshot();
        apply(&mut tree, &edit, &initial, Mat4::from_translation(Vec3::Y));
        let cloud = model::objects_ref(&tree)[0]
            .imported_splats
            .as_ref()
            .unwrap();
        assert_eq!(cloud.splats[0], initial.splats[0]);
        assert_eq!(cloud.splats[1].position, Vec3::new(1., 1., 0.));
        assert_eq!(cloud.splats[2], initial.splats[2]);
        assert!(delete_selected(&mut tree));
        let cloud = model::objects_ref(&tree)[0]
            .imported_splats
            .as_ref()
            .unwrap();
        assert_eq!(cloud.splats.len(), 2);
        tree.undo();
        assert_eq!(
            model::objects_ref(&tree)[0]
                .imported_splats
                .as_ref()
                .unwrap()
                .splats
                .len(),
            3
        );
        assert!(model::cloud_edit(&tree).unwrap().indices.is_empty());
        assert_eq!(model::selected(&tree), vec![id]);
    }

    #[test]
    fn deleting_every_splat_removes_empty_asset() {
        let (mut tree, id) = scene();
        model::set_cloud_edit(
            &mut tree,
            Some(CloudEdit {
                object: id,
                indices: vec![0, 1, 2],
            }),
        );
        assert!(delete_selected(&mut tree));
        assert!(model::objects_ref(&tree).is_empty());
        assert!(model::cloud_edit(&tree).is_none());
    }

    #[test]
    fn repeated_click_enters_edit_and_backspace_deletes_only_picked_splat() {
        use winit::keyboard::KeyCode;
        let (mut tree, id) = scene();
        model::set_cloud_edit(&mut tree, None);
        model::set_selected(&mut tree, Vec::new());
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let point = camera.project(Vec3::new(1., 0., 0.), 1.).unwrap();
        let pointer = Vec2::new(point.x, point.y);
        crate::interactions::InteractionState::select_at(&camera, &mut tree, pointer, Some(id), false);
        assert!(model::cloud_edit(&tree).is_none());
        crate::interactions::InteractionState::select_at(&camera, &mut tree, pointer, Some(id), false);
        assert_eq!(model::cloud_edit(&tree).unwrap().indices, vec![1]);
        let mut interactions = crate::interactions::InteractionState::default();
        let mut commands = crate::commands::Commands::default();
        crate::commands::register_all(&mut commands);
        interactions.key_pressed(KeyCode::Backspace, false, &commands, &mut tree);
        assert_eq!(model::objects_ref(&tree)[0].imported_splats.as_ref().unwrap().splats.len(), 2);
        assert_eq!(model::selected(&tree), vec![id]);
    }

    #[test]
    fn grab_shortcut_moves_selected_splat_without_moving_asset() {
        use winit::keyboard::KeyCode;
        let (mut tree, id) = scene();
        let mut camera = Camera::new();
        camera.viewport = Vec2::new(800., 600.);
        camera.position = Vec3::new(0., 0., 6.);
        camera.target = Vec3::ZERO;
        let point = camera.project(Vec3::new(1., 0., 0.), 1.).unwrap();
        let mut interactions = crate::interactions::InteractionState::default();
        interactions.cursor_moved(Vec2::new(point.x, point.y), false);
        let mut commands = crate::commands::Commands::default();
        crate::commands::register_all(&mut commands);
        interactions.key_pressed(KeyCode::KeyG, false, &commands, &mut tree);
        interactions.update(&mut camera, &mut tree);
        interactions.cursor_moved(Vec2::new(point.x + 40., point.y), false);
        interactions.update(&mut camera, &mut tree);
        let object = &model::objects_ref(&tree)[0];
        assert_eq!(object.uuid, id);
        assert_eq!(object.transform.translation, Vec3::ZERO);
        let splats = &object.imported_splats.as_ref().unwrap().splats;
        assert_ne!(splats[1].position, Vec3::X);
        assert_eq!(splats[0].position, Vec3::ZERO);
        assert_eq!(splats[2].position, Vec3::new(2., 0., 0.));
    }

    #[test]
    fn rotation_and_scale_change_only_selected_splat_data() {
        let (mut tree, id) = scene();
        model::set_cloud_edit(&mut tree, Some(CloudEdit { object: id, indices: vec![0, 1] }));
        let (edit, initial) = source(&tree).unwrap();
        let pivot = center(&tree).unwrap();
        let rotation = Mat4::from_translation(pivot)
            * Mat4::from_quat(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
            * Mat4::from_translation(-pivot);
        apply(&mut tree, &edit, &initial, rotation);
        let splats = &model::objects_ref(&tree)[0].imported_splats.as_ref().unwrap().splats;
        assert!((splats[0].position - Vec3::new(0.5, -0.5, 0.)).length() < 0.0001);
        assert!((splats[1].position - Vec3::new(0.5, 0.5, 0.)).length() < 0.0001);
        assert_eq!(splats[2], initial.splats[2]);
        apply(&mut tree, &edit, &initial, Mat4::from_scale(Vec3::splat(2.)));
        let splats = &model::objects_ref(&tree)[0].imported_splats.as_ref().unwrap().splats;
        assert_eq!(splats[1].position, Vec3::new(2., 0., 0.));
        assert_eq!(splats[1].scale, Vec3::splat(0.4));
        assert_eq!(splats[2], initial.splats[2]);
    }

    #[test]
    fn transform_shortcut_with_no_points_keeps_object_idle() {
        use winit::keyboard::KeyCode;
        let (mut tree, _) = scene();
        let mut edit = model::cloud_edit(&tree).unwrap();
        edit.indices.clear();
        model::set_cloud_edit(&mut tree, Some(edit));
        let mut interactions = crate::interactions::InteractionState::default();
        let mut commands = crate::commands::Commands::default();
        crate::commands::register_all(&mut commands);
        interactions.key_pressed(KeyCode::KeyG, false, &commands, &mut tree);
        assert!(matches!(tree.get_path("editor.state"), model::ClaydashValue::None));
    }
}
