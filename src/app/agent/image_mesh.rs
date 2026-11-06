//! Exact image-pixel to source-prism mapping for calibrated orthographic panels.
use super::*;
use serde::Serialize;
use crate::app::mesh_polygon_separate::Projection;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MeshImagePanel {
    pub scene_revision: u64,
    pub projection: Projection,
    pub image_origin: [u32; 2],
    pub panel_size: [u32; 2],
    pub world_at_origin: [f32; 2],
    pub world_per_pixel_x: [f32; 2],
    pub world_per_pixel_y: [f32; 2],
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshImagePolygonArgs {
    pub id: uuid::Uuid,
    pub expected_mesh_revision: u64,
    pub panel: MeshImagePanel,
    pub polygon_pixels: Vec<[f32; 2]>,
    pub depth: [f32; 2],
    pub name: Option<String>,
}

impl MeshImagePanel {
    pub fn from_camera(camera: &Camera, image_origin: [u32; 2], projection: Projection,
        scene_revision: u64) -> Result<Self, String> {
        if camera.projection_mode != ProjectionMode::Orthographic {
            return Err("image mesh panels require an orthographic camera".into());
        }
        let point = |x: f32, y: f32| -> Result<[f32; 2], String> {
            let (origin, ray)=camera.ray(Vec2::new(x,y));
            let (u,v,d)=projection.coordinates(origin);
            let (ru,rv,rd)=projection.coordinates(ray);
            if rd.abs()<0.99 { return Err("image mesh panel camera is not aligned with its projection axis".into()); }
            let t=-d/rd;
            Ok([u+ru*t,v+rv*t])
        };
        let a=point(0.,0.)?; let x=point(1.,0.)?; let y=point(0.,1.)?;
        Ok(Self { scene_revision, projection, image_origin,
            panel_size:[camera.viewport.x as u32,camera.viewport.y as u32],
            world_at_origin:a,world_per_pixel_x:[x[0]-a[0],x[1]-a[1]],
            world_per_pixel_y:[y[0]-a[0],y[1]-a[1]] })
    }

    pub fn polygon(&self, pixels: &[[f32; 2]], current_revision: u64)
        -> Result<Vec<[f32; 2]>, String> {
        if self.scene_revision!=current_revision {
            return Err(format!("capture scene revision conflict: captured {}, current {}; capture again",self.scene_revision,current_revision));
        }
        if !(3..=32).contains(&pixels.len()) || self.panel_size.contains(&0)
            || self.panel_size.iter().any(|&size|size>2048)
            || self.world_at_origin.iter().chain(self.world_per_pixel_x.iter())
                .chain(self.world_per_pixel_y.iter()).any(|v|!v.is_finite()) {
            return Err("image polygon needs 3–32 points and finite calibrated panel metadata".into());
        }
        let determinant=self.world_per_pixel_x[0]*self.world_per_pixel_y[1]
            -self.world_per_pixel_x[1]*self.world_per_pixel_y[0];
        if determinant.abs()<1e-14 { return Err("image panel calibration is singular".into()); }
        pixels.iter().map(|&pixel| {
            let x=pixel[0]-self.image_origin[0] as f32;
            let y=pixel[1]-self.image_origin[1] as f32;
            if !x.is_finite() || !y.is_finite() || x<0. || y<0.
                || x>self.panel_size[0] as f32 || y>self.panel_size[1] as f32 {
                return Err("polygon pixel lies outside its captured panel".into());
            }
            Ok([self.world_at_origin[0]+x*self.world_per_pixel_x[0]+y*self.world_per_pixel_y[0],
                self.world_at_origin[1]+x*self.world_per_pixel_x[1]+y*self.world_per_pixel_y[1]])
        }).collect()
    }
}

pub(super) fn validate_source(tree: &model::DataTree, id: uuid::Uuid) -> Result<(),String> {
    let object=model::objects_ref(tree).iter().find(|object|object.uuid==id)
        .ok_or_else(||format!("object {id} does not exist"))?;
    if object.imported_mesh.is_none() { return Err("object is not an editable source mesh".into()); }
    if object.mirror.is_some() || object.lattice.is_some() || object.repetition.enabled
        || object.path_extrusion.is_some() || object.surface_inlay.is_some()
        || object.image_stencil.is_some() {
        return Err("image polygon separation requires an unmodified source mesh; disable mirror, lattice, repetition and other geometry modifiers before capture".into());
    }
    Ok(())
}

pub(super) fn preview(tree: &model::DataTree, scene_revision: u64,
    args: MeshImagePolygonArgs) -> AgentResult {
    validate_source(tree,args.id)?;
    let polygon=args.panel.polygon(&args.polygon_pixels,scene_revision)?;
    let mut draft=tree.clone();
    let part=crate::app::mesh_polygon_separate::separate_mesh_polygon(&mut draft,args.id,
        Some(args.expected_mesh_revision),args.panel.projection,polygon.clone(),args.depth,args.name.clone())?;
    let objects=model::objects_ref(&draft);
    let source=objects.iter().find(|object|object.uuid==args.id).unwrap().imported_mesh.as_ref().unwrap();
    let selected=objects.iter().find(|object|object.uuid==part).unwrap().imported_mesh.as_ref().unwrap();
    let middle=(args.depth[0]+args.depth[1])*0.5;
    let contour_preview_points=polygon.iter().map(|&[u,v]| {
        let position=match args.panel.projection {
            Projection::Xy => [u,v,middle],
            Projection::Xz => [u,middle,v],
            Projection::Yz => [middle,u,v],
        };
        json!({"position":position})
    }).collect::<Vec<_>>();
    Ok(json!({"id":args.id,"scene_revision":scene_revision,"mesh_revision":args.expected_mesh_revision,
        "projection":args.panel.projection,"world_polygon":polygon,"depth":args.depth,
        "selected_triangles":selected.triangles.len(),"remaining_triangles":source.triangles.len(),
        "contour_preview_points":contour_preview_points,
        "preview":"exact source geometry partition; no scene mutation",
        "apply_action":{"type":"SeparateMeshImagePolygon","id":args.id,
            "expected_mesh_revision":args.expected_mesh_revision,"panel":args.panel,
            "polygon_pixels":args.polygon_pixels,"depth":args.depth,"name":args.name}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_orthographic_pixels_round_trip_all_panel_axes_and_offsets() {
        for (projection,direction,up,offset,world) in [
            (Projection::Yz,Vec3::X,Vec3::Y,[0,0],Vec3::new(0.,0.2,-0.4)),
            (Projection::Xz,Vec3::Y,Vec3::NEG_Z,[320,0],Vec3::new(0.3,0.,-0.2)),
            (Projection::Xy,Vec3::Z,Vec3::Y,[640,0],Vec3::new(-0.3,0.4,0.)),
        ] {
            let mut camera=Camera::new();
            camera.position=direction*8.; camera.target=Vec3::ZERO; camera.up=up;
            camera.projection_mode=ProjectionMode::Orthographic;
            camera.viewport=Vec2::splat(320.);
            camera.fit_bounds(Vec3::splat(-1.),Vec3::splat(1.),10.).unwrap();
            let panel=MeshImagePanel::from_camera(&camera,offset,projection,42).unwrap();
            let local=camera.project(world,1.).unwrap();
            let pixel=[local.x+offset[0] as f32,local.y+offset[1] as f32];
            let points=panel.polygon(&[pixel,pixel,pixel],42).unwrap();
            let (u,v,_)=projection.coordinates(world);
            assert!((points[0][0]-u).abs()<2e-5 && (points[0][1]-v).abs()<2e-5);
            assert!(panel.polygon(&[pixel,pixel,pixel],43).unwrap_err().contains("revision"));
            assert!(panel.polygon(&[[offset[0] as f32-1.,0.],pixel,pixel],42).is_err());
        }
    }
}
