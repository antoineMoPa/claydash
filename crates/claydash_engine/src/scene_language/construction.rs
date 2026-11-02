//! Declarative access to construction entities and ordinary object modifiers.
use super::*;
use crate::model::{
    CageAxis, Lattice, LatticeEdge, SceneSeams, Seam, SeamBinding, SeamPoint, SeamTarget, SheetEdge,
};

pub struct CompiledScene {
    pub parts: Vec<(String, SdfObject)>,
    pub seams: SceneSeams,
    pub seam_paths: Vec<(String, uuid::Uuid)>,
}
#[derive(Clone)]
pub(super) enum PointSpec {
    World(Vec3),
    Snap {
        source: String,
        at: Vec3,
    },
    Surface {
        source: String,
        triangle: usize,
        barycentric: Vec3,
    },
}
#[derive(Clone)]
pub(super) struct SeamSpec {
    points: Vec<PointSpec>,
    closed: bool,
}
#[derive(Clone)]
pub(super) struct CageSpec {
    resolution: u8,
    min: Option<Vec3>,
    max: Option<Vec3>,
    controls: Vec<([usize; 3], Vec3)>,
    offsets: Option<Vec<Vec3>>,
}
#[derive(Clone)]
pub(super) struct BindingSpec {
    object: String,
    seam: String,
    target: SeamTarget,
    span: Option<[usize; 2]>,
    reverse: bool,
    tolerance: f32,
}
pub(super) struct CompileContext<'a> {
    loader: &'a mut dyn FnMut(&str) -> Result<Vec<SdfObject>, String>,
    assets: HashMap<String, Vec<SdfObject>>,
    pub seams: Vec<(String, usize, SeamSpec)>,
    pub bindings: Vec<(usize, BindingSpec)>,
}
impl<'a> CompileContext<'a> {
    pub fn new(loader: &'a mut dyn FnMut(&str) -> Result<Vec<SdfObject>, String>) -> Self {
        Self {
            loader,
            assets: HashMap::new(),
            seams: Vec::new(),
            bindings: Vec::new(),
        }
    }
}
pub(super) fn text(v: Value) -> Result<String, String> {
    if let Value::Text(v) = v {
        Ok(v)
    } else {
        Err("expected a string".into())
    }
}
fn boolean(v: Value) -> Result<bool, String> {
    if let Value::Bool(v) = v {
        Ok(v)
    } else {
        Err("expected true or false".into())
    }
}
fn integer(v: Value) -> Result<usize, String> {
    let n = number(v)?;
    if n < 0.0 || n.fract() != 0.0 || n > 1_000_000.0 {
        return Err("expected a nonnegative integer <= 1000000".into());
    }
    Ok(n as usize)
}
pub(super) fn list(v: Value) -> Result<Vec<Value>, String> {
    if let Value::List(v) = v {
        Ok(v)
    } else {
        Err("expected a list".into())
    }
}
pub(super) fn required(args: &mut BTreeMap<String, Value>, key: &str) -> Result<Value, String> {
    args.remove(key).ok_or_else(|| format!("missing '{key}'"))
}
pub(super) fn no_extra(args: &BTreeMap<String, Value>, name: &str) -> Result<(), String> {
    if let Some(key) = args.keys().next() {
        Err(format!("unknown {name} argument '{key}'"))
    } else {
        Ok(())
    }
}
fn fixed_indices<const N: usize>(v: Value) -> Result<[usize; N], String> {
    list(v)?
        .into_iter()
        .map(integer)
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| format!("expected {N} indices"))
}
fn point(v: Value) -> Result<PointSpec, String> {
    match v {
        Value::Point(p) => Ok(p),
        v => Ok(PointSpec::World(vector(v)?)),
    }
}
fn axis(v: Value) -> Result<CageAxis, String> {
    match text(v)?.as_str() {
        "X" => Ok(CageAxis::X),
        "Y" => Ok(CageAxis::Y),
        "Z" => Ok(CageAxis::Z),
        _ => Err("axis must be X, Y or Z".into()),
    }
}
fn target_edge(v: Value) -> Result<SheetEdge, String> {
    match text(v)?.as_str() {
        "MinX" => Ok(SheetEdge::MinX),
        "MaxX" => Ok(SheetEdge::MaxX),
        "MinY" => Ok(SheetEdge::MinY),
        "MaxY" => Ok(SheetEdge::MaxY),
        _ => Err("sheet edge must be MinX, MaxX, MinY or MaxY".into()),
    }
}

