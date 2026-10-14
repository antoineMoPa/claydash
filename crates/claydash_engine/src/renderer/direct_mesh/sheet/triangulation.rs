//! Keep material boundaries fixed while reconnecting interior samples for quality.
use super::{check_cancel, edge};
use glam::DVec2;
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

pub(super) fn improve(
    points: &mut Vec<DVec2>,
    faces: &mut Vec<[u32; 3]>,
    insert_support: bool,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if faces.is_empty() {
        return Ok(());
    }
    check_cancel(cancel)?;
    let mut uses = BTreeMap::<_, usize>::new();
    for &[a, b, c] in faces.iter() {
        for (a, b) in [(a, b), (b, c), (c, a)] {
            *uses.entry(edge(a, b)).or_default() += 1;
        }
    }
    let constraints = uses
        .into_iter()
        .filter_map(|((a, b), n)| (n == 1).then_some([a as usize, b as usize]))
        .collect();
    let vertices = points.iter().map(|p| Point2::new(p.x, p.y)).collect();
    let mut cdt =
        ConstrainedDelaunayTriangulation::<Point2<f64>>::bulk_load_cdt(vertices, constraints)
            .map_err(|error| format!("sheet constrained triangulation failed: {error:?}"))?;
    // Preserve circular samples exactly. Splitting their chords here would move
    // boundary vertices away from the native cut; curvature refinement owns that.
    let result = cdt.refine(
        RefinementParameters::new()
            .exclude_outer_faces(true)
            .keep_constraint_edges()
            .with_angle_limit(AngleLimit::from_deg(if insert_support {25.0} else {0.0}))
            .with_max_additional_vertices(100_000),
    );
    check_cancel(cancel)?;
    if !result.refinement_complete {
        return Err("sheet triangle-quality refinement exceeded its vertex budget".into());
    }
    let excluded: BTreeSet<_> = result
        .excluded_faces
        .into_iter()
        .map(|f| f.index())
        .collect();
    points.clear();
    points.resize(cdt.num_vertices(), DVec2::ZERO);
    for vertex in cdt.vertices() {
        let p = vertex.position();
        points[vertex.fix().index()] = DVec2::new(p.x, p.y);
    }
    faces.clear();
    for face in cdt.inner_faces() {
        if !excluded.contains(&face.fix().index()) {
            faces.push(face.vertices().map(|v| v.fix().index() as u32));
        }
    }
    Ok(())
}
