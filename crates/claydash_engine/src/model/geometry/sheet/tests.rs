//! Regression coverage for sheet recipes, forming and relief geometry.
use super::*;
use glam::{Vec2, Vec3};
fn crowned_path_panel() -> SheetPanelParams {
    SheetPanelParams {
        size: Vec2::new(0.9, 0.7),
        thickness: 0.008,
        surface: Some(SheetSurface {
            heights: [
                0.0, 0.04, 0.04, 0.0, 0.04, 0.12, 0.12, 0.04, 0.04, 0.12, 0.12, 0.04, 0.0, 0.04,
                0.04, 0.0,
            ],
        }),
        path_embosses: vec![SheetPathEmboss {
            stations: vec![
                SheetPathStation {
                    point: Vec2::new(-0.35, 0.0),
                    width: 0.04,
                    depth: 0.01,
                    profile: SheetPathProfile::Rounded,
                },
                SheetPathStation {
                    point: Vec2::new(0.0, 0.08),
                    width: 0.075,
                    depth: 0.025,
                    profile: SheetPathProfile::Crease,
                },
                SheetPathStation {
                    point: Vec2::new(0.35, 0.0),
                    width: 0.04,
                    depth: 0.01,
                    profile: SheetPathProfile::Rounded,
                },
            ],
            taper_start: 0.12,
            taper_end: 0.12,
        }],
        cuts: vec![SheetCut::Circle {
            center: Vec2::new(0.25, -0.2),
            radius: 0.04,
        }],
        ..Default::default()
    }
}

#[test]
fn compound_surface_curves_both_directions_and_path_varies_and_tapers() {
    let panel = crowned_path_panel();
    assert!(panel.validate().is_ok());
    let surface = panel.surface.as_ref().unwrap();
    let centre = surface.height_at(Vec2::ZERO, panel.size);
    assert!(centre > surface.height_at(Vec2::X * 0.3, panel.size));
    assert!(centre > surface.height_at(Vec2::Y * 0.25, panel.size));
    let path = &panel.path_embosses[0];
    assert!(path.height_at(path.stations[0].point).abs() < 1e-6);
    assert!(path.height_at(path.stations[2].point).abs() < 1e-6);
    assert!((path.height_at(path.stations[1].point) - 0.025).abs() < 1e-6);
    let early = path.stations[0].point.lerp(path.stations[1].point, 0.4);
    assert!(path.height_at(early) > 0.0 && path.height_at(early) < 0.025);
    let mut recess = path.clone();
    for s in &mut recess.stations {
        s.depth = -s.depth;
    }
    for uv in [early, Vec2::ZERO, Vec2::new(0.0, 0.12)] {
        assert!((path.height_at(uv) + recess.height_at(uv)).abs() < 1e-7);
    }
}

#[test]
fn compound_path_material_cuts_bounds_stages_and_bends_agree() {
    let mut panel = crowned_path_panel();
    for direction in [0.0, 0.7] {
        panel.bends = vec![SheetBend {
            start: 0.1,
            direction,
            radius: 0.5,
            angle: 0.5,
        }];
        assert!(panel.validate().is_ok());
        let bounds = panel.local_extent();
        for i in 0..31 {
            for j in 0..25 {
                let uv = Vec2::new(-0.45 + 0.9 * i as f32 / 30.0, -0.35 + 0.7 * j as f32 / 24.0);
                for z in [-0.004, 0.0, 0.004] {
                    let formed = panel.form_point(uv.extend(z));
                    assert!(formed.abs().cmple(bounds + Vec3::splat(1e-5)).all());
                    if panel.outline_distance(uv) < -1e-4 {
                        assert!(panel.distance(formed) <= 1e-5, "lost {uv:?}");
                    }
                }
            }
        }
        let removed = Vec2::new(0.25, -0.2);
        assert!(panel.distance(panel.material_point(removed)) > 0.0);
        let recipe = serde_json::to_value(&panel).unwrap();
        for stage in [SheetStage::Blank, SheetStage::Cut] {
            panel.stage = stage;
            assert_eq!(panel.material_point(Vec2::ZERO), Vec3::ZERO);
            assert_eq!(
                panel.distance(removed.extend(0.0)) < 0.0,
                stage == SheetStage::Blank
            );
        }
        panel.stage = SheetStage::Formed;
        assert_eq!(serde_json::to_value(&panel).unwrap(), recipe);
        assert_eq!(
            serde_json::to_value(
                serde_json::from_value::<SheetPanelParams>(recipe.clone()).unwrap()
            )
            .unwrap(),
            recipe
        );
    }
}

