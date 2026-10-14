//! Sheet recipes, material-space relief profiles and validation.
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Tensor-product cubic Bezier height patch. Row-major: x varies fastest.
/// XY remains the original material rectangle; heights are control values,
/// not interpolation samples. This graph cannot represent overhangs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SheetSurface {
    pub heights: [f32; 16],
}

impl SheetSurface {
    pub fn height_at(&self, uv: Vec2, size: Vec2) -> f32 {
        let t = (uv / size + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE);
        let basis = |t: f32| {
            [
                (1.0 - t).powi(3),
                3.0 * t * (1.0 - t).powi(2),
                3.0 * t * t * (1.0 - t),
                t * t * t,
            ]
        };
        let x = basis(t.x);
        let y = basis(t.y);
        let mut height = 0.0;
        for j in 0..4 {
            for i in 0..4 {
                height += self.heights[j * 4 + i] * x[i] * y[j];
            }
        }
        height
    }

    fn slope_bound(&self, size: Vec2) -> f32 {
        let mut slope = Vec2::ZERO;
        for j in 0..4 {
            for i in 0..3 {
                slope.x = slope.x.max(
                    3.0 * (self.heights[j * 4 + i + 1] - self.heights[j * 4 + i]).abs() / size.x,
                );
                slope.y = slope.y.max(
                    3.0 * (self.heights[(i + 1) * 4 + j] - self.heights[i * 4 + j]).abs() / size.y,
                );
            }
        }
        slope.length()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum SheetPathProfile {
    #[default]
    Rounded,
    /// A sharp centre crease with a smooth zero-slope outer shoulder.
    Crease,
}
impl SheetPathProfile {
    pub fn blend_value(self) -> f32 {
        match self {
            Self::Rounded => 0.0,
            Self::Crease => 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SheetPathStation {
    pub point: Vec2,
    /// Half-width of the relief, measured in material XY.
    pub width: f32,
    pub depth: f32,
    #[serde(default)]
    pub profile: SheetPathProfile,
}

/// Continuous envelope of variable-profile segment sweeps, with arclength end taper.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SheetPathEmboss {
    pub stations: Vec<SheetPathStation>,
    pub taper_start: f32,
    pub taper_end: f32,
}
impl SheetPathEmboss {
    pub const MAX_STATIONS: usize = 8;
    pub fn length(&self) -> f32 {
        self.stations
            .windows(2)
            .map(|s| s[0].point.distance(s[1].point))
            .sum()
    }
    pub fn height_at(&self, uv: Vec2) -> f32 {
        let total = self.length();
        let mut along = 0.0;
        let mut height: f32 = 0.0;
        for s in self.stations.windows(2) {
            let a = &s[0];
            let b = &s[1];
            let e = b.point - a.point;
            let length = e.length();
            let t = ((uv - a.point).dot(e) / e.length_squared()).clamp(0.0, 1.0);
            let width = a.width + (b.width - a.width) * t;
            let depth = a.depth + (b.depth - a.depth) * t;
            let r = (uv.distance(a.point + e * t) / width).min(1.0);
            let rounded = 0.5 * (1.0 + (std::f32::consts::PI * r).cos());
            let crease = (1.0 - r).powi(2);
            let blend =
                a.profile.blend_value() + (b.profile.blend_value() - a.profile.blend_value()) * t;
            let smooth = |v: f32| {
                let v = v.clamp(0.0, 1.0);
                v * v * (3.0 - 2.0 * v)
            };
            let arclength = along + t * length;
            let taper =
                smooth(arclength / self.taper_start) * smooth((total - arclength) / self.taper_end);
            let value = depth * (rounded + (crease - rounded) * blend) * taper;
            if value.abs() > height.abs() {
                height = value;
            }
            along += length;
        }
        height
    }
    fn depth_range(&self) -> Vec2 {
        self.stations.iter().fold(Vec2::ZERO, |r, s| {
            Vec2::new(r.x.min(s.depth), r.y.max(s.depth))
        })
    }
    fn slope_bound(&self) -> f32 {
        self.stations
            .windows(2)
            .map(|s| {
                let length = s[0].point.distance(s[1].point);
                let width = s[0].width.min(s[1].width);
                let depth = s[0].depth.abs().max(s[1].depth.abs());
                // Radial profile slope <=2; r<=1 inside support. Outside it height is zero.
                2.0 * depth / width * (1.0 + (s[1].width - s[0].width).abs() / length)
                    + (s[1].depth - s[0].depth).abs() / length
                    + depth / length
                    + 1.5 * depth * (1.0 / self.taper_start + 1.0 / self.taper_end)
            })
            .fold(0.0, f32::max)
    }
    fn validate(&self) -> Result<(), String> {
        if !(2..=Self::MAX_STATIONS).contains(&self.stations.len())
            || !self.taper_start.is_finite()
            || self.taper_start <= 0.0
            || !self.taper_end.is_finite()
            || self.taper_end <= 0.0
            || !self.length().is_finite()
            || self.taper_start + self.taper_end > self.length()
            || self.stations.iter().any(|s| {
                !s.point.is_finite()
                    || !s.width.is_finite()
                    || s.width <= 0.0
                    || !s.depth.is_finite()
                    || s.depth == 0.0
            })
            || self.stations.windows(2).any(|s| {
                s[0].point.distance(s[1].point) <= 1e-5
                    || s[0].depth.signum() != s[1].depth.signum()
            })
        {
            return Err("path emboss requires 2–8 distinct adjacent stations, positive finite widths/tapers, same-sign nonzero depths and taper sum <= path length".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetEdge {
    MinX,
    MaxX,
    MinY,
    MaxY,
}

/// Directed dependency: this panel follows the source panel's boundary and tangent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SheetSeam {
    pub source: uuid::Uuid,
    pub source_edge: SheetEdge,
    pub edge: SheetEdge,
    /// Separation along the source's outward material axis, in source-local units.
    pub gap: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetStage {
    Blank,
    Cut,
    #[default]
    Formed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SheetCut {
    /// Retain the left side of the directed line from `from` to `to`.
    Straight {
        from: Vec2,
        to: Vec2,
    },
    Circle {
        center: Vec2,
        radius: f32,
    },
}

/// Material-space relief, applied before forming the sheet's bends.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SheetEmboss {
    Circle {
        center: Vec2,
        /// Radius of the constant-depth inner disk.
        radius: f32,
        /// Signed material-Z displacement; negative values make a recess.
        depth: f32,
        /// Radial width of the rounded transition back to the original sheet.
        transition: f32,
    },
}

impl SheetEmboss {
    pub fn height_at(&self, uv: Vec2) -> f32 {
        match self {
            Self::Circle {
                center,
                radius,
                depth,
                transition,
            } => {
                let distance = uv.distance(*center);
                if distance <= *radius {
                    *depth
                } else if distance >= radius + transition {
                    0.0
                } else {
                    let t = (distance - radius) / transition;
                    depth * 0.5 * (1.0 + (std::f32::consts::PI * t).cos())
                }
            }
        }
    }

    fn depth(&self) -> f32 {
        match self {
            Self::Circle { depth, .. } => *depth,
        }
    }

    fn slope_bound(&self) -> f32 {
        match self {
            Self::Circle {
                depth, transition, ..
            } => depth.abs() / transition * std::f32::consts::FRAC_PI_2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SheetBend {
    /// Start along the bend direction; the fold line is perpendicular to it.
    pub start: f32,
    /// Direction in sheet-local XY, in radians from +X. Missing values preserve legacy strips.
    #[serde(default)]
    pub direction: f32,
    /// Neutral-surface radius. Zone length is radius * abs(angle).
    pub radius: f32,
    /// Signed radians, with positive bends towards local +Z.
    pub angle: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SheetPanelParams {
    pub size: Vec2,
    pub thickness: f32,
    #[serde(default)]
    pub cuts: Vec<SheetCut>,
    #[serde(default)]
    pub bends: Vec<SheetBend>,
    #[serde(default)]
    pub embosses: Vec<SheetEmboss>,
    #[serde(default)]
    pub surface: Option<SheetSurface>,
    #[serde(default)]
    pub path_embosses: Vec<SheetPathEmboss>,
    #[serde(default)]
    pub seam: Option<SheetSeam>,
    #[serde(default)]
    pub stage: SheetStage,
}

/// Analytic strip, shared by CPU evaluation and GPU upload.
pub struct SheetStrip {
    pub start: f32,
    pub end: f32,
    pub origin: Vec2,
    pub heading: f32,
    pub radius: f32,
    pub angle: f32,
}

impl Default for SheetPanelParams {
    fn default() -> Self {
        Self {
            size: Vec2::new(2.0, 1.0),
            thickness: 0.04,
            cuts: vec![],
            bends: vec![],
            embosses: vec![],
            surface: None,
            path_embosses: vec![],
            seam: None,
            stage: SheetStage::Formed,
        }
    }
}

impl SheetPanelParams {
    pub const MAX_CUTS: usize = 32;
    pub const MAX_BENDS: usize = 8;
    pub const MAX_EMBOSSES: usize = 8;
    pub const MAX_PATH_EMBOSSES: usize = 4;
    /// Directional forming evaluates three inverse charts per operation.
    pub const MAX_DIRECTIONAL_BENDS: usize = 4;

    pub fn uses_directional_forming(&self) -> bool {
        self.bends.iter().any(|bend| bend.direction != 0.0)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite()
            || self.size.min_element() <= 0.0
            || !self.thickness.is_finite()
            || self.thickness <= 0.0
            || self.cuts.len() > Self::MAX_CUTS
            || self.bends.len() > Self::MAX_BENDS
            || self.embosses.len() > Self::MAX_EMBOSSES
            || self.path_embosses.len() > Self::MAX_PATH_EMBOSSES
        {
            return Err("invalid sheet dimensions or operation count".into());
        }
        if self
            .surface
            .as_ref()
            .is_some_and(|s| s.heights.iter().any(|h| !h.is_finite()))
        {
            return Err("surface control heights must be finite".into());
        }
        if self
            .seam
            .as_ref()
            .is_some_and(|s| !s.gap.is_finite() || s.gap < 0.0)
        {
            return Err("seam gap must be finite and nonnegative".into());
        }
        for path in &self.path_embosses {
            path.validate()?;
        }
        for cut in &self.cuts {
            let valid = match cut {
                SheetCut::Straight { from, to } => {
                    from.is_finite() && to.is_finite() && from.distance(*to) > 1e-5
                }
                SheetCut::Circle { center, radius } => {
                    center.is_finite() && radius.is_finite() && *radius > 0.0
                }
            };
            if !valid {
                return Err("invalid sheet cut".into());
            }
        }
        for emboss in &self.embosses {
            let valid = match emboss {
                SheetEmboss::Circle {
                    center,
                    radius,
                    depth,
                    transition,
                } => {
                    center.is_finite()
                        && radius.is_finite()
                        && *radius > 0.0
                        && depth.is_finite()
                        && *depth != 0.0
                        && transition.is_finite()
                        && *transition > 0.0
                        && (radius + transition).is_finite()
                }
            };
            if !valid {
                return Err("circular emboss requires finite center, positive radius/transition and finite nonzero depth".into());
            }
        }
        if !self.embossed_depth_range().is_finite()
            || !self.emboss_metric_scale().is_finite()
            || self.emboss_metric_scale() < 0.05
        {
            return Err("combined emboss transitions are too steep; increase transition width or reduce depth".into());
        }
        let directional = self.uses_directional_forming();
        if directional && self.bends.len() > Self::MAX_DIRECTIONAL_BENDS {
            return Err(
                "directional forming supports at most 4 bends (legacy parallel strips support 8)"
                    .into(),
            );
        }
        let mut end = -self.size.x * 0.5;
        for bend in &self.bends {
            if !bend.start.is_finite()
                || !bend.direction.is_finite()
                || !bend.radius.is_finite()
                || !bend.angle.is_finite()
                || bend.radius <= self.thickness * 0.5
                || bend.angle.abs() < 1e-4
                || bend.angle.abs() >= std::f32::consts::PI
                || (!directional
                    && (bend.start < end
                        || bend.start + bend.radius * bend.angle.abs() > self.size.x * 0.5))
            {
                return Err("sheet bends require finite direction/start, radius > thickness/2 and 0 < |angle| < pi; legacy parallel zones must be ordered, nonoverlapping and inside the blank".into());
            }
            end = bend.start + bend.radius * bend.angle.abs();
        }
        if directional
            && self
                .directional_scales()
                .iter()
                .any(|scale| !scale.is_finite() || *scale < 0.05)
        {
            return Err("directional bend cylinder centres must remain outside the preceding formed slab bounds with at least 5% radial clearance; increase radius, reverse bend sign or reorder bends".into());
        }
        if !directional && self.has_relief() {
            let mut formed = self.clone();
            formed.stage = SheetStage::Formed;
            if formed
                .strips()
                .iter()
                .any(|strip| strip.radius != 0.0 && self.strip_metric_scale(strip.radius) < 0.05)
            {
                return Err("embossed sheet requires at least 5% radial clearance inside each bend; increase bend radius or reduce emboss depth".into());
            }
        }
        if !self.local_extent().length().is_finite() {
            return Err("formed sheet bounds exceed finite render coordinates".into());
        }
        Ok(())
    }
}

mod forming;

#[cfg(test)]
mod tests;
