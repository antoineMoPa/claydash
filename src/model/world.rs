use serde::{Deserialize, Serialize};

use super::{
    BooleanOperation, ClaydashValue, DataTree, GroupRenderRepresentation, MaterialKind, SdfObject,
};

pub fn deferred_fallback_reason(objects: &[SdfObject]) -> Option<&'static str> {
    if objects
        .iter()
        .any(|object| object.material.kind == MaterialKind::Custom)
    {
        Some("custom shader material")
    } else if objects.iter().any(|object| {
        object
            .image_stencil
            .as_ref()
            .is_some_and(|stencil| stencil.image_plane)
    }) {
        Some("image plane with alpha")
    } else if objects.iter().any(|object| {
        object.render_representation == GroupRenderRepresentation::GaussianSplats
            && (object.boolean_parent.is_some()
                || object.operation != BooleanOperation::Union
                || object.repetition.enabled
                || object.mirror.is_some()
                // Independent captures contain cage deformation already;
                // their rendered proxy clears the cage before composition.
                || objects.iter().any(|member| {
                    member.material.kind != MaterialKind::Solid
                        || member.image_stencil.is_some()
                        || member.surface_inlay.is_some()
                }))
    }) {
        Some("Gaussian splat group requires exact composition")
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    #[default]
    Studio,
    Sky,
    Flat,
    Transparent,
}

