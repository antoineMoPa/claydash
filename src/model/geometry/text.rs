use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use ttf_parser::{Face, OutlineBuilder};

use super::{BezierCurveParams, SdfObject, SdfParams};

const FONT: &[u8] = include_bytes!("../../../assets/fonts/FiraMono-Medium.ttf");
pub const MAX_TEXT_POINTS: usize = 16_384;
pub const MAX_SCENE_TEXT_POINTS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextParams {
    pub text: String,
    pub size: f32,
    pub half_depth: f32,
    pub tracking: f32,
    pub line_spacing: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<uuid::Uuid>,
}

impl Default for TextParams {
    fn default() -> Self {
        Self {
            text: "Text".into(),
            size: 0.5,
            half_depth: 0.06,
            tracking: 0.0,
            line_spacing: 1.3,
            path: None,
        }
    }
}

#[derive(Clone)]
struct Outline {
    advance: f32,
    edges: Vec<[Vec2; 2]>,
    min: Vec2,
    max: Vec2,
}

#[derive(Clone)]
pub struct TextGlyph {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub min: Vec2,
    pub max: Vec2,
    pub edges: Vec<[Vec2; 2]>,
}

#[derive(Clone, Default)]
pub struct PreparedText {
    pub glyphs: Vec<TextGlyph>,
    pub half_depth: f32,
    pub minimum: Vec3,
    pub maximum: Vec3,
}

impl PreparedText {
    pub fn local_extent(&self) -> Vec3 {
        self.minimum.abs().max(self.maximum.abs())
    }

    pub fn packed_points(&self) -> usize {
        1 + self
            .glyphs
            .iter()
            .map(|glyph| 9 + glyph.edges.len() * 2)
            .sum::<usize>()
    }

    pub fn distance(&self, point: Vec3) -> f32 {
        let mut nearest = 100.0_f32;
        for glyph in &self.glyphs {
            let delta = point - glyph.origin;
            let flat = Vec2::new(delta.dot(glyph.x), delta.dot(glyph.y));
            let depth = delta.dot(glyph.z).abs() - self.half_depth;
            let box_delta = (glyph.min - flat).max(flat - glyph.max).max(Vec2::ZERO);
            if box_delta.length() > nearest.max(0.0) + self.half_depth {
                continue;
            }
            let mut edge_distance = f32::INFINITY;
            let mut inside = false;
            for &[a, b] in &glyph.edges {
                let edge = b - a;
                let t = ((flat - a).dot(edge) / edge.length_squared().max(1e-10)).clamp(0.0, 1.0);
                edge_distance = edge_distance.min(flat.distance(a + edge * t));
                if (a.y > flat.y) != (b.y > flat.y)
                    && flat.x < a.x + (flat.y - a.y) * (b.x - a.x) / (b.y - a.y)
                {
                    inside = !inside;
                }
            }
            if !edge_distance.is_finite() {
                continue;
            }
            let planar = if inside {
                -edge_distance
            } else {
                edge_distance
            };
            let q = Vec2::new(planar, depth);
            nearest = nearest.min(q.max(Vec2::ZERO).length() + q.max_element().min(0.0));
        }
        nearest
    }
}

struct EdgeBuilder {
    edges: Vec<[Vec2; 2]>,
    first: Vec2,
    last: Vec2,
    active: bool,
}
impl EdgeBuilder {
    fn add(&mut self, point: Vec2) {
        if self.last.distance_squared(point) > 1e-10 {
            self.edges.push([self.last, point]);
        }
        self.last = point;
    }
}
impl OutlineBuilder for EdgeBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.active {
            self.close();
        }
        self.first = Vec2::new(x, y);
        self.last = self.first;
        self.active = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(Vec2::new(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.last;
        let b = Vec2::new(x1, y1);
        let c = Vec2::new(x, y);
        let steps = ((a - b * 2.0 + c).length().sqrt() / 3.0)
            .ceil()
            .clamp(4.0, 24.0) as usize;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            self.add(a * u * u + b * 2.0 * u * t + c * t * t);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.last;
        let b = Vec2::new(x1, y1);
        let c = Vec2::new(x2, y2);
        let d = Vec2::new(x, y);
        let steps = ((a - b).length() + (b - c).length() + (c - d).length())
            .sqrt()
            .ceil()
            .clamp(8.0, 32.0) as usize;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            self.add(a * u * u * u + b * 3.0 * u * u * t + c * 3.0 * u * t * t + d * t * t * t);
        }
    }
    fn close(&mut self) {
        if self.active {
            self.add(self.first);
            self.active = false;
        }
    }
}

