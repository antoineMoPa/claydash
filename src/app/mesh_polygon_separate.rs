//! Partition an imported triangle mesh against a finite world-space prism.
use std::{collections::HashMap, sync::Arc};
use glam::Vec3;
use serde::{Deserialize,Serialize};
use crate::{model, renderer::poisson_mesh::geometry::{baked_triangle_uv, Mesh}};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Projection { Xy, Xz, Yz }
impl Projection {
    pub(crate) fn coordinates(self, p: Vec3) -> (f32, f32, f32) {
        match self { Self::Xy => (p.x,p.y,p.z), Self::Xz => (p.x,p.z,p.y), Self::Yz => (p.y,p.z,p.x) }
    }
}

#[derive(Clone, Copy)]
struct Plane { a: f32, b: f32, c: f32, depth: bool }
impl Plane {
    fn distance(self, p: Vec3, projection: Projection) -> f32 {
        let (u,v,d) = projection.coordinates(p);
        if self.depth { self.a*d + self.c } else { self.a*u + self.b*v + self.c }
    }
}

#[derive(Clone, Copy)]
struct Corner { vertex: u32, uv: [f32;2], normal: Vec3 }

struct Workspace<'a> {
    source: &'a Mesh,
    positions: Vec<Vec3>,
    colors: Vec<[f32;3]>,
    world: glam::Mat4,
    inverse: glam::Mat4,
    edges: HashMap<(usize,u32,u32),u32>,
}
impl Workspace<'_> {
    fn crossing(&mut self, plane_index: usize, plane: Plane, projection: Projection,
        a: Corner, b: Corner, da: f32, db: f32) -> Corner {
        if da == 0. { return a; }
        if db == 0. { return b; }
        let t = da/(da-db);
        let key = (plane_index,a.vertex.min(b.vertex),a.vertex.max(b.vertex));
        let vertex = *self.edges.entry(key).or_insert_with(|| {
            let aw = self.world.transform_point3(self.positions[a.vertex as usize]);
            let bw = self.world.transform_point3(self.positions[b.vertex as usize]);
            let mut point = aw.lerp(bw,t);
            let residual = plane.distance(point,projection);
            // Project onto the clipping plane to keep shared boundaries exact.
            let denominator = plane.a*plane.a + plane.b*plane.b;
            let (u,v,d) = projection.coordinates(point);
            let (u,v,d) = if plane.depth { (u,v,d-residual/plane.a) }
                else { (u-residual*plane.a/denominator,v-residual*plane.b/denominator,d) };
            point = match projection { Projection::Xy => Vec3::new(u,v,d),
                Projection::Xz => Vec3::new(u,d,v), Projection::Yz => Vec3::new(d,u,v) };
            let index = self.positions.len() as u32;
            self.positions.push(self.inverse.transform_point3(point));
            if !self.source.colors.is_empty() {
                let ac = self.colors[a.vertex as usize]; let bc = self.colors[b.vertex as usize];
                self.colors.push(std::array::from_fn(|i| ac[i]+(bc[i]-ac[i])*t));
            }
            index
        });
        let n = a.normal.lerp(b.normal,t);
        Corner { vertex, uv: std::array::from_fn(|i| a.uv[i]+(b.uv[i]-a.uv[i])*t),
            normal: if n.length_squared()>1e-20 { n/n.length() } else { a.normal } }
    }

    fn split(&mut self, polygon: &[Corner], plane_index: usize, plane: Plane,
        projection: Projection) -> (Vec<Corner>,Vec<Corner>) {
        let mut inside=Vec::with_capacity(polygon.len()+1);
        let mut outside=Vec::with_capacity(polygon.len()+1);
        for i in 0..polygon.len() {
            let a=polygon[i]; let b=polygon[(i+1)%polygon.len()];
            let da=plane.distance(self.world.transform_point3(self.positions[a.vertex as usize]),projection);
            let db=plane.distance(self.world.transform_point3(self.positions[b.vertex as usize]),projection);
            if da>=0. { inside.push(a); } else { outside.push(a); }
            if (da>=0.) != (db>=0.) {
                let cross=self.crossing(plane_index,plane,projection,a,b,da,db);
                inside.push(cross); outside.push(cross);
            }
        }
        (inside,outside)
    }
}