impl BackgroundMode {
    pub const ALL: [Self; 4] = [Self::Studio, Self::Sky, Self::Flat, Self::Transparent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Studio => "Studio",
            Self::Sky => "Sky & Sun",
            Self::Flat => "Flat color",
            Self::Transparent => "Transparent",
        }
    }

    pub fn shader_id(self) -> u32 {
        match self {
            Self::Studio => 0,
            Self::Sky => 1,
            Self::Flat => 2,
            Self::Transparent => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderPipelineMode {
    #[default]
    Exact,
    Deferred,
}

impl RenderPipelineMode {
    pub const ALL: [Self; 2] = [Self::Exact, Self::Deferred];

    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "Exact ray shading",
            Self::Deferred => "Deferred (experimental)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct World {
    pub render_pipeline: RenderPipelineMode,
    pub screen_space_ambient_occlusion: bool,
    pub screen_space_reflections: bool,
    pub background: BackgroundMode,
    pub flat_color: [f32; 3],
    pub latitude: f32,
    pub day_of_year: u32,
    pub solar_time: f32,
    pub azimuth: f32,
    pub turbidity: f32,
    pub sun_temperature: f32,
    pub sun_intensity: f32,
}

impl Default for World {
    fn default() -> Self {
        Self {
            render_pipeline: RenderPipelineMode::Exact,
            screen_space_ambient_occlusion: true,
            screen_space_reflections: false,
            background: BackgroundMode::Studio,
            flat_color: [0.32, 0.43, 0.6],
            latitude: 45.0,
            day_of_year: 172,
            solar_time: 12.0,
            azimuth: 0.0,
            turbidity: 2.5,
            sun_temperature: 5778.0,
            sun_intensity: 1.0,
        }
    }
}

impl World {
    pub fn sun_direction(self) -> [f32; 3] {
        let latitude = self.latitude.to_radians();
        let declination = (23.44_f32.to_radians().sin()
            * ((self.day_of_year as f32 - 80.0) * std::f32::consts::TAU / 365.0).sin())
        .asin();
        let hour = (self.solar_time - 12.0) * std::f32::consts::TAU / 24.0;
        let east = -declination.cos() * hour.sin();
        let up =
            latitude.sin() * declination.sin() + latitude.cos() * declination.cos() * hour.cos();
        let north =
            latitude.cos() * declination.sin() - latitude.sin() * declination.cos() * hour.cos();
        let rotation = self.azimuth.to_radians();
        [
            east * rotation.cos() - north * rotation.sin(),
            up,
            east * rotation.sin() + north * rotation.cos(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_settings_round_trip_and_legacy_world_defaults_to_exact() {
        let legacy: World = serde_json::from_str(r#"{"background":"Flat"}"#).unwrap();
        assert_eq!(legacy.render_pipeline, RenderPipelineMode::Exact);
        let settings = World {
            render_pipeline: RenderPipelineMode::Deferred,
            screen_space_ambient_occlusion: false,
            screen_space_reflections: true,
            ..World::default()
        };
        let restored: World =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn deferred_accepts_glass_but_identifies_image_alpha() {
        let mut object = SdfObject::create_kind(super::super::PrimitiveKind::Box);
        assert_eq!(deferred_fallback_reason(&[object.clone()]), None);
        object.material.opacity = 0.5;
        assert_eq!(deferred_fallback_reason(&[object.clone()]), None);
        object.material.kind = MaterialKind::Transparent;
        assert_eq!(deferred_fallback_reason(&[object.clone()]), None);
        object.material.opacity = 1.0;
        object.image_stencil = Some(super::super::ImageStencil::new(
            "image".into(),
            Vec::new(),
            true,
        ));
        object.image_stencil.as_mut().unwrap().image_plane = true;
        assert_eq!(
            deferred_fallback_reason(&[object]),
            Some("image plane with alpha")
        );
    }

    #[test]
    fn deferred_accepts_independent_gaussian_cage_capture() {
        use super::super::{Lattice, PrimitiveKind};
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        let mut cage = Lattice::new(-glam::Vec3::ONE, glam::Vec3::ONE, 7);
        cage.offsets.fill(glam::Vec3::new(0.2, -0.1, 0.0));
        root.lattice = Some(cage);
        let mut child = SdfObject::create_kind(PrimitiveKind::Box);
        child.boolean_parent = Some(root.uuid);
        child.lattice = Some(Lattice::new(-glam::Vec3::ONE, glam::Vec3::ONE, 3));
        assert_eq!(deferred_fallback_reason(&[root, child]), None);
    }

    #[test]
    fn deferred_preserves_gaussian_composition_fallbacks() {
        use super::super::{Mirror, PrimitiveKind};
        let mut root = SdfObject::create_kind(PrimitiveKind::Sphere);
        root.render_representation = GroupRenderRepresentation::GaussianSplats;
        let expected = Some("Gaussian splat group requires exact composition");
        let mut nested = root.clone();
        nested.boolean_parent = Some(uuid::Uuid::new_v4());
        assert_eq!(deferred_fallback_reason(&[nested]), expected);
        let mut subtract = root.clone();
        subtract.operation = BooleanOperation::Subtract;
        assert_eq!(deferred_fallback_reason(&[subtract]), expected);
        let mut repeated = root.clone();
        repeated.repetition.enabled = true;
        assert_eq!(deferred_fallback_reason(&[repeated]), expected);
        let mut mirrored = root.clone();
        mirrored.mirror = Some(Mirror {
            axes: [true, false, false],
        });
        assert_eq!(deferred_fallback_reason(&[mirrored]), expected);
        root.material.kind = MaterialKind::Metallic;
        assert_eq!(deferred_fallback_reason(&[root]), expected);
    }

    #[test]
    fn solar_position_tracks_season_and_time() {
        let mut world = World {
            latitude: 45.0,
            day_of_year: 172,
            ..World::default()
        };
        let summer_noon = world.sun_direction()[1];
        world.day_of_year = 355;
        let winter_noon = world.sun_direction()[1];
        world.day_of_year = 172;
        world.solar_time = 0.0;
        let midnight = world.sun_direction()[1];
        assert!(summer_noon > winter_noon);
        assert!(summer_noon > midnight);
        assert!(midnight < 0.0);
    }

    #[test]
    fn world_round_trips_through_scene_value() {
        let mut tree = DataTree::default();
        let settings = World {
            background: BackgroundMode::Transparent,
            ..World::default()
        };
        tree.set_path("scene.world", ClaydashValue::World(settings));
        let bytes = crate::document::serialize_scene(&tree).unwrap();
        let restored = crate::document::deserialize_scene(&bytes).unwrap();
        match restored.get_path("world") {
            ClaydashValue::World(value) => assert_eq!(value, settings),
            _ => panic!("missing world settings"),
        }
    }
}

pub fn world(tree: &DataTree) -> World {
    match tree.get_path("scene.world") {
        ClaydashValue::World(world) => world,
        _ => World::default(),
    }
}