pub(super) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "seam"
            | "snap"
            | "surface_point"
            | "lattice"
            | "control"
            | "sheet_edge"
            | "lattice_edge"
            | "bind"
            | "mesh_asset"
    )
}
pub(super) fn evaluate_call(
    name: &str,
    pos: &[Value],
    args: &mut BTreeMap<String, Value>,
    context: &mut CompileContext<'_>,
) -> Result<Option<Value>, String> {
    let known = reserved(name);

    if !known {
        return Ok(None);
    }
    if name == "sheet_edge" {
        if pos.len() != 1 {
            return Err("sheet_edge requires one edge string".into());
        }
        no_extra(args, name)?;
        return Ok(Some(Value::Target(SeamTarget::SheetBoundary(target_edge(
            pos[0].clone(),
        )?))));
    }
    if !pos.is_empty() {
        return Err(format!("{name} accepts named arguments only"));
    }
    let value = match name {
        "seam" => {
            let points = list(required(args, "points")?)?
                .into_iter()
                .map(point)
                .collect::<Result<Vec<_>, _>>()?;
            let closed = args
                .remove("closed")
                .map(boolean)
                .transpose()?
                .unwrap_or(false);
            Value::Seam(SeamSpec { points, closed })
        }
        "snap" => Value::Point(PointSpec::Snap {
            source: text(required(args, "source")?)?,
            at: vector(required(args, "at")?)?,
        }),
        "surface_point" => Value::Point(PointSpec::Surface {
            source: text(required(args, "source")?)?,
            triangle: integer(required(args, "triangle")?)?,
            barycentric: vector(required(args, "barycentric")?)?,
        }),
        "control" => Value::Control(
            fixed_indices::<3>(required(args, "index")?)?,
            vector(required(args, "offset")?)?,
        ),
        "lattice" => {
            let resolution = args
                .remove("resolution")
                .map(integer)
                .transpose()?
                .unwrap_or(3);
            if !(2..=9).contains(&resolution) {
                return Err("lattice resolution must be an integer from 2 to 9".into());
            }
            let min = args.remove("min").map(vector).transpose()?;
            let max = args.remove("max").map(vector).transpose()?;
            if min.is_some() != max.is_some() {
                return Err(
                    "provide both lattice min and max, or omit both for automatic bounds".into(),
                );
            }
            let controls = args
                .remove("controls")
                .map(list)
                .transpose()?
                .unwrap_or_default()
                .into_iter()
                .map(|v| match v {
                    Value::Control(i, p) => Ok((i, p)),
                    _ => Err("controls must contain control(index: [...], offset: [...])".into()),
                })
                .collect::<Result<Vec<_>, String>>()?;
            let offsets = args
                .remove("offsets")
                .map(list)
                .transpose()?
                .map(|v| v.into_iter().map(vector).collect::<Result<Vec<_>, _>>())
                .transpose()?;
            if offsets.is_some() && !controls.is_empty() {
                return Err("use sparse controls or full offsets, not both".into());
            }
            Value::Cage(CageSpec {
                resolution: resolution as u8,
                min,
                max,
                controls,
                offsets,
            })
        }
        "lattice_edge" => {
            let axis = axis(required(args, "axis")?)?;
            let sides = list(required(args, "sides")?)?
                .into_iter()
                .map(boolean)
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| "lattice_edge sides requires two booleans")?;
            Value::Target(SeamTarget::LatticeEdge(LatticeEdge { axis, sides }))
        }
        "bind" => {
            let object = text(required(args, "object")?)?;
            let seam = text(required(args, "seam")?)?;
            let Value::Target(target) = required(args, "target")? else {
                return Err("bind target must be sheet_edge(...) or lattice_edge(...)".into());
            };
            let span = args.remove("span").map(fixed_indices::<2>).transpose()?;
            let reverse = args
                .remove("reverse")
                .map(boolean)
                .transpose()?
                .unwrap_or(false);
            let tolerance = args
                .remove("tolerance")
                .map(number)
                .transpose()?
                .unwrap_or(0.001);
            Value::Binding(BindingSpec {
                object,
                seam,
                target,
                span,
                reverse,
                tolerance,
            })
        }
        "mesh_asset" => {
            let path = text(required(args, "path")?)?;
            let mesh = args.remove("mesh").map(integer).transpose()?.unwrap_or(0);
            if !context.assets.contains_key(&path) {
                let objects = (context.loader)(&path)?;
                context.assets.insert(path.clone(), objects);
            }
            let object = context.assets[&path]
                .get(mesh)
                .cloned()
                .ok_or_else(|| format!("mesh_asset '{path}' has no mesh {mesh}"))?;
            if object.imported_mesh.is_none() {
                return Err("mesh_asset loader must return triangle meshes".into());
            }
            let mut parts = vec![Part {
                path: String::new(),
                object,
            }];
            let at = take_vec(args, "at", Vec3::ZERO)?;
            let rotation = take_vec(args, "rotation", Vec3::ZERO)?;
            let scale = take_vec(args, "scale", Vec3::ONE)?;
            apply_transform(&mut parts, at, rotation, scale)?;
            Value::Parts(parts)
        }
        _ => unreachable!(),
    };
    no_extra(args, name)?;
    Ok(Some(value))
}

