//! Forward forming, inverse distance fields and conservative bounds.
use super::{
    SheetBend, SheetCut, SheetEmboss, SheetPanelParams, SheetPathEmboss, SheetStage, SheetStrip,
};
use glam::{Vec2, Vec3};

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

    pub(super) fn strip_metric_scale(&self, radius: f32) -> f32 {
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
        if !self.uses_directional_forming() {
            return self.form_point_with_strips(point, &self.strips());
        }
        point.z += self.emboss_height(point.truncate());
        for bend in &self.bends {
            point = bend.map_point(point);
        }
        point
    }

    /// Map a point with parallel strips cached from this panel's recipe/stage.
    /// Batch tessellation callers avoid allocating the strip list per vertex.
    pub(crate) fn form_point_with_strips(&self, mut point: Vec3, strips: &[SheetStrip]) -> Vec3 {
        if self.stage != SheetStage::Formed {
            return point;
        }
        point.z += self.emboss_height(point.truncate());
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

impl SheetPanelParams {
    /// Conservative formed bounds of the retained blank. Circular holes cannot
    /// enlarge it; straight half-plane cuts can substantially shrink it.
    pub(crate) fn clipped_local_bounds(&self) -> Option<(Vec3, Vec3)> {
        self.clipped_local_bounds_with_margin(0.0)
    }

    /// Enclose the retained inverse charts, including a material-space tolerance.
    pub(crate) fn clipped_local_bounds_with_margin(&self, margin: f32) -> Option<(Vec3, Vec3)> {
        if !margin.is_finite() || margin < 0.0 || self.validate().is_err() || (self.stage == SheetStage::Formed && self.uses_directional_forming()) { return None; }
        let h = self.size.as_dvec2() * 0.5 + glam::DVec2::splat(margin as f64);
        let mut polygon = vec![glam::DVec2::new(-h.x,-h.y),glam::DVec2::new(h.x,-h.y),glam::DVec2::new(h.x,h.y),glam::DVec2::new(-h.x,h.y)];
        if self.stage != SheetStage::Blank {
            for cut in &self.cuts {
                if let SheetCut::Straight { from, to } = cut {
                    let from = from.as_dvec2();
                    let edge = to.as_dvec2() - from;
                    let previous = std::mem::take(&mut polygon);
                    for i in 0..previous.len() {
                        let a=previous[i]; let b=previous[(i+1)%previous.len()];
                        let padding=edge.length()*margin as f64;
                        let da=edge.perp_dot(a-from)+padding; let db=edge.perp_dot(b-from)+padding;
                        if da >= 0.0 { polygon.push(a); }
                        if (da<0.0 && db>0.0)||(da>0.0 && db<0.0) { polygon.push(a+(b-a)*(da/(da-db))); }
                    }
                }
            }
        }
        if polygon.len()<3 { return None; }
        let low=polygon.iter().fold(glam::DVec2::splat(f64::INFINITY),|a,p| a.min(*p)).as_vec2();
        let high=polygon.iter().fold(glam::DVec2::splat(f64::NEG_INFINITY),|a,p| a.max(*p)).as_vec2();
        let depth=if self.stage==SheetStage::Formed { self.embossed_depth_range() } else { Vec2::new(-self.thickness*0.5,self.thickness*0.5) };
        let depth=depth+Vec2::new(-margin,margin);
        let mut minimum=Vec3::splat(f32::INFINITY); let mut maximum=Vec3::splat(f32::NEG_INFINITY);
        for strip in self.strips() {
            let start=(strip.start-margin).max(low.x); let end=(strip.end+margin).min(high.x);
            if start>end { continue; }
            let mut coordinates=vec![start,end];
            if strip.radius!=0.0 {
                let a=strip.heading+(start-strip.start)/strip.radius;
                let b=strip.heading+(end-strip.start)/strip.radius;
                let quarter=std::f32::consts::FRAC_PI_2;
                for k in (a.min(b)/quarter).ceil() as i32..=(a.max(b)/quarter).floor() as i32 {
                    coordinates.push(strip.start+strip.radius*(k as f32*quarter-strip.heading));
                }
            }
            for x in coordinates {
                let angle=strip.heading+if strip.radius==0.0 { 0.0 } else { (x-strip.start)/strip.radius };
                let (s,c)=angle.sin_cos();
                for d in [depth.x,depth.y] {
                    let p=strip.point(x)+Vec2::new(-s,c)*d;
                    minimum=minimum.min(Vec3::new(p.x,low.y,p.y));
                    maximum=maximum.max(Vec3::new(p.x,high.y,p.y));
                }
            }
        }
        (minimum.is_finite() && maximum.is_finite()).then_some((minimum,maximum))
    }
}
