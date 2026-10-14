//! Native source field used for smooth blends and spatial modifiers.
//! Image planes use the native thin-box field: shader image alpha belongs to
//! the baked material, never to an invented alpha-threshold geometry field.
use crate::model::{object_world_matrix, PreparedSubtreeSampler, SdfObject};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) struct PreparedDirectField<'a> {
    sampler: PreparedSubtreeSampler<'a>,
    cell_discontinuities: bool,
}

impl<'a> PreparedDirectField<'a> {
    pub(super) fn new(scene: &'a [SdfObject], root: Uuid) -> Result<Self, String> {
        let mut indices = HashMap::new();
        for (index, object) in scene.iter().enumerate() {
            if indices.insert(object.uuid, index).is_some() {
                return Err("Direct mesh source has duplicate object identifiers".into());
            }
        }
        let root_index = *indices
            .get(&root)
            .ok_or_else(|| "Direct mesh root no longer exists".to_owned())?;
        let mut children = vec![Vec::new(); scene.len()];
        for (index, object) in scene.iter().enumerate() {
            if let Some(parent) = object.boolean_parent.and_then(|id| indices.get(&id)) {
                children[*parent].push(index);
            }
        }
        let mut state = vec![0_u8; scene.len()];
        validate_dependencies(scene, &indices, &children, root_index, &mut state, 0)?;
        for (index, object) in scene
            .iter()
            .enumerate()
            .filter(|(index, _)| state[*index] != 0)
        {
            validate_geometry_references(scene, object)?;
            let mut ancestor = Some(index);
            let mut chain = Vec::new();
            while let Some(candidate) = ancestor {
                if chain.contains(&candidate) || chain.len() >= 256 {
                    return Err(
                        "Direct mesh source has a cyclic or excessively deep parent hierarchy"
                            .into(),
                    );
                }
                chain.push(candidate);
                validate_modifier(&scene[candidate])?;
                ancestor = scene[candidate]
                    .boolean_parent
                    .map(|id| {
                        indices
                            .get(&id)
                            .copied()
                            .ok_or_else(|| format!("Missing Boolean parent on {}", object.name))
                    })
                    .transpose()?;
            }
            let matrix = object_world_matrix(scene, object.uuid);
            if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
                return Err(format!(
                    "Direct mesh requires a finite, invertible transform on {}",
                    object.name
                ));
            }
        }
        let cell_discontinuities = scene.iter().enumerate().any(|(index, object)| {
            state[index] != 0
                && object.repetition.enabled
                && matches!(object.repetition.mode, crate::model::RepetitionMode::Linear)
                && object.repetition.count.iter().any(|&count| count > 1)
        });
        let sampler = PreparedSubtreeSampler::new(scene, root)
            .ok_or_else(|| "Direct mesh root no longer exists".to_owned())?;
        Ok(Self {
            sampler,
            cell_discontinuities,
        })
    }

    pub(super) fn sample(&mut self, world: Vec3) -> (f32, Uuid) {
        self.sampler.sample(world)
    }

    pub(super) fn deformation_extent(&self, grid_inverse: Mat4) -> Vec3 {
        self.sampler.deformation_extent(grid_inverse)
    }

    pub(super) fn has_cell_discontinuities(&self) -> bool {
        self.cell_discontinuities
    }
}

fn validate_geometry_references(scene: &[SdfObject], object: &SdfObject) -> Result<(), String> {
    if matches!(object.params, crate::model::SdfParams::TextParams(_)) {
        crate::model::prepared_scene_text(scene, object)
            .map_err(|error| format!("Invalid text geometry on {}: {error}", object.name))?;
    }
    if let Some(profile) = object
        .path_extrusion
        .and_then(|modifier| modifier.profile_curve)
    {
        let reference = scene
            .iter()
            .find(|candidate| candidate.uuid == profile)
            .ok_or_else(|| format!("Missing custom profile curve on {}", object.name))?;
        if !matches!(
            reference.params,
            crate::model::SdfParams::BezierCurveParams(_)
        ) {
            return Err(format!(
                "Custom profile on {} must reference a Bézier curve",
                object.name
            ));
        }
        let vertices = crate::model::profile_curve_vertices(scene, profile)
            .ok_or_else(|| format!("Invalid custom profile curve on {}", object.name))?;
        if vertices.len() < 3 || vertices.iter().any(|point| !point.is_finite()) {
            return Err(format!(
                "Custom profile curve on {} needs finite polygon vertices",
                object.name
            ));
        }
    }
    Ok(())
}

fn validate_dependencies(
    scene: &[SdfObject],
    indices: &HashMap<Uuid, usize>,
    children: &[Vec<usize>],
    index: usize,
    state: &mut [u8],
    depth: usize,
) -> Result<(), String> {
    if state[index] == 1 || depth >= 256 {
        return Err(
            "Direct mesh encountered a cyclic or excessively deep Boolean/inlay dependency".into(),
        );
    }
    if state[index] == 2 {
        return Ok(());
    }
    state[index] = 1;
    for &child in &children[index] {
        validate_dependencies(scene, indices, children, child, state, depth + 1)?;
    }
    if let Some(inlay) = scene[index].surface_inlay {
        let host = *indices
            .get(&inlay.host)
            .ok_or_else(|| format!("Missing surface inlay host on {}", scene[index].name))?;
        validate_dependencies(scene, indices, children, host, state, depth + 1)?;
    }
    state[index] = 2;
    Ok(())
}

fn validate_modifier(object: &SdfObject) -> Result<(), String> {
    if !object.softness.is_finite() || object.softness < 0.0 {
        return Err(format!("Invalid Boolean softness on {}", object.name));
    }
    if let Some(inlay) = object.surface_inlay {
        if !inlay.offset.is_finite() || !inlay.thickness.is_finite() || inlay.thickness <= 0.0 {
            return Err(format!("Invalid surface inlay shell on {}", object.name));
        }
    }
    if object.repetition.enabled {
        if !object.repetition.spacing.is_finite() {
            return Err(format!("Invalid repetition spacing on {}", object.name));
        }
        if let crate::model::RepetitionMode::Radial { pivot, .. } = object.repetition.mode {
            if !pivot.is_finite() {
                return Err(format!(
                    "Invalid radial repetition pivot on {}",
                    object.name
                ));
            }
        }
    }
    if let Some(cage) = &object.lattice {
        let n = cage.resolution as usize;
        if !(2..=9).contains(&n)
            || cage.offsets.len() != n * n * n
            || !cage.min.is_finite()
            || !cage.max.is_finite()
            || (cage.max - cage.min).min_element() <= 0.0
            || cage
                .effective_offsets()
                .iter()
                .any(|offset| !offset.is_finite())
        {
            return Err(format!("Invalid lattice cage on {}", object.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