pub(super) fn apply_modifiers(
    object: &mut SdfObject,
    args: &mut BTreeMap<String, Value>,
) -> Result<(), String> {
    if let Some(v) = args.remove("opacity") {
        let alpha = number(v)?;
        if !(0.0..=1.0).contains(&alpha) {
            return Err("opacity must be between 0 and 1".into());
        }
        object.color.w = alpha;
        object.material.color.w = alpha;
    }
    if let Some(v) = args.remove("mirror") {
        let mut axes = [false; 3];
        for v in list(v)? {
            let i = axis(v)?.index();
            if axes[i] {
                return Err("duplicate mirror axis".into());
            }
            axes[i] = true;
        }
        object.mirror = Some(crate::model::Mirror { axes });
    }
    if let Some(v) = args.remove("lattice") {
        let Value::Cage(spec) = v else {
            return Err("lattice requires lattice(...)".into());
        };
        let (min, max) = if let (Some(min), Some(max)) = (spec.min, spec.max) {
            (min, max)
        } else {
            let (mut min, mut max) = crate::model::lattice_bounds(&[object.clone()], object.uuid)
                .ok_or("cannot compute lattice bounds")?;
            let padding = (max - min).max_element() * 0.15;
            for axis in 0..3 {
                if max[axis] - min[axis] < padding * 2.0 {
                    let center = (min[axis] + max[axis]) * 0.5;
                    min[axis] = center - padding.max(0.001);
                    max[axis] = center + padding.max(0.001);
                }
            }
            (min, max)
        };
        if !min.is_finite() || !max.is_finite() || (max - min).min_element() <= 0.0 {
            return Err("lattice max must exceed min on every axis".into());
        }
        let mut cage = Lattice::new(min, max, spec.resolution);
        let n = spec.resolution as usize;
        if let Some(offsets) = spec.offsets {
            if offsets.len() != n * n * n {
                return Err(format!("lattice offsets requires {} vectors", n * n * n));
            }
            cage.offsets = offsets;
        }
        let mut seen = HashSet::new();
        for (index, offset) in spec.controls {
            if index.iter().any(|i| *i >= n)
                || !cage.is_surface_point(index[0], index[1], index[2])
                || !seen.insert(index)
            {
                return Err(
                    "lattice controls must name distinct surface nodes inside its resolution"
                        .into(),
                );
            }
            let slot = cage.index(index[0], index[1], index[2]);
            cage.offsets[slot] = offset;
        }
        cage.current_shape_key = None;
        object.lattice = Some(cage);
    }
    Ok(())
}