fn outline(character: char) -> Result<Arc<Outline>, String> {
    static CACHE: OnceLock<Mutex<HashMap<char, Arc<Outline>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().unwrap().get(&character) {
        return Ok(found.clone());
    }
    let face = Face::parse(FONT, 0).map_err(|_| "bundled font could not be read")?;
    let glyph = face
        .glyph_index(character)
        .ok_or_else(|| format!("font has no glyph for {character:?}"))?;
    let units = face.units_per_em() as f32;
    let mut builder = EdgeBuilder {
        edges: Vec::new(),
        first: Vec2::ZERO,
        last: Vec2::ZERO,
        active: false,
    };
    face.outline_glyph(glyph, &mut builder);
    builder.close();
    for edge in &mut builder.edges {
        edge[0] /= units;
        edge[1] /= units;
    }
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for edge in &builder.edges {
        for point in edge {
            min = min.min(*point);
            max = max.max(*point);
        }
    }
    if builder.edges.is_empty() {
        min = Vec2::ZERO;
        max = Vec2::ZERO;
    }
    let result = Arc::new(Outline {
        advance: face.glyph_hor_advance(glyph).unwrap_or(0) as f32 / units,
        edges: builder.edges,
        min,
        max,
    });
    cache.lock().unwrap().insert(character, result.clone());
    Ok(result)
}

