//! Analytic sheets: legacy material strips and ordered directional spatial forming.
use glam::{Vec2, Vec3};
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

impl SheetBend {
    fn map_point(&self, point: Vec3) -> Vec3 {
        let (s, c) = self.direction.sin_cos();
        let x = c * point.x + s * point.y;
        let y = -s * point.x + c * point.y;
        let u = x - self.start;
        let length = self.radius * self.angle.abs();
        let r = self.radius * self.angle.signum();
        let (x, z) = if u <= 0.0 {
            (x, point.z)
        } else {
            let t = if u < length { u / r } else { self.angle };
            let (st, ct) = t.sin_cos();
            let tail = (u - length).max(0.0);
            (
                self.start + (r - point.z) * st + tail * ct,
                r - (r - point.z) * ct + tail * st,
            )
        };
        Vec3::new(c * x - s * y, s * x + c * y, z)
    }
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

impl SheetStrip {
    pub fn point(&self, x: f32) -> Vec2 {
        let u = x - self.start;
        let local = if self.radius == 0.0 {
            Vec2::new(u, 0.0)
        } else {
            let t = u / self.radius;
            Vec2::new(self.radius * t.sin(), self.radius * (1.0 - t.cos()))
        };
        let (s, c) = self.heading.sin_cos();
        self.origin + Vec2::new(c * local.x - s * local.y, s * local.x + c * local.y)
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

    pub fn emboss_height(&self, uv: Vec2) -> f32 {
        self.embosses
            .iter()
            .map(|emboss| emboss.height_at(uv))
            .sum::<f32>()
            + self
                .surface
                .as_ref()
                .map_or(0.0, |surface| surface.height_at(uv, self.size))
            + self
                .path_embosses
                .iter()
                .map(|path| path.height_at(uv))
                .sum::<f32>()
    }

    pub fn has_relief(&self) -> bool {
        self.surface.is_some() || !self.embosses.is_empty() || !self.path_embosses.is_empty()
    }

    /// Conservative signed Z interval, independent of inspection stage and cuts.
    pub fn embossed_depth_range(&self) -> Vec2 {
        let mut range = Vec2::new(-self.thickness * 0.5, self.thickness * 0.5);
        for emboss in &self.embosses {
            range.x += emboss.depth().min(0.0);
            range.y += emboss.depth().max(0.0);
        }
        if let Some(surface) = &self.surface {
            range.x += surface
                .heights
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            range.y += surface
                .heights
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
        }
        for path in &self.path_embosses {
            range += path.depth_range();
        }
        range
    }

    /// Reciprocal largest singular value of the material-Z shear's Jacobian.
    pub fn emboss_metric_scale(&self) -> f32 {
        let slope: f32 = self
            .embosses
            .iter()
            .map(SheetEmboss::slope_bound)
            .sum::<f32>()
            + self
                .surface
                .as_ref()
                .map_or(0.0, |s| s.slope_bound(self.size))
            + self
                .path_embosses
                .iter()
                .map(SheetPathEmboss::slope_bound)
                .sum::<f32>();
        2.0 / ((slope * slope + 4.0).sqrt() + slope)
    }

    fn strip_metric_scale(&self, radius: f32) -> f32 {
        let range = self.embossed_depth_range();
        if radius > 0.0 {
            ((radius - range.y) / radius).min(1.0)
        } else {
            ((radius.abs() + range.x) / radius.abs()).min(1.0)
        }
    }

    pub fn strips(&self) -> Vec<SheetStrip> {
        let mut strips = Vec::with_capacity(self.bends.len() * 2 + 1);
        self.visit_strips(|strip| strips.push(strip));
        strips
    }

    fn visit_strips(&self, mut visit: impl FnMut(SheetStrip)) {
        let mut start = -self.size.x * 0.5;
        let mut origin = Vec2::new(start, 0.0);
        let mut heading = 0.0;
        if self.stage == SheetStage::Formed {
            for bend in &self.bends {
                if bend.start > start {
                    let strip = SheetStrip {
                        start,
                        end: bend.start,
                        origin,
                        heading,
                        radius: 0.0,
                        angle: 0.0,
                    };
                    origin = strip.point(strip.end);
                    visit(strip);
                }
                let end = bend.start + bend.radius * bend.angle.abs();
                let strip = SheetStrip {
                    start: bend.start,
                    end,
                    origin,
                    heading,
                    radius: bend.radius * bend.angle.signum(),
                    angle: bend.angle,
                };
                origin = strip.point(end);
                visit(strip);
                heading += bend.angle;
                start = end;
            }
        }
        if start < self.size.x * 0.5 {
            visit(SheetStrip {
                start,
                end: self.size.x * 0.5,
                origin,
                heading,
                radius: 0.0,
                angle: 0.0,
            });
        }
    }

    pub fn material_point(&self, uv: Vec2) -> Vec3 {
        self.form_point(uv.extend(0.0))
    }

    pub fn local_extent(&self) -> Vec3 {
        if self.uses_directional_forming() && self.stage == SheetStage::Formed {
            let mut extent = Vec3::ZERO;
            self.visit_directional_bounds(|low, high| extent = low.abs().max(high.abs()));
            return extent;
        }
        let mut extent = Vec2::ZERO;
        self.visit_strips(|strip| {
            extent = extent
                .max(strip.origin.abs())
                .max(strip.point(strip.end).abs());
            if strip.radius != 0.0 {
                // Tangent extrema bound circular arcs exactly, without sampling.
                let finish = strip.heading + strip.angle;
                for k in -16..=16 {
                    let heading = k as f32 * std::f32::consts::FRAC_PI_2;
                    if heading >= strip.heading.min(finish) && heading <= strip.heading.max(finish)
                    {
                        let x = strip.start + strip.radius * (heading - strip.heading);
                        extent = extent.max(strip.point(x).abs());
                    }
                }
            }
        });
        let relief = if self.stage == SheetStage::Formed {
            self.embossed_depth_range().abs().max_element() - self.thickness * 0.5
        } else {
            0.0
        };
        Vec3::new(
            extent.x + self.thickness * 0.5 + relief,
            self.size.y * 0.5,
            extent.y + self.thickness * 0.5 + relief,
        )
    }

    fn visit_directional_bounds(&self, mut visit: impl FnMut(Vec3, Vec3)) {
        let range = self.embossed_depth_range();
        let mut low = Vec3::new(-self.size.x * 0.5, -self.size.y * 0.5, range.x);
        let mut high = Vec3::new(self.size.x * 0.5, self.size.y * 0.5, range.y);
        visit(low, high);
        for bend in &self.bends {
            let (s, c) = bend.direction.sin_cos();
            let mut axis_low = Vec2::splat(f32::INFINITY);
            let mut axis_high = Vec2::splat(f32::NEG_INFINITY);
            for x in [low.x, high.x] {
                for y in [low.y, high.y] {
                    let axis = Vec2::new(c * x + s * y, -s * x + c * y);
                    axis_low = axis_low.min(axis);
                    axis_high = axis_high.max(axis);
                }
            }
            let length = bend.radius * bend.angle.abs();
            let end = bend.start + length;
            let mut coordinates = vec![axis_low.x, axis_high.x];
            // Piecewise endpoints and tangent extrema enclose the complete
            // transformed input box, including both thickness faces.
            for x in [bend.start, end] {
                if x >= axis_low.x && x <= axis_high.x {
                    coordinates.push(x);
                }
            }
            for k in -2..=2 {
                let t = k as f32 * std::f32::consts::FRAC_PI_2;
                if t >= bend.angle.min(0.0) && t <= bend.angle.max(0.0) {
                    let x = bend.start + bend.radius * bend.angle.signum() * t;
                    if x >= axis_low.x && x <= axis_high.x {
                        coordinates.push(x);
                    }
                }
            }
            let mut next_low = Vec3::splat(f32::INFINITY);
            let mut next_high = Vec3::splat(f32::NEG_INFINITY);
            for x in coordinates {
                for y in [axis_low.y, axis_high.y] {
                    for z in [low.z, high.z] {
                        let point = bend.map_point(Vec3::new(c * x - s * y, s * x + c * y, z));
                        next_low = next_low.min(point);
                        next_high = next_high.max(point);
                    }
                }
            }
            low = next_low;
            high = next_high;
            visit(low, high);
        }
    }

    /// Lower bound for each inverse chart's metric over the preceding slab.
    /// The cylinder centre must stay outside that slab's Z interval.
    pub fn directional_scales(&self) -> Vec<f32> {
        let mut bounds = Vec::new();
        self.visit_directional_bounds(|low, high| bounds.push((low, high)));
        self.bends
            .iter()
            .zip(bounds)
            .map(|(bend, (low, high))| {
                let gap = if bend.angle > 0.0 {
                    bend.radius - high.z
                } else {
                    bend.radius + low.z
                };
                (gap / bend.radius).min(1.0)
            })
            .collect()
    }

    /// Apply each spatial forming operation in recipe order. Directions and starts
    /// remain in sheet-local coordinates; subsequent operations act on the result.
    pub fn form_point(&self, mut point: Vec3) -> Vec3 {
        if self.stage != SheetStage::Formed {
            return point;
        }
        point.z += self.emboss_height(point.truncate());
        if !self.uses_directional_forming() {
            let strips = self.strips();
            if let Some(strip) = strips
                .iter()
                .find(|strip| point.x <= strip.end)
                .or_else(|| strips.last())
            {
                let neutral = strip.point(point.x);
                let heading = strip.heading
                    + if strip.radius == 0.0 {
                        0.0
                    } else {
                        (point.x - strip.start) / strip.radius
                    };
                return Vec3::new(
                    neutral.x - heading.sin() * point.z,
                    point.y,
                    neutral.y + heading.cos() * point.z,
                );
            }
            return point;
        }
        for bend in &self.bends {
            point = bend.map_point(point);
        }
        point
    }

    fn directional_distance(&self, point: Vec3) -> f32 {
        // DFS needs 1 + 2 * depth slots, rather than storing all 3^depth leaves.
        let scales = self.directional_scales();
        let mut stack = vec![(point, self.bends.len(), f32::NEG_INFINITY, 1.0)];
        let mut result = f32::INFINITY;
        while let Some((point, remaining, constraint, scale)) = stack.pop() {
            if remaining == 0 {
                let planar = self.outline_distance(point.truncate()) * scale;
                let depth = ((point.z - self.emboss_height(point.truncate())).abs()
                    - self.thickness * 0.5)
                    * scale;
                let q = Vec2::new(planar, depth);
                result = result
                    .min(constraint.max(q.max(Vec2::ZERO).length() + q.max_element().min(0.0)));
                continue;
            }
            let bend = &self.bends[remaining - 1];
            let (s, c) = bend.direction.sin_cos();
            let x = c * point.x + s * point.y - bend.start;
            let y = -s * point.x + c * point.y;
            let r = bend.radius * bend.angle.signum();
            let length = bend.radius * bend.angle.abs();
            let mut push = |u: f32, z: f32, domain: f32, factor: f32| {
                let x = bend.start + u;
                stack.push((
                    Vec3::new(c * x - s * y, s * x + c * y, z),
                    remaining - 1,
                    constraint.max(domain * scale * factor),
                    scale * factor,
                ));
            };
            // Stationary and outgoing rigid half-spaces.
            push(x, point.z, x, scales[remaining - 1]);
            let (st, ct) = bend.angle.sin_cos();
            let dx = x - r * st;
            let dz = point.z - r * (1.0 - ct);
            let u = length + ct * dx + st * dz;
            push(u, -st * dx + ct * dz, length - u, scales[remaining - 1]);
            // Validated clearance keeps the preceding slab on the radius's
            // original side, so the opposite radial inverse cannot reach material.
            let rho = Vec2::new(x, r - point.z).length();
            let t = (r.signum() * x).atan2(r.signum() * (r - point.z));
            let u = r * t;
            push(
                u,
                r - r.signum() * rho,
                (-u).max(u - length),
                scales[remaining - 1],
            );
        }
        result * self.emboss_metric_scale()
    }

    pub fn outline_distance(&self, uv: Vec2) -> f32 {
        let q = uv.abs() - self.size * 0.5;
        let mut d = q.max(Vec2::ZERO).length() + q.max_element().min(0.0);
        if self.stage != SheetStage::Blank {
            for cut in &self.cuts {
                let c = match cut {
                    SheetCut::Straight { from, to } => {
                        let e = *to - *from;
                        let p = uv - *from;
                        (e.y * p.x - e.x * p.y) / e.length()
                    }
                    SheetCut::Circle { center, radius } => *radius - uv.distance(*center),
                };
                d = d.max(c);
            }
        }
        d
    }

    pub fn distance(&self, point: Vec3) -> f32 {
        if self.uses_directional_forming() && self.stage == SheetStage::Formed {
            return self.directional_distance(point);
        }
        let half = self.thickness * 0.5;
        let mut result = f32::INFINITY;
        self.visit_strips(|strip| {
            let delta = Vec2::new(point.x, point.z) - strip.origin;
            let (s, c) = strip.heading.sin_cos();
            let p = Vec2::new(c * delta.x + s * delta.y, -s * delta.x + c * delta.y);
            let (u, depth, factor) = if strip.radius == 0.0 {
                (p.x, p.y, 1.0)
            } else {
                let r = strip.radius;
                let t = (r.signum() * p.x).atan2(r.signum() * (r - p.y));
                (
                    r * t,
                    r.signum() * (r.abs() - Vec2::new(p.x, p.y - r).length()),
                    if self.stage == SheetStage::Formed && self.has_relief() {
                        self.strip_metric_scale(r).max(0.01)
                    } else {
                        (1.0 - half / r.abs()).max(0.01)
                    },
                )
            };
            let x = strip.start + u;
            let depth = depth
                - if self.stage == SheetStage::Formed {
                    self.emboss_height(Vec2::new(x, point.y))
                } else {
                    0.0
                };
            let planar = self
                .outline_distance(Vec2::new(x, point.y))
                .max(strip.start - x)
                .max(x - strip.end)
                * factor;
            let q = Vec2::new(planar, depth.abs() - half);
            result = result.min(q.max(Vec2::ZERO).length() + q.max_element().min(0.0));
        });
        result
            * if self.stage == SheetStage::Formed {
                self.emboss_metric_scale()
            } else {
                1.0
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn crowned_path_panel() -> SheetPanelParams {
        SheetPanelParams {
            size: Vec2::new(0.9, 0.7),
            thickness: 0.008,
            surface: Some(SheetSurface {
                heights: [
                    0.0, 0.04, 0.04, 0.0, 0.04, 0.12, 0.12, 0.04, 0.04, 0.12, 0.12, 0.04, 0.0,
                    0.04, 0.04, 0.0,
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
                    let uv =
                        Vec2::new(-0.45 + 0.9 * i as f32 / 30.0, -0.35 + 0.7 * j as f32 / 24.0);
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
            let slope = (p.emboss_height(uv + Vec2::Y * 1e-5)
                - p.emboss_height(uv - Vec2::Y * 1e-5))
                / 2e-5;
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
            serde_json::from_value(serde_json::json!({"size":[0.9,0.7],"thickness":0.008}))
                .unwrap();
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
            serde_json::to_value(
                serde_json::from_value::<SheetPanelParams>(encoded.clone()).unwrap()
            )
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
}