pub(super) fn finish(
    parts: Vec<(String, SdfObject)>,
    context: CompileContext<'_>,
    source: &str,
) -> Result<CompiledScene, String> {
    let mut lookup = HashMap::new();
    for (path, object) in &parts {
        lookup.insert(path.clone(), object.uuid);
        if let Some(alias) = path.strip_suffix('/') {
            lookup.insert(alias.to_owned(), object.uuid);
        }
    }
    let id = |name: &str| {
        lookup
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown part '{name}'"))
    };
    let objects = parts.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
    let mut seams = SceneSeams::default();
    let mut seam_paths = Vec::new();
    let mut seam_lookup = HashMap::new();
    for (name, offset, spec) in context.seams {
        let point = |p: PointSpec| -> Result<SeamPoint, String> {
            Ok(match p {
                PointSpec::World(p) => SeamPoint::World(p),
                PointSpec::Snap { source, at } => {
                    crate::model::seam_surface_point(&objects, id(&source)?, at)?
                }
                PointSpec::Surface {
                    source,
                    triangle,
                    barycentric,
                } => SeamPoint::Surface {
                    object: id(&source)?,
                    triangle,
                    barycentric,
                },
            })
        };
        let points = spec
            .points
            .into_iter()
            .map(point)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| error(source, offset, &format!("seam '{name}': {e}")))?;
        let hash = Sha256::digest(format!("claydash-scene-dsl-seam-v1:{name}").as_bytes());
        let mut bytes = [0; 16];
        bytes.copy_from_slice(&hash[..16]);
        bytes[6] = (bytes[6] & 15) | 0x50;
        bytes[8] = (bytes[8] & 63) | 0x80;
        let uuid = uuid::Uuid::from_bytes(bytes);
        seam_lookup.insert(name.clone(), uuid);
        seam_paths.push((name.clone(), uuid));
        seams.seams.push(Seam {
            id: uuid,
            name,
            points,
            closed: spec.closed,
        });
    }
    let labels = context
        .bindings
        .iter()
        .map(|(_, b)| format!("{} -> {} ({:?})", b.object, b.seam, b.target))
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    for (offset, spec) in context.bindings {
        let object = id(&spec.object).map_err(|e| error(source, offset, &e))?;
        let seam = seam_lookup
            .get(&spec.seam)
            .copied()
            .ok_or_else(|| error(source, offset, &format!("unknown seam '{}'", spec.seam)))?;
        seams.bindings.push(SeamBinding {
            object,
            seam,
            target: spec.target,
            span: spec.span,
            reverse: spec.reverse,
            tolerance: spec.tolerance,
        });
    }
    let mut bound = objects;
    crate::model::resolve_seam_bindings(&seams, &mut bound)
        .map_err(|e| format!("connections [{labels}]: {e}"))?;
    Ok(CompiledScene {
        parts: parts.into_iter().map(|(path, _)| path).zip(bound).collect(),
        seams,
        seam_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_parts_and_shared_native_sheet_connections_are_deterministic() {
        let source = r#"
object pair({}):
 left = sheet_panel(size:[2,1],at:[-1,0,0],lattice:lattice(min:[-1,-0.5,-0.1],max:[1,0.5,0.1]));
 right = sheet_panel(size:[2,1],at:[1,0,0],lattice:lattice(min:[-1,-0.5,-0.1],max:[1,0.5,0.1]));
end
place(pair(),name:"panels");
join = seam(points:[[0,-0.5,0],[0,0,0.04],[0,0.5,0]]);
bind(object:"panels/left",seam:"join",target:sheet_edge("MaxX"));
bind(object:"panels/right",seam:"join",target:sheet_edge("MinX"));
"#;
        let a = compile_scene(source).unwrap();
        let b = compile_scene(&format!("extra=box(at:[8,0,0]);{source}")).unwrap();
        assert_eq!(a.parts.len(), 2);
        assert_eq!(a.seams.bindings.len(), 2);
        assert_eq!(a.seam_paths, b.seam_paths);
        for (path, object) in &a.parts {
            assert_eq!(
                object.uuid,
                b.parts.iter().find(|(p, _)| p == path).unwrap().1.uuid
            );
            assert!(object
                .lattice
                .as_ref()
                .unwrap()
                .offsets
                .iter()
                .any(|p| p.length() > 0.01));
        }
        assert!(compile(source).err().unwrap().contains("compile_scene"));
        assert!(compile_scene(&source.replace("panels/left", "missing"))
            .err()
            .unwrap()
            .contains("unknown part 'missing'"));
    }
    #[test]
    fn cage_edge_and_native_bend_are_regular_scene_features() {
        let scene = compile_scene(
            r#"
block=box(size:[2,2,2],lattice:lattice(min:[-1,-1,-1],max:[1,1,1]));
edge=seam(points:[[1,-1,-1],[1.2,0,-0.9],[1,1,-1]]);
bind(object:"block",seam:"edge",target:lattice_edge(axis:"Y",sides:[true,false]));
panel=sheet_panel(size:[2,1],bends:[bend(start:0,radius:0.3,angle:45)]);
"#,
        )
        .unwrap();
        assert_eq!(scene.seams.bindings.len(), 1);
        assert!(scene
            .parts
            .iter()
            .any(|(_, o)| matches!(&o.params,SdfParams::SheetPanelParams(p) if p.bends.len()==1)));
        let cage = scene
            .parts
            .iter()
            .find(|(p, _)| p == "block/")
            .unwrap()
            .1
            .lattice
            .as_ref()
            .unwrap();
        assert!(cage
            .offsets
            .iter()
            .any(|p| p.distance(Vec3::new(0.2, 0., 0.1)) < 1e-5));
    }
    #[test]
    fn sparse_controls_are_validated_and_expand_capture_bounds() {
        let scene=compile_scene("p=box(lattice:lattice(controls:[control(index:[0,0,0],offset:[0,3,0])]),mirror:[\"X\"],opacity:0.4);").unwrap();
        let object = &scene.parts[0].1;
        assert_eq!(object.color.w, 0.4);
        assert_eq!(object.mirror.unwrap().axes, [true, false, false]);
        assert!(bounds(&[object.clone()]).unwrap().1.y > 3.);
        for cage in [
            "lattice(resolution:10)",
            "lattice(min:[0,0,0])",
            "lattice(min:[0,0,0],max:[0,1,1])",
            "lattice(controls:[control(index:[1,1,1],offset:[0,0,1])])",
            "lattice(controls:[control(index:[3,0,0],offset:[0,0,1])])",
            "lattice(offsets:[[0,0,0]])",
        ] {
            assert!(
                compile_scene(&format!("p=box(lattice:{cage});")).is_err(),
                "{cage}"
            );
        }
        assert!(
            compile_scene("object a({}): s=seam(points:[[0,0,0],[1,0,0]]); end place(a());")
                .is_err()
        );
        assert!(compile_scene("object lattice({}): a=box(); end").is_err());
    }
    #[test]
    fn declared_assets_are_cached_snap_to_named_mesh_and_keep_owned_ids() {
        let mut object = SdfObject::create_kind(PrimitiveKind::Box);
        object.imported_mesh = Some(std::sync::Arc::new(
            crate::renderer::poisson_mesh::geometry::Mesh::from_parts(
                vec![
                    Vec3::new(10., 0., 0.),
                    Vec3::new(12., 0., 0.),
                    Vec3::new(10., 2., 0.),
                ],
                vec![[0, 1, 2]],
                vec![object.uuid],
            ),
        ));
        let source = r#"ref=mesh_asset(path:"ref.glb"); copy=mesh_asset(path:"ref.glb",at:[0,0,4]); trace=seam(points:[snap(source:"ref",at:[10,0,0.2]),surface_point(source:"ref",triangle:0,barycentric:[0,1,0])]);"#;
        let mut calls = 0;
        let scene = compile_with_assets(source, &mut |_| {
            calls += 1;
            Ok(vec![object.clone()])
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(scene.parts.len(), 2);
        for (_, o) in &scene.parts {
            assert!(o
                .imported_mesh
                .as_ref()
                .unwrap()
                .owners
                .iter()
                .all(|id| *id == o.uuid));
        }
        let objects = scene
            .parts
            .iter()
            .map(|(_, o)| o.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            scene.seams.seams[0].positions(&objects).unwrap(),
            vec![Vec3::new(10., 0., 0.), Vec3::new(12., 0., 0.)]
        );
        let (min, max) = bounds(&objects).unwrap();
        assert!(min.x >= 10. && max.x >= 12. && max.z >= 4.);
        assert!(compile_scene(source)
            .err()
            .unwrap()
            .contains("asset loader"));
    }
}