#[test]
fn surface_path_validation_rejects_unbounded_and_singular_operations() {
    let mut panel = crowned_path_panel();
    panel.surface.as_mut().unwrap().heights[0] = f32::NAN;
    assert!(panel.validate().is_err());
    for invalid in [0.0, -0.1, f32::INFINITY, f32::NAN] {
        let mut panel = crowned_path_panel();
        panel.path_embosses[0].stations[0].width = invalid;
        assert!(panel.validate().is_err());
        let mut panel = crowned_path_panel();
        panel.path_embosses[0].taper_start = invalid;
        assert!(panel.validate().is_err());
    }
    let mut panel = crowned_path_panel();
    panel.path_embosses[0].stations[1].depth = -0.02;
    assert!(panel.validate().is_err());
    panel = crowned_path_panel();
    panel.path_embosses[0].stations[1].point = panel.path_embosses[0].stations[0].point;
    assert!(panel.validate().is_err());
    panel = crowned_path_panel();
    let station = panel.path_embosses[0].stations[0].clone();
    panel.path_embosses[0].stations.resize(9, station);
    assert!(panel.validate().is_err());
    panel = crowned_path_panel();
    panel
        .path_embosses
        .resize(5, panel.path_embosses[0].clone());
    assert!(panel.validate().is_err());
    panel = crowned_path_panel();
    panel.surface.as_mut().unwrap().heights[5] = 100.0;
    assert!(panel.validate().unwrap_err().contains("too steep"));
    panel = crowned_path_panel();
    panel.path_embosses[0].stations[1].width = 0.00001;
    assert!(panel.validate().unwrap_err().contains("too steep"));
    panel = crowned_path_panel();
    panel.surface.as_mut().unwrap().heights = [1e30; 16];
    assert!(panel.validate().unwrap_err().contains("bounds"));
}
fn arch_panel() -> SheetPanelParams {
    SheetPanelParams {
        size: Vec2::new(0.9, 0.7),
        thickness: 0.008,
        embosses: vec![SheetEmboss::Circle {
            center: Vec2::new(0.0, -0.32),
            radius: 0.2,
            depth: 0.04,
            transition: 0.1,
        }],
        cuts: vec![SheetCut::Circle {
            center: Vec2::new(0.0, -0.32),
            radius: 0.2,
        }],
        ..Default::default()
    }
}

#[test]
fn circular_emboss_forms_a_rounded_cut_lip_and_preserves_stage_recipe() {
    let mut p = arch_panel();
    assert!(p.validate().is_ok());
    let inner = Vec2::new(0.0, -0.12);
    let shoulder = Vec2::new(0.0, -0.07);
    let outer = Vec2::new(0.0, -0.02);
    assert!((p.material_point(inner).z - 0.04).abs() < 1e-6);
    assert!((p.material_point(shoulder).z - 0.02).abs() < 1e-6);
    assert!(p.material_point(outer).z.abs() < 1e-6);
    assert!(p.distance(p.material_point(shoulder)) < 0.0);
    assert!(
        p.distance(shoulder.extend(0.0)) > 0.005,
        "old flat surface must be gone"
    );
    assert!(
        p.distance(p.material_point(Vec2::new(0.0, -0.32))) > 0.0,
        "cut remains open"
    );
    assert!(
        p.distance(p.material_point(Vec2::new(0.0, -0.6))) > 0.0,
        "blank clips the relief"
    );
    // Tangents flatten at both ends of the rounded transition.
    for uv in [inner, outer] {
        let slope =
            (p.emboss_height(uv + Vec2::Y * 1e-5) - p.emboss_height(uv - Vec2::Y * 1e-5)) / 2e-5;
        assert!(slope.abs() < 0.001, "rounded transition tangent {slope}");
    }
    let recipe = serde_json::to_value(&p).unwrap();
    for stage in [SheetStage::Blank, SheetStage::Cut] {
        p.stage = stage;
        assert_eq!(p.material_point(shoulder), shoulder.extend(0.0));
        assert_eq!(
            p.distance(Vec3::new(0.0, -0.32, 0.0)) < 0.0,
            stage == SheetStage::Blank
        );
    }
    p.stage = SheetStage::Formed;
    assert_eq!(serde_json::to_value(&p).unwrap(), recipe);
    let SheetEmboss::Circle { depth, .. } = &mut p.embosses[0];
    *depth = -0.04;
    assert!((p.material_point(shoulder).z + 0.02).abs() < 1e-6);
    assert!(p.distance(p.material_point(shoulder)) < 0.0);
}