fn emit(polygon: &[Corner], output: &mut Mesh, source: &Workspace<'_>,
    vertices: &mut HashMap<u32,u32>, owner: uuid::Uuid, textured: bool) -> Result<(),String> {
    if polygon.len()<3 { return Ok(()); }
    for i in 1..polygon.len()-1 {
        let corners=[polygon[0],polygon[i],polygon[i+1]];
        let p=corners.map(|c| source.positions[c.vertex as usize]);
        if (p[1]-p[0]).cross(p[2]-p[0]).length_squared()<=1e-20 { continue; }
        let triangle=corners.map(|corner| *vertices.entry(corner.vertex).or_insert_with(|| {
            let index=output.positions.len() as u32;
            output.positions.push(source.positions[corner.vertex as usize]);
            if !source.colors.is_empty() { output.colors.push(source.colors[corner.vertex as usize]); }
            index
        }));
        output.triangles.push(triangle); output.owners.push(owner);
        output.normals.push(corners.map(|corner| corner.normal));
        if textured { output.uvs.push(corners.map(|corner| corner.uv)); }
        if output.triangles.len()>10_000_000 || output.positions.len()>10_000_000 {
            return Err("partition exceeds the 10,000,000 vertex or face limit".into());
        }
    }
    Ok(())
}

fn planes(polygon: &[[f32;2]], depth: [f32;2]) -> Result<Vec<Plane>,String> {
    if !(3..=32).contains(&polygon.len()) || polygon.iter().flatten().any(|v| !v.is_finite())
        || !depth[0].is_finite() || !depth[1].is_finite() || depth[0]>=depth[1] {
        return Err("polygon needs 3–32 finite points and a finite increasing depth interval".into());
    }
    let twice_area=(0..polygon.len()).map(|i| {
        let a=polygon[i]; let b=polygon[(i+1)%polygon.len()]; a[0]*b[1]-b[0]*a[1]
    }).sum::<f32>();
    if twice_area.abs()<=1e-12 { return Err("polygon area is zero".into()); }
    let sign=twice_area.signum();
    let mut result=Vec::with_capacity(polygon.len()+2);
    for i in 0..polygon.len() {
        let a=polygon[i]; let b=polygon[(i+1)%polygon.len()];
        let edge=[b[0]-a[0],b[1]-a[1]];
        let length=(edge[0]*edge[0]+edge[1]*edge[1]).sqrt();
        if length<=1e-8 { return Err("polygon has a repeated or near-duplicate point".into()); }
        let cross=edge[0]*(polygon[(i+2)%polygon.len()][1]-b[1])
            -edge[1]*(polygon[(i+2)%polygon.len()][0]-b[0]);
        if cross*sign<=1e-10 { return Err("polygon must be strictly convex with no repeated or collinear points".into()); }
        for (j,point) in polygon.iter().enumerate() {
            if j==i || j==(i+1)%polygon.len() { continue; }
            let side=edge[0]*(point[1]-a[1])-edge[1]*(point[0]-a[0]);
            if side*sign<=1e-10 {
                return Err("polygon must be strictly convex and must not self-intersect".into());
            }
        }
        let nx=-sign*edge[1]/length; let ny=sign*edge[0]/length;
        result.push(Plane { a:nx,b:ny,c:-(nx*a[0]+ny*a[1]),depth:false });
    }
    result.push(Plane { a:1.,b:0.,c:-depth[0],depth:true });
    result.push(Plane { a:-1.,b:0.,c:depth[1],depth:true });
    Ok(result)
}