/// The path matrix maps path-local control points into text-local coordinates.
/// Text advances from the first path anchor by arc length. Glyphs are rigid and
/// face the path tangent, with up parallel to the curve's local XY plane.
impl TextParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.text.is_empty() {
            return Err("text is empty".into());
        }
        if !self.size.is_finite()
            || self.size <= 0.0
            || !self.half_depth.is_finite()
            || self.half_depth <= 0.0
            || !self.tracking.is_finite()
            || !self.line_spacing.is_finite()
            || self.line_spacing <= 0.0
        {
            return Err("text size, depth, tracking, or line spacing is invalid".into());
        }
        if self.text.chars().count() > 256 {
            return Err("text exceeds 256 characters".into());
        }
        if self.path.is_some() && self.text.contains('\n') {
            return Err("text on a path must be a single line".into());
        }
        Ok(())
    }

    pub fn prepared(&self, path: Option<(&BezierCurveParams, Mat4)>) -> PreparedText {
        self.try_prepared(path).unwrap_or_default()
    }

    pub fn try_prepared(
        &self,
        path: Option<(&BezierCurveParams, Mat4)>,
    ) -> Result<PreparedText, String> {
        self.validate()?;
        if self.path.is_some() && path.is_none() {
            return Err("text path is missing".into());
        }
        let path_samples = if let Some((curve, matrix)) = path {
            let mut samples = Vec::new();
            let mut length = 0.0;
            for segment in 0..curve.segment_count() {
                for step in 0..=32 {
                    if segment > 0 && step == 0 {
                        continue;
                    }
                    let point = matrix.transform_point3(curve.point(segment, step as f32 / 32.0));
                    if let Some((_, last, _, _)) = samples.last() {
                        length += point.distance(*last);
                    }
                    let tangent = curve.tangent(segment, step as f32 / 32.0);
                    let (_, carried_up) =
                        super::curve_profile_frame(curve, tangent.normalize_or_zero());
                    samples.push((
                        length,
                        point,
                        matrix.transform_vector3(tangent),
                        matrix.transform_vector3(-carried_up),
                    ));
                }
            }
            samples
        } else {
            Vec::new()
        };
        if self.path.is_some() && path_samples.len() < 2 {
            return Err("text path has no segments".into());
        }
        let mut prepared = PreparedText {
            half_depth: self.half_depth,
            minimum: Vec3::splat(f32::INFINITY),
            maximum: Vec3::splat(f32::NEG_INFINITY),
            glyphs: Vec::new(),
        };
        let mut cursor = 0.0;
        let mut line = 0usize;
        for character in self.text.chars() {
            if character == '\n' {
                cursor = 0.0;
                line += 1;
                continue;
            }
            let shape = outline(character)?;
            let advance = shape.advance * self.size + self.tracking;
            if advance <= 0.0 {
                return Err("tracking makes glyph advance nonpositive".into());
            }
            if let Some((length, _, _, _)) = path_samples.last() {
                if cursor + advance > *length + 0.0001 {
                    return Err("text extends beyond the path".into());
                }
            }
            let (origin, x, y, z) = if path_samples.is_empty() {
                (
                    Vec3::new(cursor, -(line as f32) * self.line_spacing * self.size, 0.0),
                    Vec3::X,
                    Vec3::Y,
                    Vec3::Z,
                )
            } else {
                let center = cursor + advance * 0.5;
                let index = path_samples
                    .partition_point(|entry| entry.0 < center)
                    .clamp(1, path_samples.len() - 1);
                let (a_len, a, _, _) = path_samples[index - 1];
                let (b_len, b, tangent, side) = path_samples[index];
                let t = ((center - a_len) / (b_len - a_len).max(1e-6)).clamp(0.0, 1.0);
                let x = tangent.normalize_or_zero();
                let y = (side - x * side.dot(x)).normalize_or_zero();
                let z = x.cross(y).normalize_or_zero();
                if x.length_squared() < 0.5 || y.length_squared() < 0.5 {
                    return Err("text path has a degenerate tangent".into());
                }
                let origin = a.lerp(b, t) - x * advance * 0.5;
                (origin, x, y, z)
            };
            if !shape.edges.is_empty() {
                let glyph = TextGlyph {
                    origin,
                    x,
                    y,
                    z,
                    min: shape.min * self.size,
                    max: shape.max * self.size,
                    edges: shape
                        .edges
                        .iter()
                        .map(|edge| [edge[0] * self.size, edge[1] * self.size])
                        .collect(),
                };
                if glyph.edges.len() > 1024 {
                    return Err("a text glyph exceeds the outline edge limit".into());
                }
                for px in [glyph.min.x, glyph.max.x] {
                    for py in [glyph.min.y, glyph.max.y] {
                        for pz in [-self.half_depth, self.half_depth] {
                            let corner = origin + x * px + y * py + z * pz;
                            prepared.minimum = prepared.minimum.min(corner);
                            prepared.maximum = prepared.maximum.max(corner);
                        }
                    }
                }
                prepared.glyphs.push(glyph);
            }
            cursor += advance;
        }
        if prepared.packed_points() > MAX_TEXT_POINTS {
            return Err(format!("text outlines exceed {MAX_TEXT_POINTS} GPU points"));
        }
        if prepared.glyphs.is_empty() {
            prepared.minimum = Vec3::ZERO;
            prepared.maximum = Vec3::ZERO;
        }
        Ok(prepared)
    }
}

