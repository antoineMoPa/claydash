use serde::{Deserialize, Serialize};

use super::{ClaydashValue, DataTree};

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct World {
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