#[test]
fn embosses_follow_both_bend_modes_and_conservative_thickness_bounds() {
    let mut p = arch_panel();
    p.bends = vec![
        SheetBend {
            start: -0.3,
            direction: 0.0,
            radius: 0.24,
            angle: 0.8,
        },
        SheetBend {
            start: 0.05,
            direction: 0.0,
            radius: 0.2,
            angle: -0.7,
        },
    ];
    p.embosses.push(SheetEmboss::Circle {
        center: Vec2::new(0.22, 0.1),
        radius: 0.1,
        depth: -0.025,
        transition: 0.1,
    });
    for direction in [0.0, 0.9] {
        p.bends[1].direction = direction;
        assert!(p.validate().is_ok());
        let bound = p.local_extent();
        for i in 0..=30 {
            for j in 0..=20 {
                let uv = Vec2::new(-0.45 + i as f32 * 0.03, -0.35 + j as f32 * 0.035);
                for z in [-p.thickness * 0.5, 0.0, p.thickness * 0.5] {
                    let formed = p.form_point(uv.extend(z));
                    assert!(
                        formed.abs().cmple(bound + Vec3::splat(1e-5)).all(),
                        "lost embossed bound {formed:?}"
                    );
                    if p.outline_distance(uv) < -1e-4 {
                        assert!(
                            p.distance(formed) <= 1e-5,
                            "lost formed emboss {uv:?}: {}",
                            p.distance(formed)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn circular_emboss_validation_and_old_recipe_loading_are_explicit() {
    let old: SheetPanelParams =
        serde_json::from_value(serde_json::json!({"size":[0.9,0.7],"thickness":0.008})).unwrap();
    assert!(old.embosses.is_empty());
    assert!(old.validate().is_ok());
    let mut p = arch_panel();
    p.embosses.resize(9, p.embosses[0].clone());
    assert!(p.validate().is_err());
    for invalid in [0.0, -0.1, f32::NAN, f32::INFINITY] {
        let mut p = arch_panel();
        let SheetEmboss::Circle { transition, .. } = &mut p.embosses[0];
        *transition = invalid;
        assert!(p.validate().is_err());
    }
    let mut p = arch_panel();
    let SheetEmboss::Circle {
        depth, transition, ..
    } = &mut p.embosses[0];
    *depth = 0.04;
    *transition = 0.00001;
    assert!(p.validate().unwrap_err().contains("too steep"));
    p = arch_panel();
    p.bends.push(SheetBend {
        start: 0.0,
        direction: 0.0,
        radius: 0.03,
        angle: 0.7,
    });
    assert!(p.validate().unwrap_err().contains("radial clearance"));
    p.bends[0].direction = 0.7;
    assert!(p.validate().unwrap_err().contains("radial clearance"));
    let encoded = serde_json::to_value(arch_panel()).unwrap();
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<SheetPanelParams>(encoded.clone()).unwrap())
            .unwrap(),
        encoded
    );
}
#[test]
fn cuts_and_stages_preserve_recipe() {
    let mut p = SheetPanelParams::default();
    p.cuts.push(SheetCut::Circle {
        center: Vec2::ZERO,
        radius: 0.2,
    });
    p.stage = SheetStage::Blank;
    assert!(p.distance(Vec3::ZERO) < 0.0);
    p.stage = SheetStage::Cut;
    assert!(p.distance(Vec3::ZERO) > 0.0);
    assert!(p.distance(Vec3::new(0.5, 0.0, 0.0)) < 0.0);
    p.cuts.push(SheetCut::Straight {
        from: Vec2::ZERO,
        to: Vec2::Y,
    });
    assert!(p.distance(Vec3::new(0.5, 0.0, 0.0)) > 0.0);
    assert!(p.distance(Vec3::new(-0.5, 0.0, 0.0)) < 0.0);
}
#[test]
fn radius_bends_map_material_and_reject_invalid_recipes() {
    let mut p = SheetPanelParams::default();
    p.bends.push(SheetBend {
        start: -0.6,
        direction: 0.0,
        radius: 0.4,
        angle: 1.5,
    });
    assert!(p.validate().is_ok());
    for x in [-0.8, -0.5, -0.3, -0.1, 0.3, 0.8] {
        let center = p.material_point(Vec2::new(x, 0.0));
        assert!(p.distance(center) < 0.0, "{x}: {}", p.distance(center));
        assert!(p.distance(center + Vec3::Y) > 0.0);
    }
    p.bends.push(SheetBend {
        start: -0.2,
        direction: 0.0,
        radius: 0.2,
        angle: 1.0,
    });
    assert!(p.validate().is_err());
    p.bends.clear();
    p.thickness = f32::NAN;
    assert!(p.validate().is_err());
}

#[test]
fn negative_and_multiple_bends_preserve_cuts_and_bounds() {
    let mut p = SheetPanelParams::default();
    p.bends = vec![
        SheetBend {
            start: -0.9,
            direction: 0.0,
            radius: 0.2,
            angle: -1.2,
        },
        SheetBend {
            start: 0.1,
            direction: 0.0,
            radius: 0.3,
            angle: 1.5,
        },
    ];
    p.cuts.push(SheetCut::Circle {
        center: Vec2::new(-0.8, 0.0),
        radius: 0.05,
    });
    assert!(p.validate().is_ok());
    assert!(p.distance(p.material_point(Vec2::new(-0.8, 0.0))) > 0.0);
    let extent = p.local_extent();
    for i in 0..=200 {
        let uv = Vec2::new(-1.0 + i as f32 / 100.0, 0.3);
        let surface = p.material_point(uv);
        assert!(surface.abs().cmple(extent).all());
        assert!(
            p.distance(surface) <= 0.00001,
            "material point must lie in the sheet: {uv:?}"
        );
    }
    let encoded = serde_json::to_string(&p).unwrap();
    let decoded: SheetPanelParams = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.stage, p.stage);
    assert_eq!(decoded.bends.len(), 2);
}
fn directional_panel() -> SheetPanelParams {
    SheetPanelParams {
        bends: vec![
            SheetBend {
                start: 0.15,
                direction: 0.0,
                radius: 0.22,
                angle: 1.2,
            },
            SheetBend {
                start: -0.1,
                direction: std::f32::consts::FRAC_PI_2,
                radius: 0.2,
                angle: -1.1,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn perpendicular_bends_form_crossing_regions_in_recipe_order() {
    let p = directional_panel();
    assert!(p.validate().is_ok());
    let mut reversed = p.clone();
    reversed.bends.reverse();
    let uv = Vec2::new(0.75, 0.35);
    assert!(p.material_point(uv).distance(reversed.material_point(uv)) > 0.05);
    let extent = p.local_extent();
    for i in 0..=40 {
        for j in 0..=20 {
            for z in [-p.thickness * 0.5, 0.0, p.thickness * 0.5] {
                let flat = Vec3::new(-1.0 + i as f32 * 0.05, -0.5 + j as f32 * 0.05, z);
                let formed = p.form_point(flat);
                assert!(
                    formed.abs().cmple(extent + Vec3::splat(1e-5)).all(),
                    "bounds: {formed:?} {extent:?}"
                );
                assert!(
                    p.distance(formed) <= 1e-5,
                    "lost material {flat:?}: {}",
                    p.distance(formed)
                );
            }
        }
    }
    assert!(p.material_point(Vec2::new(0.9, -0.4)).z > 0.0);
    assert!(p.material_point(Vec2::new(-0.9, 0.4)).z < 0.0);
}

#[test]
fn directional_cuts_and_stage_views_preserve_original_material() {
    let mut p = directional_panel();
    let uv = Vec2::new(0.7, -0.35);
    p.cuts.push(SheetCut::Circle {
        center: uv,
        radius: 0.07,
    });
    assert!(p.distance(p.material_point(uv)) > 0.0);
    let recipe = serde_json::to_value(&p).unwrap();
    for stage in [SheetStage::Blank, SheetStage::Cut] {
        p.stage = stage;
        assert_eq!(p.material_point(uv), uv.extend(0.0));
        assert_eq!(p.distance(uv.extend(0.0)) < 0.0, stage == SheetStage::Blank);
    }
    p.stage = SheetStage::Formed;
    assert_eq!(serde_json::to_value(&p).unwrap(), recipe);
    p.cuts.push(SheetCut::Straight {
        from: Vec2::new(-0.8, 0.0),
        to: Vec2::new(-0.8, -1.0),
    });
    assert!(p.distance(p.material_point(Vec2::new(-0.95, 0.1))) > 0.0);
}

#[test]
fn directional_validation_and_legacy_defaults_are_explicit() {
    let old: SheetPanelParams = serde_json::from_value(serde_json::json!({
        "size": [2.0, 1.0], "thickness": 0.04,
        "bends": [{"start": -0.8, "radius": 0.2, "angle": -1.0}]
    }))
    .unwrap();
    assert!(!old.uses_directional_forming());
    assert!(old.validate().is_ok());
    let mut p = directional_panel();
    p.bends[0].direction = f32::NAN;
    assert!(p.validate().is_err());
    p = directional_panel();
    p.bends.resize(5, p.bends[0].clone());
    assert!(p.validate().unwrap_err().contains("at most 4"));
    p.bends.clear();
    for i in 0..8 {
        p.bends.push(SheetBend {
            start: -0.9 + i as f32 * 0.2,
            direction: 0.0,
            radius: 0.03,
            angle: 0.5,
        });
    }
    assert!(
        p.validate().is_ok(),
        "all eight legacy zones stay supported"
    );
}

#[test]
fn strong_directional_bends_and_singular_recipes_have_explicit_behavior() {
    let mut p = directional_panel();
    p.bends[0].start = -0.9;
    p.bends[0].angle = 2.8;
    p.bends[1].angle = -2.7;
    p.bends[1].direction = 0.7;
    p.bends[1].radius = 0.8;
    p.bends.push(SheetBend {
        start: 0.1,
        direction: -0.8,
        radius: 3.0,
        angle: 0.7,
    });
    p.bends.push(SheetBend {
        start: -0.2,
        direction: 1.4,
        radius: 5.0,
        angle: -0.8,
    });
    assert!(p.validate().is_ok());
    for i in 0..20 {
        let flat = Vec3::new(-0.9 + i as f32 * 0.09, 0.2, 0.0);
        assert!(
            p.distance(p.form_point(flat)) <= 1e-5,
            "four bends lost {flat:?}"
        );
    }
    let mut single = SheetPanelParams::default();
    single.bends.push(SheetBend {
        start: 0.0,
        direction: std::f32::consts::FRAC_PI_2,
        radius: 0.2,
        angle: 1.0,
    });
    assert!(single.validate().is_ok());
    assert!(
        single.distance(Vec3::new(0.0, 0.0, 0.2)) > 0.1,
        "cylinder axis must not create phantom sheet hits"
    );
    assert!(single.distance(Vec3::new(0.0, 0.0, 0.19)) > 0.1);
    p.bends[1].radius = 0.08;
    p.bends[1].angle = 2.7;
    assert!(p.validate().unwrap_err().contains("radial clearance"));
}
