use serde::{Deserialize, Serialize};

/// A saved render choice for an object or Boolean group. Derived bake data
/// does not belong in the scene document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRenderRepresentation {
    BoxDepthAtlas,
    SphereDepthAtlas,
    GaussianSplats,
    #[default]
    #[serde(other)]
    ExactSdf,
}

impl GroupRenderRepresentation {
    pub const ALL: [Self; 4] = [
        Self::ExactSdf,
        Self::BoxDepthAtlas,
        Self::SphereDepthAtlas,
        Self::GaussianSplats,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ExactSdf => "Exact SDF (current)",
            Self::BoxDepthAtlas => "Box depth + texture atlas",
            Self::SphereDepthAtlas => "Sphere depth + texture atlas",
            Self::GaussianSplats => "Gaussian splats (SDF)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ExactSdf => "Editable source geometry with the current exact renderer.",
            Self::BoxDepthAtlas => "Capture depth and appearance from six box faces.",
            Self::SphereDepthAtlas => {
                "Capture depth and appearance with inward rays from a sphere."
            }
            Self::GaussianSplats => "Fit Gaussian surface samples from radial SDF captures.",
        }
    }

    pub fn limitation(self) -> &'static str {
        match self {
            Self::ExactSdf => "Full source detail; repeated group queries can be expensive.",
            Self::BoxDepthAtlas => {
                "A single depth layer misses hidden surfaces and close parallax."
            }
            Self::SphereDepthAtlas => {
                "A single radial layer misses hidden surfaces and close parallax."
            }
            Self::GaussianSplats => {
                "Approximate surface; thin details and hidden layers may disappear."
            }
        }
    }

    pub fn is_exact(&self) -> bool {
        *self == Self::ExactSdf
    }
}