fn separate_convex_mesh_polygon(draft: &mut model::DataTree, id: uuid::Uuid,
    expected_mesh_revision: Option<u64>, projection: Projection, polygon: Vec<[f32;2]>,
    depth: [f32;2], name: Option<String>) -> Result<uuid::Uuid,String> {
    let planes=planes(&polygon,depth)?;
    if model::scene_seams(draft).seams.iter().flat_map(|s| &s.points).any(|p|
        matches!(p,model::SeamPoint::Surface { object,.. } if *object==id)) {
        return Err("mesh has surface-attached seam points; detach them before separation".into());
    }
    let mut objects=model::objects(draft);
    let index=objects.iter().position(|o| o.uuid==id)
        .ok_or_else(|| format!("object {id} does not exist"))?;
    if objects.iter().any(|o| o.boolean_parent==Some(id)) {
        return Err("mesh has boolean children; detach them before separation".into());
    }
    let world=model::object_world_matrix(&objects,id);
    if !world.is_finite() || world.determinant().abs()<1e-12 {
        return Err("object world transform must be finite and invertible".into());
    }
    let mesh=objects[index].imported_mesh.as_ref().ok_or("object is not an editable mesh")?;
    if mesh.positions.len()>500_000 {
        return Err("polygon separation source exceeds 500,000 vertices; separate a smaller part first".into());
    }
    if expected_mesh_revision.is_some_and(|expected| expected!=mesh.revision) {
        return Err(format!("mesh revision conflict: expected {}, current {}",expected_mesh_revision.unwrap(),mesh.revision));
    }
    if mesh.triangles.len().saturating_mul(planes.len())>2_000_000 {
        return Err("polygon partition exceeds the 2,000,000 face-plane work limit".into());
    }
    if mesh.normals.len()!=mesh.triangles.len() || (!mesh.uvs.is_empty() && mesh.uvs.len()!=mesh.triangles.len())
        || (!mesh.colors.is_empty() && mesh.colors.len()!=mesh.positions.len())
        || mesh.triangles.iter().flatten().any(|&v|v as usize>=mesh.positions.len()) {
        return Err("source mesh has inconsistent geometry or attributes".into());
    }
    if mesh.positions.iter().any(|point| !point.is_finite() || !world.transform_point3(*point).is_finite()) {
        return Err("source mesh has non-finite local or world positions".into());
    }
    let new_id=uuid::Uuid::new_v4();
    let mut remainder=Mesh::from_parts(Vec::new(),Vec::new(),Vec::new());
    let mut extracted=Mesh::from_parts(Vec::new(),Vec::new(),Vec::new());
    for output in [&mut remainder,&mut extracted] {
        output.shading=mesh.shading; output.baked_texture=mesh.baked_texture.clone();
        output.revision=mesh.revision.wrapping_add(1);
    }
    let mut work=Workspace { source:mesh, positions:mesh.positions.clone(), colors:mesh.colors.clone(),
        world,inverse:world.inverse(),edges:HashMap::new() };
    let mut old_vertices=HashMap::new(); let mut new_vertices=HashMap::new();
    let textured=!mesh.uvs.is_empty() || mesh.baked_texture.is_some();
    for (face_index,face) in mesh.triangles.iter().enumerate() {
        let uv=if !mesh.uvs.is_empty() { mesh.uvs[face_index] }
            else if let Some(texture)=&mesh.baked_texture { baked_triangle_uv(face_index,texture.size,mesh.triangles.len()) }
            else { [[0.;2];3] };
        let mut inside=face.iter().enumerate().map(|(i,&vertex)| Corner {
            vertex,uv:uv[i],normal:mesh.normals[face_index][i]
        }).collect::<Vec<_>>();
        let world_triangle=face.map(|vertex| world.transform_point3(mesh.positions[vertex as usize]));
        if planes.iter().copied().any(|plane|
            world_triangle.iter().all(|&point| plane.distance(point,projection)<0.)) {
            emit(&inside,&mut remainder,&work,&mut old_vertices,id,textured)?;
            continue;
        }
        for (plane_index,plane) in planes.iter().copied().enumerate() {
            if inside.len()<3 { break; }
            let (next,outside)=work.split(&inside,plane_index,plane,projection);
            emit(&outside,&mut remainder,&work,&mut old_vertices,id,textured)?;
            inside=next;
        }
        emit(&inside,&mut extracted,&work,&mut new_vertices,new_id,textured)?;
        if remainder.triangles.len().saturating_add(extracted.triangles.len())>10_000_000 {
            return Err("polygon partition exceeds the 10,000,000 face limit".into());
        }
    }
    if remainder.triangles.is_empty() || extracted.triangles.is_empty() {
        return Err("polygon must select some faces and leave some faces in source mesh".into());
    }
    let mut part=objects[index].clone(); part.uuid=new_id;
    part.name=name.unwrap_or_else(|| format!("{} part",part.name));
    part.imported_mesh=Some(Arc::new(extracted));
    part.saved_mesh=None; part.saved_group_capture=None; part.saved_neural_field=None;
    objects[index].imported_mesh=Some(Arc::new(remainder));
    objects[index].saved_mesh=None; objects[index].saved_group_capture=None;
    objects[index].saved_neural_field=None;
    objects.push(part); model::set_objects(draft,objects);
    Ok(new_id)
}

