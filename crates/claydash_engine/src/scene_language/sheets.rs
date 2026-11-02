//! Typed declarative mappings to existing native sheet operations.
use super::construction::{list, no_extra, required, text};
use super::*;
use crate::model::{
    SheetCut, SheetEmboss, SheetPathEmboss, SheetPathProfile, SheetPathStation, SheetStage,
    SheetSurface,
};
use glam::Vec2;

fn point(value: Value) -> Result<Vec2, String> {
    let values = list(value)?;
    let [x, y]: [Value; 2] = values.try_into().map_err(|_| "expected a 2D point")?;
    Ok(Vec2::new(number(x)?, number(y)?))
}
pub(super) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "cut_line"
            | "cut_circle"
            | "emboss_circle"
            | "sheet_surface"
            | "emboss_station"
            | "path_emboss"
    )
}
pub(super) fn evaluate_call(
    name: &str,
    pos: &[Value],
    args: &mut BTreeMap<String, Value>,
) -> Result<Option<Value>, String> {
    if !reserved(name) {
        return Ok(None);
    }
    if !pos.is_empty() {
        return Err(format!("{name} accepts named arguments only"));
    }
    let value = match name {
        "cut_line" => Value::Cut(SheetCut::Straight {
            from: point(required(args, "from")?)?,
            to: point(required(args, "to")?)?,
        }),
        "cut_circle" => Value::Cut(SheetCut::Circle {
            center: point(required(args, "center")?)?,
            radius: number(required(args, "radius")?)?,
        }),
        "emboss_circle" => Value::Emboss(SheetEmboss::Circle {
            center: point(required(args, "center")?)?,
            radius: number(required(args, "radius")?)?,
            depth: number(required(args, "depth")?)?,
            transition: number(required(args, "transition")?)?,
        }),
        "sheet_surface" => {
            let heights = list(required(args, "heights")?)?
                .into_iter()
                .map(number)
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| {
                    "sheet_surface requires exactly 16 control heights (X varies fastest)"
                })?;
            Value::Surface(SheetSurface { heights })
        }
        "emboss_station" => {
            let point = point(required(args, "point")?)?;
            let width = number(required(args, "width")?)?;
            let depth = number(required(args, "depth")?)?;
            let profile = match args.remove("profile").map(text).transpose()?.as_deref() {
                None | Some("Rounded") => SheetPathProfile::Rounded,
                Some("Crease") => SheetPathProfile::Crease,
                _ => return Err("emboss profile must be Rounded or Crease".into()),
            };
            Value::PathStation(SheetPathStation {
                point,
                width,
                depth,
                profile,
            })
        }
        "path_emboss" => {
            let stations = list(required(args, "stations")?)?
                .into_iter()
                .map(|v| match v {
                    Value::PathStation(v) => Ok(v),
                    _ => Err("stations must contain emboss_station(...) values"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Value::PathEmboss(SheetPathEmboss {
                stations,
                taper_start: number(required(args, "taper_start")?)?,
                taper_end: number(required(args, "taper_end")?)?,
            })
        }
        _ => unreachable!(),
    };
    no_extra(args, name)?;
    Ok(Some(value))
}
pub(super) fn apply_params(
    object: &mut SdfObject,
    args: &mut BTreeMap<String, Value>,
) -> Result<(), String> {
    for key in ["cuts", "embosses", "surface", "path_embosses", "stage"] {
        let Some(value) = args.remove(key) else {
            continue;
        };
        let SdfParams::SheetPanelParams(panel) = &mut object.params else {
            return Err(format!("{key} is only supported on sheet_panel"));
        };
        match key {
            "cuts" => {
                panel.cuts = list(value)?
                    .into_iter()
                    .map(|v| match v {
                        Value::Cut(v) => Ok(v),
                        _ => Err("cuts must contain cut_line(...) or cut_circle(...) values"),
                    })
                    .collect::<Result<_, _>>()?
            }
            "embosses" => {
                panel.embosses = list(value)?
                    .into_iter()
                    .map(|v| match v {
                        Value::Emboss(v) => Ok(v),
                        _ => Err("embosses must contain emboss_circle(...) values"),
                    })
                    .collect::<Result<_, _>>()?
            }
            "surface" => {
                let Value::Surface(v) = value else {
                    return Err("surface requires sheet_surface(...)".into());
                };
                panel.surface = Some(v);
            }
            "path_embosses" => {
                panel.path_embosses = list(value)?
                    .into_iter()
                    .map(|v| match v {
                        Value::PathEmboss(v) => Ok(v),
                        _ => Err("path_embosses must contain path_emboss(...) values"),
                    })
                    .collect::<Result<_, _>>()?
            }
            "stage" => {
                panel.stage = match text(value)?.as_str() {
                    "Blank" => SheetStage::Blank,
                    "Cut" => SheetStage::Cut,
                    "Formed" => SheetStage::Formed,
                    _ => return Err("sheet stage must be Blank, Cut or Formed".into()),
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PANEL: &str = r#"
panel = sheet_panel(size:[2,1], thickness:0.02,
 cuts:[cut_line(from:[0.9,-0.5],to:[0.9,0.5]), cut_circle(center:[-0.6,-0.4],radius:0.25)],
 embosses:[emboss_circle(center:[-0.6,-0.4],radius:0.25,depth:0.03,transition:0.12)],
 surface:sheet_surface(heights:[0,0,0,0, 0,0.15,0.15,0, 0,0.15,0.15,0, 0,0,0,0]),
 path_embosses:[path_emboss(stations:[emboss_station(point:[0,-0.25],width:0.08,depth:0.01),emboss_station(point:[0,0.25],width:0.1,depth:0.015,profile:"Crease")],taper_start:0.1,taper_end:0.1)],
 bends:[bend(start:0.6,radius:0.4,angle:35)],stage:"Formed");
"#;
    fn panel(source: &str) -> crate::model::SheetPanelParams {
        let scene = compile_scene(source).unwrap();
        let SdfParams::SheetPanelParams(p) = scene.parts[0].1.params.clone() else {
            panic!()
        };
        p
    }
    #[test]
    fn cuts_crown_relief_and_bends_compose_as_native_geometry() {
        let p = panel(PANEL);
        assert_eq!(p.cuts.len(), 2);
        assert_eq!(p.embosses.len(), 1);
        assert_eq!(p.path_embosses.len(), 1);
        assert_eq!(p.bends.len(), 1);
        assert!(p.material_point(Vec2::ZERO).z > 0.08);
        assert!(
            p.distance(p.material_point(Vec2::new(-0.6, -0.4))) > 0.001,
            "cut must remove geometry"
        );
        assert!(
            p.distance(p.material_point(Vec2::new(0.95, 0.))) > 0.,
            "directed cut retains left side"
        );
        assert!(p.distance(p.material_point(Vec2::new(-0.2, 0.))) < 0.);
        assert!(matches!(
            p.path_embosses[0].stations[1].profile,
            SheetPathProfile::Crease
        ));
    }
    #[test]
    fn stages_change_geometry_without_discarding_recipe() {
        let source = r#"p=sheet_panel(size:[2,1],thickness:0.02,cuts:[cut_circle(center:[0,0],radius:0.2)],embosses:[emboss_circle(center:[0.5,0],radius:0.1,depth:0.06,transition:0.15)],stage:"Formed");"#;
        let formed = panel(source);
        let cut = panel(&source.replace("Formed", "Cut"));
        let blank = panel(&source.replace("Formed", "Blank"));
        assert!(formed.distance(Vec3::ZERO) > 0.);
        assert!(cut.distance(Vec3::ZERO) > 0.);
        assert!(blank.distance(Vec3::ZERO) < 0.);
        assert!((formed.material_point(Vec2::new(0.5, 0.)).z - 0.06).abs() < 1e-6);
        assert!(cut.material_point(Vec2::new(0.5, 0.)).z.abs() < 1e-6);
        assert_eq!(blank.cuts.len(), 1);
        assert_eq!(blank.embosses.len(), 1);
    }
    #[test]
    fn invalid_operations_use_native_validation_and_typed_arguments() {
        for arg in [
            "cuts:[cut_circle(center:[0,0],radius:0)]",
            "cuts:[cut_line(from:[0,0],to:[0,0])]",
            "cuts:[emboss_circle(center:[0,0],radius:0.2,depth:0.01,transition:0.1)]",
            "embosses:[emboss_circle(center:[0,0],radius:0.2,depth:0,transition:0.1)]",
            "embosses:[emboss_circle(center:[0,0],radius:0.2,depth:1,transition:0.0001)]",
            "surface:sheet_surface(heights:[0,0])",
            "surface:[0,0,0]",
            "path_embosses:[path_emboss(stations:[emboss_station(point:[0,0],width:0.1,depth:0.01)],taper_start:0.1,taper_end:0.1)]",
            "path_embosses:[path_emboss(stations:[emboss_station(point:[0,0],width:0.1,depth:0.01,profile:\"crease\")],taper_start:0.1,taper_end:0.1)]",
            "cuts:[cut_circle(center:[0,0,0],radius:0.2)]",
            "cuts:[cut_circle(center:[0,0],radius:0.2,typo:0)]",
            "stage:\"formed\"",
        ]{assert!(compile_scene(&format!("p=sheet_panel({arg});")).is_err(),"{arg}");}
        assert!(compile_scene("p=box(cuts:[]);").is_err());
        assert!(compile_scene("object cut_circle({}): p=box(); end").is_err());
        let cuts = vec!["cut_circle(center:[0,0],radius:0.1)"; 33].join(",");
        assert!(compile_scene(&format!("p=sheet_panel(cuts:[{cuts}]);")).is_err());
    }
}