/// Resolve the path through world transforms. Cache is keyed by the actual text
/// parameters and curve points/matrices, so editing either object invalidates it.
pub fn prepared_scene_text(
    scene: &[SdfObject],
    object: &SdfObject,
) -> Result<Arc<PreparedText>, String> {
    let SdfParams::TextParams(params) = &object.params else {
        return Err("object is not text".into());
    };
    let path = if let Some(id) = params.path {
        let path_object = scene
            .iter()
            .find(|candidate| candidate.uuid == id)
            .ok_or("text path is missing")?;
        let SdfParams::BezierCurveParams(curve) = &path_object.params else {
            return Err("text path must reference a Bézier curve".into());
        };
        let matrix = super::object_world_matrix(scene, object.uuid).inverse()
            * super::object_world_matrix(scene, id);
        Some((curve, matrix))
    } else {
        None
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(params)
        .map_err(|error| error.to_string())?
        .hash(&mut hasher);
    if let Some((curve, matrix)) = path {
        serde_json::to_string(curve)
            .map_err(|error| error.to_string())?
            .hash(&mut hasher);
        for value in matrix.to_cols_array() {
            value.to_bits().hash(&mut hasher);
        }
    }
    let key = hasher.finish();
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<PreparedText>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().unwrap().get(&key) {
        return Ok(found.clone());
    }
    let prepared = Arc::new(params.try_prepared(path)?);
    let mut cache = cache.lock().unwrap();
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert(key, prepared.clone());
    Ok(prepared)
}

pub fn validate_scene_text_budget(scene: &[SdfObject]) -> Result<(), String> {
    let mut total = 0;
    for object in scene {
        if matches!(object.params, SdfParams::TextParams(_)) {
            total += prepared_scene_text(scene, object)?.packed_points();
            if total > MAX_SCENE_TEXT_POINTS {
                return Err(format!(
                    "scene text outlines exceed {MAX_SCENE_TEXT_POINTS} GPU points"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holes_and_extrusion_have_signed_distance() {
        let text = TextParams {
            text: "O".into(),
            size: 1.0,
            half_depth: 0.1,
            ..TextParams::default()
        };
        let prepared = text.try_prepared(None).unwrap();
        let glyph = &prepared.glyphs[0];
        let center = (glyph.min + glyph.max) * 0.5;
        assert!(
            prepared.distance(center.extend(0.0)) > 0.0,
            "O's counter must be empty"
        );
        let mut solid = None;
        for i in 0..30 {
            for j in 0..30 {
                let flat = glyph.min
                    + (glyph.max - glyph.min) * Vec2::new(i as f32 / 29.0, j as f32 / 29.0);
                if prepared.distance(flat.extend(0.0)) < -0.01 {
                    solid = Some(flat);
                    break;
                }
            }
        }
        let flat = solid.expect("glyph must have a stroke");
        assert!(
            prepared.distance(flat.extend(0.2)) > 0.0,
            "extruded glyph must have a flat cap"
        );
    }

    #[test]
    fn multiline_layout_and_path_transform() {
        let text = TextParams {
            text: "A\nA".into(),
            size: 0.5,
            ..TextParams::default()
        };
        let straight = text.try_prepared(None).unwrap();
        assert_eq!(straight.glyphs.len(), 2);
        assert!((straight.glyphs[1].origin.y + 0.65).abs() < 1e-4);
        let curve = BezierCurveParams {
            points: vec![Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::X * 3.0],
            closed: false,
        };
        let path_text = TextParams {
            text: "AA".into(),
            path: Some(uuid::Uuid::new_v4()),
            ..TextParams::default()
        };
        let placed = path_text
            .try_prepared(Some((&curve, Mat4::from_translation(Vec3::Y))))
            .unwrap();
        assert!(placed.glyphs[0].origin.y > 0.9);
        assert!(placed.glyphs[1].origin.x > placed.glyphs[0].origin.x);
        assert!(path_text
            .try_prepared(Some((&curve, Mat4::IDENTITY)))
            .is_ok());
        assert!(TextParams {
            text: "A".repeat(30),
            ..path_text
        }
        .try_prepared(Some((&curve, Mat4::IDENTITY)))
        .is_err());
    }

    #[test]
    fn typed_params_roundtrip_and_reject_invalid_tracking() {
        let original = TextParams {
            text: "B8\nO".into(),
            ..TextParams::default()
        };
        let stored = serde_json::to_string(&original).unwrap();
        let restored: TextParams = serde_json::from_str(&stored).unwrap();
        assert_eq!(restored, original);
        let invalid = TextParams {
            tracking: -100.0,
            ..original
        };
        assert!(invalid.try_prepared(None).is_err());
    }

    #[test]
    fn referenced_path_edit_invalidates_prepared_scene_geometry() {
        let mut path = SdfObject::create_kind(super::super::PrimitiveKind::BezierCurve);
        path.params = SdfParams::BezierCurveParams(BezierCurveParams {
            points: vec![Vec3::ZERO, Vec3::Z, Vec3::Z * 2.0, Vec3::Z * 3.0],
            closed: false,
        });
        let mut text = SdfObject::create_kind(super::super::PrimitiveKind::Text);
        text.params = SdfParams::TextParams(TextParams {
            text: "AB".into(),
            path: Some(path.uuid),
            ..TextParams::default()
        });
        let scene = vec![path.clone(), text.clone()];
        let first = prepared_scene_text(&scene, &text).unwrap();
        assert!(
            first.glyphs[0].x.z > 0.9,
            "Z-directed path needs a valid carried frame"
        );
        assert!(first.glyphs[0].y.length_squared() > 0.9);
        path.transform.translation = Vec3::Y;
        let moved_scene = vec![path, text.clone()];
        let second = prepared_scene_text(&moved_scene, &text).unwrap();
        assert!(second.minimum.y > first.minimum.y + 0.9);
    }
}
