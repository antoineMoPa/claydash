use super::*;

pub(super) struct HostPrograms {
    pub starts: Vec<u32>,
    pub parents_offset: u32,
    pub capacity: u32,
}

// Preserve the editable subtree before render-only union flattening/splitting.
// An inlay references that subtree, never the enclosing car/document component.
pub(super) fn pack(objects: &[&SdfObject], points: &mut Vec<GpuPolygonPoint>) -> HostPrograms {
    let indices: std::collections::HashMap<_, _> = objects.iter().enumerate()
        .map(|(i, object)| (object.uuid, i)).collect();
    let parents: Vec<_> = objects.iter().map(|object| object.boolean_parent
        .and_then(|id| indices.get(&id).copied())).collect();
    let mut starts: Vec<u32> = (0..objects.len() as u32).collect();
    for i in 0..objects.len() {
        if let Some(parent) = parents[i].filter(|&parent| parent > i) {
            starts[parent] = starts[parent].min(starts[i]);
        }
    }
    let mut capacity = 1;
    for object in objects {
        if let Some(host) = object.surface_inlay.and_then(|inlay| indices.get(&inlay.host).copied()) {
            let start = starts[host] as usize;
            let direct = parents[start..host].iter().all(|parent| *parent == Some(host));
            if !direct { capacity = capacity.max((host - start + 1).next_power_of_two() as u32); }
        }
    }
    for (root, start) in starts.iter_mut().enumerate() {
        if parents[*start as usize..root].iter().all(|parent| *parent == Some(root)) {
            *start |= 0x8000_0000; // Streaming host: every operand is a direct child.
        }
    }
    let parents_offset = points.len() as u32;
    if objects.iter().any(|object| object.surface_inlay.is_some()) {
        points.extend(parents.into_iter().map(|parent| GpuPolygonPoint {
            position: [f32::from_bits(parent.map_or(u32::MAX, |i| i as u32)), 0.0],
        }));
    }
    HostPrograms { starts, parents_offset, capacity }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_inlay_keeps_original_host_and_nested_parent_program() {
        let mut host = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        let mut child = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        child.boolean_parent = Some(host.uuid);
        let mut cut = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        cut.boolean_parent = Some(child.uuid);
        let car = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        host.boolean_parent = Some(car.uuid);
        let mut patch = SdfObject::create_kind(crate::model::PrimitiveKind::Sphere);
        patch.boolean_parent = Some(car.uuid);
        patch.surface_inlay = Some(crate::model::SurfaceInlay { host: host.uuid, offset: 0.0, thickness: 0.01 });
        let ordered = vec![&cut, &child, &host, &patch, &car];
        let mut points = Vec::new();
        let programs = pack(&ordered, &mut points);
        assert_eq!(programs.starts[2], 0);
        assert_eq!(programs.capacity, 4);
        assert_eq!(points[0].position[0].to_bits(), 1);
        assert_eq!(points[1].position[0].to_bits(), 2);
        assert_eq!(points[2].position[0].to_bits(), 4);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests;