// Ear clipping makes a simple concave outline a disjoint collection of convex
// prisms. Each ear is cut from the remaining source, so shared boundaries are
// not extracted twice and per-corner source attributes survive every cut.
fn polygon_ears(polygon: &[[f32; 2]]) -> Result<Vec<Vec<[f32; 2]>>, String> {
    if !(3..=32).contains(&polygon.len()) || polygon.iter().flatten().any(|v| !v.is_finite()) {
        return Err("polygon needs 3–32 finite points".into());
    }
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
    };
    let area = (0..polygon.len()).map(|i| {
        let a=polygon[i]; let b=polygon[(i+1)%polygon.len()];
        a[0]*b[1]-b[0]*a[1]
    }).sum::<f32>();
    if area.abs() <= 1e-10 { return Err("polygon area is zero".into()); }
    let sign=area.signum();
    for i in 0..polygon.len() {
        let a=polygon[i]; let b=polygon[(i+1)%polygon.len()];
        if (a[0]-b[0]).hypot(a[1]-b[1])<=1e-8 {
            return Err("polygon has a repeated or near-duplicate point".into());
        }
        for j in i+1..polygon.len() {
            if j==i+1 || (i==0 && j==polygon.len()-1) { continue; }
            let c=polygon[j]; let d=polygon[(j+1)%polygon.len()];
            let ab_c=cross(a,b,c); let ab_d=cross(a,b,d);
            let cd_a=cross(c,d,a); let cd_b=cross(c,d,b);
            if ab_c*ab_d<0. && cd_a*cd_b<0. {
                return Err("polygon must not self-intersect".into());
            }
        }
    }
    let mut indices=(0..polygon.len()).collect::<Vec<_>>();
    let mut ears=Vec::with_capacity(polygon.len()-2);
    while indices.len()>3 {
        let mut chosen=None;
        for at in 0..indices.len() {
            let prev=polygon[indices[(at+indices.len()-1)%indices.len()]];
            let point=polygon[indices[at]];
            let next=polygon[indices[(at+1)%indices.len()]];
            if cross(prev,point,next)*sign<=1e-10 { continue; }
            if indices.iter().enumerate().any(|(other,&index)| {
                if other==at || other==(at+indices.len()-1)%indices.len() || other==(at+1)%indices.len() { return false; }
                let p=polygon[index];
                // A vertex on an ear diagonal must also block the ear. Otherwise
                // the remaining outline can fold across that vertex and overlap.
                cross(prev,point,p)*sign>=-1e-10 && cross(point,next,p)*sign>=-1e-10
                    && cross(next,prev,p)*sign>=-1e-10
            }) { continue; }
            chosen=Some(at); break;
        }
        let at=chosen.ok_or("polygon contains a degenerate turn or overlapping edges")?;
        ears.push(vec![polygon[indices[(at+indices.len()-1)%indices.len()]],
            polygon[indices[at]], polygon[indices[(at+1)%indices.len()]]]);
        indices.remove(at);
    }
    ears.push(indices.iter().map(|&i|polygon[i]).collect());
    Ok(ears)
}

