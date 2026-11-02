//! Editor state layered over the reusable scene and geometry model.
pub use claydash_engine::model::*;
mod planar_four_bar;
mod polygon_faces;
mod state;
mod variables;
pub use planar_four_bar::*;
pub use polygon_faces::*;
pub use state::*;
pub use variables::*;
#[cfg(test)]
mod tests;

pub fn scene_seams(tree: &DataTree) -> SceneSeams {
    match tree.get_path("scene.seams") {
        ClaydashValue::Seams(value) => value,
        _ => SceneSeams::default(),
    }
}
pub fn try_set_seams(tree: &mut DataTree, seams: SceneSeams) -> Result<(), String> {
    let mut objects = objects(tree);
    let mut variables = scene_variables(tree);
    evaluate_derived_variables(&mut variables);
    apply_vector_bindings(&variables, &mut objects);
    resolve_sheet_seams(&mut objects)?;
    resolve_seam_bindings(&seams, &mut objects)?;
    tree.set_path("scene.seams", ClaydashValue::Seams(seams));
    set_objects(tree, objects);
    Ok(())
}