pub(crate) fn separate_mesh_polygon(draft: &mut model::DataTree, id: uuid::Uuid,
    expected_mesh_revision: Option<u64>, projection: Projection, polygon: Vec<[f32;2]>,
    depth: [f32;2], name: Option<String>) -> Result<uuid::Uuid,String> {
    // The convex path keeps its existing single-pass partition and revision behavior.
    if planes(&polygon,depth).is_ok() {
        return separate_convex_mesh_polygon(draft,id,expected_mesh_revision,projection,polygon,depth,name);
    }
    if !depth[0].is_finite() || !depth[1].is_finite() || depth[0]>=depth[1] {
        return Err("depth interval must be finite and increasing".into());
    }
    let ears=polygon_ears(&polygon)?;
    let mut work=draft.clone();
    let mut parts=Vec::with_capacity(ears.len());
    let mut revision=expected_mesh_revision;
    for ear in ears {
        match separate_convex_mesh_polygon(&mut work,id,revision,projection,ear,depth,name.clone()) {
            Ok(part) => {
                parts.push(part);
                revision=model::objects_ref(&work).iter().find(|object|object.uuid==id)
                    .and_then(|object|object.imported_mesh.as_ref().map(|mesh|mesh.revision));
            }
            Err(error) if error=="polygon must select some faces and leave some faces in source mesh" => {}
            Err(error) => return Err(error),
        }
    }
    if parts.is_empty() { return Err("polygon must select some faces and leave some faces in source mesh".into()); }
    let mut objects=model::objects(&work);
    let first=parts[0];
    let first_index=objects.iter().position(|object|object.uuid==first).unwrap();
    let mut merged=objects[first_index].imported_mesh.as_ref().unwrap().as_ref().clone();
    for part in parts.iter().skip(1) {
        let index=objects.iter().position(|object|object.uuid==*part).unwrap();
        let mesh=objects[index].imported_mesh.as_ref().unwrap();
        let base=merged.positions.len() as u32;
        merged.positions.extend_from_slice(&mesh.positions);
        merged.colors.extend_from_slice(&mesh.colors);
        merged.triangles.extend(mesh.triangles.iter().map(|face|face.map(|vertex|vertex+base)));
        merged.normals.extend_from_slice(&mesh.normals);
        merged.uvs.extend_from_slice(&mesh.uvs);
        merged.owners.extend(std::iter::repeat(first).take(mesh.triangles.len()));
    }
    merged.revision=revision.unwrap();
    objects[first_index].imported_mesh=Some(Arc::new(merged));
    objects.retain(|object| !parts[1..].contains(&object.uuid));
    model::set_objects(&mut work,objects);
    *draft=work;
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App,agent::{Action,ApplyArgs}};
    use crate::app::mesh_actions::create_mesh;

    fn area(mesh: &Mesh) -> f32 {
        mesh.triangles.iter().map(|f| {
            let [a,b,c]=f.map(|i|mesh.positions[i as usize]);
            (b-a).cross(c-a).length()*0.5
        }).sum()
    }
    fn fixture() -> (model::DataTree,uuid::Uuid) {
        let mut tree=model::DataTree::default();
        let id=create_mesh(&mut tree,Some("shell".into()),
            vec![[-2.,0.,-2.],[2.,0.,-2.],[0.,0.,2.]],vec![[0,1,2]],None).unwrap();
        let mut objects=model::objects(&tree);
        objects[0].transform.translation=Vec3::new(3.,2.,4.);
        let mesh=Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.uvs=vec![[[0.,0.],[1.,0.],[0.5,1.]]];
        mesh.colors=vec![[0.,0.,0.],[1.,0.,0.],[0.5,1.,0.]];
        model::set_objects(&mut tree,objects); (tree,id)
    }
    fn rectangle() -> Vec<[f32;2]> { vec![[2.5,3.5],[3.5,3.5],[3.5,4.5],[2.5,4.5]] }

    #[test]
    fn partitions_world_prism_without_area_loss_and_keeps_attributes() {
        let (mut tree,id)=fixture();
        let original_area=area(model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap());
        let new_id=separate_mesh_polygon(&mut tree,id,Some(0),Projection::Xz,rectangle(),[1.9,2.1],
            Some("window".into())).unwrap();
        let objects=model::objects_ref(&tree);
        assert_eq!(objects.len(),2);
        assert_eq!(objects[1].uuid,new_id);
        assert_eq!(objects[1].name,"window");
        let old=objects[0].imported_mesh.as_ref().unwrap();
        let new=objects[1].imported_mesh.as_ref().unwrap();
        assert!((area(old)+area(new)-original_area).abs()<1e-4);
        assert!((area(new)-1.).abs()<1e-4);
        assert_eq!(new.uvs.len(),new.triangles.len());
        assert_eq!(new.normals.len(),new.triangles.len());
        assert_eq!(new.colors.len(),new.positions.len());
        assert!(new.colors.iter().any(|color| color[0]>0. && color[0]<1.));
        assert!(new.positions.iter().all(|p| {
            let w=model::object_world_matrix(objects,new_id).transform_point3(*p);
            w.x>=2.5-1e-5 && w.x<=3.5+1e-5 && w.z>=3.5-1e-5 && w.z<=4.5+1e-5
        }));
    }

    #[test]
    fn outside_depth_face_crossing_projected_edges_remains_one_face() {
        let mut tree=model::DataTree::default();
        let id=create_mesh(&mut tree,Some("two layers".into()),vec![
            [-0.25,0.5,-0.25],[0.25,0.5,-0.25],[0.,0.5,0.25],
            [-2.,2.,-2.],[2.,2.,-2.],[0.,2.,2.],
        ],vec![[0,1,2],[3,4,5]],None).unwrap();
        let mut objects=model::objects(&tree);
        let mesh=Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.uvs=vec![
            [[0.,0.],[0.5,0.],[0.25,0.5]],
            [[0.1,0.2],[0.9,0.2],[0.5,0.8]],
        ];
        mesh.colors=vec![
            [1.,0.,0.],[1.,0.,0.],[1.,0.,0.],
            [0.,0.,1.],[0.,1.,0.],[1.,0.,1.],
        ];
        let original_positions=mesh.positions[3..6].to_vec();
        let original_colors=mesh.colors[3..6].to_vec();
        let original_normals=mesh.normals[1];
        let original_uvs=mesh.uvs[1];
        model::set_objects(&mut tree,objects);

        separate_mesh_polygon(&mut tree,id,Some(0),Projection::Xz,
            vec![[-0.5,-0.5],[0.5,-0.5],[0.5,0.5],[-0.5,0.5]],
            [0.,1.],None).unwrap();
        let objects=model::objects_ref(&tree);
        let remainder=objects[0].imported_mesh.as_ref().unwrap();
        let extracted=objects[1].imported_mesh.as_ref().unwrap();
        assert_eq!(remainder.triangles.len(),1);
        assert_eq!(extracted.triangles.len(),1);
        assert_eq!(remainder.normals[0],original_normals);
        assert_eq!(remainder.uvs[0],original_uvs);
        for (corner,&vertex) in remainder.triangles[0].iter().enumerate() {
            assert_eq!(remainder.positions[vertex as usize],original_positions[corner]);
            assert_eq!(remainder.colors[vertex as usize],original_colors[corner]);
        }
    }

    #[test]
    fn invalid_polygon_and_revision_leave_scene_unchanged() {
        let (mut tree,id)=fixture();
        let original=model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().triangles.clone();
        for polygon in [
            vec![[0.,0.],[1.,1.],[0.,1.],[1.,0.]],
            vec![[0.,0.],[1.,0.],[1.,0.],[0.,1.]],
            vec![[0.,1.],[0.59,-0.81],[-0.95,0.31],[0.95,0.31],[-0.59,-0.81]],
        ] {
            assert!(separate_mesh_polygon(&mut tree,id,None,Projection::Xy,polygon,[0.,1.],None).is_err());
        }
        assert!(separate_mesh_polygon(&mut tree,id,Some(1),Projection::Xz,rectangle(),[1.9,2.1],None)
            .unwrap_err().contains("revision conflict"));
        assert_eq!(model::objects_ref(&tree).len(),1);
        assert_eq!(model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap().triangles,original);
    }

    #[test]
    fn concave_polygon_partitions_source_without_area_loss() {
        let mut tree=model::DataTree::default();
        let id=create_mesh(&mut tree,Some("plane".into()),vec![
            [-1.,-1.,0.],[3.,-1.,0.],[3.,3.,0.],[-1.,3.,0.],
        ],vec![[0,1,2],[0,2,3]],None).unwrap();
        let mut objects=model::objects(&tree);
        let mesh=Arc::make_mut(objects[0].imported_mesh.as_mut().unwrap());
        mesh.uvs=vec![[[0.,0.],[1.,0.],[1.,1.]],[[0.,0.],[1.,1.],[0.,1.]]];
        model::set_objects(&mut tree,objects);
        let initial=area(model::objects_ref(&tree)[0].imported_mesh.as_ref().unwrap());
        let part=separate_mesh_polygon(&mut tree,id,Some(0),Projection::Xy,
            vec![[0.,0.],[2.,0.],[2.,1.],[1.,1.],[1.,2.],[0.,2.]],[-0.1,0.1],None).unwrap();
        let objects=model::objects_ref(&tree);
        assert_eq!(objects.len(),2);
        let extracted=objects.iter().find(|object|object.uuid==part).unwrap().imported_mesh.as_ref().unwrap();
        let remainder=objects.iter().find(|object|object.uuid==id).unwrap().imported_mesh.as_ref().unwrap();
        assert!((area(extracted)-3.).abs()<1e-4,"extracted area={}, remainder={}, initial={}",area(extracted),area(remainder),initial);
        assert!((area(extracted)+area(remainder)-initial).abs()<1e-4);
        assert_eq!(extracted.uvs.len(),extracted.triangles.len());
        assert_eq!(extracted.normals.len(),extracted.triangles.len());
    }

    #[test]
    fn action_returns_created_id_and_undo_restores_source() {
        let (tree,id)=fixture();
        let mut app=App::new(); app.new_document(); app.tree=tree;
        let result=app.agent_apply(ApplyArgs { expected_revision:None,actions:vec![
            Action::SeparateMeshPolygon { id,expected_mesh_revision:Some(0),
                projection:Projection::Xz,polygon:rectangle(),depth:[1.9,2.1],name:None }
        ]}).unwrap();
        assert_eq!(result["created_ids"].as_array().unwrap().len(),1);
        assert_eq!(model::objects_ref(&app.tree).len(),2);
        app.tree.undo();
        assert_eq!(model::objects_ref(&app.tree).len(),1);
        assert_eq!(model::objects_ref(&app.tree)[0].imported_mesh.as_ref().unwrap().triangles.len(),1);
    }
}
