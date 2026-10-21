//! Import the standard Graphdeco 3D Gaussian PLY layout. Assets are embedded in
//! scene objects, so saving a project never depends on the original file path.
use base64::{engine::general_purpose::STANDARD, Engine};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq)]
pub struct GaussianCloud {
    pub splats: Vec<Gaussian>,
}

const MAX_SPLATS: usize = 2_000_000;

#[derive(Serialize, Deserialize)]
struct PackedCloud {
    encoding: String,
    count: usize,
    data: String,
}

impl Serialize for GaussianCloud {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bytes = encode_batch(&self.splats);
        PackedCloud {
            encoding: "gaussian-f32-le-v1".into(),
            count: self.splats.len(),
            data: STANDARD.encode(bytes),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GaussianCloud {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = PackedCloud::deserialize(deserializer)?;
        let error = || serde::de::Error::custom("Invalid embedded Gaussian asset");
        if stored.encoding != "gaussian-f32-le-v1"
            || stored.count == 0
            || stored.count > MAX_SPLATS
            || stored.data.len() > stored.count * 316
        {
            return Err(error());
        }
        let bytes = STANDARD.decode(&stored.data).map_err(|_| error())?;
        let splats = decode_batch(&bytes, stored.count).map_err(|_| error())?;
        Ok(Self { splats })
    }
}

/// Compact batches transferred from the web import worker. The layout matches
/// the existing embedded asset representation, without its base64 wrapper.
pub fn encode_batch(splats: &[Gaussian]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for g in splats {
        bytes.push(g.sh_rest.len() as u8);
        for value in g.position.to_array().into_iter()
            .chain(g.scale.to_array())
            .chain(g.rotation.to_array())
            .chain(g.color)
            .chain([g.opacity])
            .chain(g.sh_rest.iter().copied())
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

pub fn decode_batch(bytes: &[u8], count: usize) -> Result<Vec<Gaussian>, String> {
    if count > MAX_SPLATS { return Err("Too many Gaussians".into()); }
    let error = || "Invalid Gaussian batch".to_string();
    let mut data = bytes;
    let mut splats = Vec::with_capacity(count.min(bytes.len() / 57));
    for _ in 0..count {
        let (&sh_count, rest) = data.split_first().ok_or_else(error)?;
        if !matches!(sh_count, 0 | 9 | 24 | 45) {
            return Err(error());
        }
        data = rest;
        let mut values = Vec::with_capacity(14 + sh_count as usize);
        for _ in 0..14 + sh_count as usize {
            values.push(Scalar::F32.read(&mut data, false).map_err(|_| error())?);
        }
        let g = Gaussian {
            position: Vec3::from_slice(&values[0..3]),
            scale: Vec3::from_slice(&values[3..6]),
            rotation: Quat::from_xyzw(values[6], values[7], values[8], values[9]),
            color: [values[10], values[11], values[12]],
            opacity: values[13],
            sh_rest: values[14..].to_vec(),
        };
        if values.iter().any(|v| !v.is_finite())
            || g.scale.min_element() <= 0.
            || (g.rotation.length_squared() - 1.).abs() > 0.001
            || !(0. ..=1.).contains(&g.opacity)
            || g.color.iter().any(|c| *c < 0.)
        {
            return Err(error());
        }
        splats.push(g);
    }
    if !data.is_empty() {
        return Err(error());
    }
    Ok(splats)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gaussian {
    pub position: Vec3,
    pub scale: Vec3,
    pub rotation: Quat,
    pub color: [f32; 3],
    pub opacity: f32,
    /// Preserved for future view-dependent shading; the viewport uses DC color.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sh_rest: Vec<f32>,
}

#[derive(Clone, Copy)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}
impl Scalar {
    fn parse(s: &str) -> Result<Self, String> {
        Ok(match s {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return Err(format!("Unsupported PLY scalar type: {s}")),
        })
    }
    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::F64 => 8,
            _ => 4,
        }
    }
    fn read(self, bytes: &mut &[u8], big: bool) -> Result<f32, String> {
        let n = self.size();
        if bytes.len() < n {
            return Err("Truncated PLY vertex data".into());
        }
        let (data, rest) = bytes.split_at(n);
        *bytes = rest;
        macro_rules! read {
            ($t:ty) => {{
                let a = data.try_into().unwrap();
                if big {
                    <$t>::from_be_bytes(a) as f32
                } else {
                    <$t>::from_le_bytes(a) as f32
                }
            }};
        }
        Ok(match self {
            Self::I8 => data[0] as i8 as f32,
            Self::U8 => data[0] as f32,
            Self::I16 => read!(i16),
            Self::U16 => read!(u16),
            Self::I32 => read!(i32),
            Self::U32 => read!(u32),
            Self::F32 => read!(f32),
            Self::F64 => read!(f64),
        })
    }
}

pub fn read(bytes: &[u8]) -> Result<GaussianCloud, String> {
    read_with_progress(bytes, |_, _| {})
}

pub fn read_with_progress(bytes: &[u8], mut progress: impl FnMut(usize, usize)) -> Result<GaussianCloud, String> {
    let mut offset = 0;
    let mut lines = Vec::new();
    loop {
        let end = bytes[offset..]
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("Missing PLY end_header")?
            + offset;
        if end > 1024 * 1024 {
            return Err("PLY header exceeds 1 MiB".into());
        }
        let line = std::str::from_utf8(&bytes[offset..end])
            .map_err(|_| "Invalid PLY header")?
            .trim();
        offset = end + 1;
        if line == "end_header" {
            break;
        }
        lines.push(line);
    }
    if lines.first() != Some(&"ply") {
        return Err("Not a PLY file".into());
    }
    let mut format = None;
    let mut count = None;
    let mut vertex = false;
    let mut properties = Vec::new();
    for line in &lines[1..] {
        let words: Vec<_> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", f @ ("ascii" | "binary_little_endian" | "binary_big_endian"), "1.0"] => {
                format = Some(*f)
            }
            ["comment", ..] | ["obj_info", ..] | [] => {}
            ["element", name, n] => {
                let n: usize = n.parse().map_err(|_| "Invalid PLY element count")?;
                vertex = *name == "vertex";
                if vertex {
                    if count.is_some() {
                        return Err("Duplicate PLY vertex element".into());
                    }
                    count = Some(n);
                } else if count.is_none() && n != 0 {
                    return Err("PLY vertex must be the first nonempty element".into());
                }
            }
            ["property", ty, name] if vertex => {
                if properties.iter().any(|(_, old)| old == name) {
                    return Err(format!("Duplicate PLY property: {name}"));
                }
                properties.push((Scalar::parse(ty)?, *name));
            }
            ["property", ..] if !vertex => {}
            _ => return Err(format!("Unsupported PLY header entry: {line}")),
        }
    }
    let format = format.ok_or("Unsupported or missing PLY format")?;
    let count = count.ok_or("Missing PLY vertex element")?;
    if count == 0 || count > MAX_SPLATS {
        return Err("PLY must contain 1 to 2,000,000 Gaussians".into());
    }
    let required = [
        "x", "y", "z", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
        "f_dc_0", "f_dc_1", "f_dc_2", "opacity",
    ];
    let indices: Vec<usize> = required
        .iter()
        .map(|name| {
            properties
                .iter()
                .position(|(_, p)| p == name)
                .ok_or_else(|| format!("Not a Gaussian PLY: missing {name}"))
        })
        .collect::<Result<_, _>>()?;
    let mut sh: Vec<_> = properties
        .iter()
        .enumerate()
        .filter_map(|(i, (_, name))| {
            name.strip_prefix("f_rest_")
                .and_then(|n| n.parse::<usize>().ok())
                .map(|n| (n, i))
        })
        .collect();
    sh.sort_unstable();
    if !matches!(sh.len(), 0 | 9 | 24 | 45) || sh.iter().enumerate().any(|(i, (n, _))| i != *n) {
        return Err("Unsupported spherical harmonic properties".into());
    }
    let mut data = &bytes[offset..];
    let stride: usize = properties.iter().map(|(ty, _)| ty.size()).sum();
    if format != "ascii"
        && count
            .checked_mul(stride)
            .is_none_or(|size| size > data.len())
    {
        return Err("Truncated PLY vertex data".into());
    }
    let ascii = if format == "ascii" {
        std::str::from_utf8(data).map_err(|_| "Invalid ASCII PLY")?
    } else {
        ""
    };
    let mut tokens = ascii.split_whitespace();
    let mut splats = Vec::new();
    let mut values = vec![0.; properties.len()];
    for row in 0..count {
        for (i, (ty, _)) in properties.iter().enumerate() {
            values[i] = if format == "ascii" {
                tokens
                    .next()
                    .ok_or("Truncated PLY vertex data")?
                    .parse()
                    .map_err(|_| "Invalid PLY number")?
            } else {
                ty.read(&mut data, format == "binary_big_endian")?
            };
            if !values[i].is_finite() {
                return Err(format!("Non-finite PLY value at vertex {row}"));
            }
        }
        let v = |i: usize| values[indices[i]];
        let scale = Vec3::new(v(3).exp(), v(4).exp(), v(5).exp());
        let q = Quat::from_xyzw(v(7), v(8), v(9), v(6));
        let length = q.length();
        if !scale.is_finite() || scale.min_element() <= 0. || !length.is_finite() || length < 1e-12
        {
            return Err(format!(
                "Invalid Gaussian scale or rotation at vertex {row}"
            ));
        }
        splats.push(Gaussian {
            position: Vec3::new(v(0), v(1), v(2)),
            scale,
            rotation: q / length,
            color: [v(10), v(11), v(12)].map(|c| (0.5 + 0.2820948 * c).max(0.)),
            opacity: 1. / (1. + (-v(13)).exp()),
            sh_rest: sh.iter().map(|(_, i)| values[*i]).collect(),
        });
        if row % 4096 == 4095 || row + 1 == count { progress(row + 1, count); }
    }
    Ok(GaussianCloud { splats })
}

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: [&str; 14] = [
        "x", "y", "z", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
        "f_dc_0", "f_dc_1", "f_dc_2", "opacity",
    ];
    fn header(format: &str) -> String {
        format!(
            "ply\nformat {format} 1.0\ncomment fixture\nelement vertex 1\n{}end_header\n",
            NAMES.map(|n| format!("property float {n}\n")).join("")
        )
    }
    #[test]
    fn ascii_and_both_binary_orders_map_gaussian_properties() {
        let values: [f32; 14] = [1., 2., 3., -1., -2., -3., 2., 0., 0., 0., 1., 0., -1., 0.];
        let ascii = format!(
            "{}{}",
            header("ascii"),
            values.map(|n| n.to_string()).join(" ")
        );
        let expected = read(ascii.as_bytes()).unwrap();
        let mut progress = Vec::new();
        assert_eq!(read_with_progress(ascii.as_bytes(), |done, total| progress.push((done, total))).unwrap(), expected);
        assert_eq!(progress, vec![(1, 1)]);
        let g = &expected.splats[0];
        assert_eq!(g.position, Vec3::new(1., 2., 3.));
        assert_eq!(g.rotation, Quat::IDENTITY);
        assert!((g.scale.x - (-1f32).exp()).abs() < 1e-6);
        assert_eq!(g.opacity, 0.5);
        assert!((g.color[0] - 0.7820948).abs() < 1e-6);
        for big in [false, true] {
            let mut bytes = header(if big {
                "binary_big_endian"
            } else {
                "binary_little_endian"
            })
            .into_bytes();
            for v in values {
                bytes.extend(if big {
                    v.to_be_bytes()
                } else {
                    v.to_le_bytes()
                });
            }
            assert_eq!(read(&bytes).unwrap(), expected);
            bytes.pop();
            assert!(read(&bytes).unwrap_err().contains("Truncated"));
        }
        assert!(read(
            ascii
                .replace("property float opacity", "property float alpha")
                .as_bytes()
        )
        .unwrap_err()
        .contains("opacity"));
        assert!(read(ascii.replace("1 2 3", "NaN 2 3").as_bytes()).is_err());
        assert!(read(b"ply\nformat ascii 1.0\n").is_err());
    }
}

#[cfg(test)]
mod asset_tests {
    use super::*;
    #[test]
    fn worker_batch_round_trip_and_rejects_truncation() {
        let splats = vec![Gaussian {
            position: Vec3::new(1., 2., 3.),
            scale: Vec3::splat(0.2),
            rotation: Quat::IDENTITY,
            color: [0.1, 0.2, 0.3],
            opacity: 0.8,
            sh_rest: vec![0.25; 9],
        }];
        let mut bytes = encode_batch(&splats);
        assert_eq!(decode_batch(&bytes, 1).unwrap(), splats);
        assert!(decode_batch(&bytes, 2).is_err());
        bytes.pop();
        assert!(decode_batch(&bytes, 1).is_err());
    }
    #[test]
    fn packed_asset_retains_all_coefficients_and_rejects_corrupt_payloads() {
        let cloud = GaussianCloud {
            splats: vec![Gaussian {
                position: Vec3::new(1., 2., 3.),
                scale: Vec3::new(0.1, 0.2, 0.3),
                rotation: Quat::from_rotation_x(0.7),
                color: [0.2, 0.7, 1.],
                opacity: 0.3,
                sh_rest: (0..45).map(|i| i as f32 / 100.).collect(),
            }],
        };
        let packed = serde_json::to_value(&cloud).unwrap();
        assert_eq!(packed["encoding"], "gaussian-f32-le-v1");
        assert_eq!(
            serde_json::from_value::<GaussianCloud>(packed.clone()).unwrap(),
            cloud
        );
        let mut corrupt = packed.clone();
        corrupt["count"] = 2.into();
        assert!(serde_json::from_value::<GaussianCloud>(corrupt).is_err());
        let mut corrupt = packed;
        corrupt["encoding"] = "unknown".into();
        assert!(serde_json::from_value::<GaussianCloud>(corrupt).is_err());
    }
    #[test]
    fn imported_asset_is_pickable_without_becoming_solid_geometry() {
        let mut object = crate::model::SdfObject::create_kind(crate::model::PrimitiveKind::Box);
        object.imported_splats = Some(std::sync::Arc::new(GaussianCloud {
            splats: vec![Gaussian {
                position: Vec3::new(3., 0., 0.),
                scale: Vec3::new(0.2, 0.1, 0.1),
                rotation: Quat::IDENTITY,
                color: [1., 0., 0.],
                opacity: 1.,
                sh_rest: vec![],
            }],
        }));
        assert!(object.distance(Vec3::ZERO).is_infinite());
        let scene = [object];
        assert_eq!(
            crate::model::raymarch(Vec3::new(3., 0., 4.), Vec3::NEG_Z, &scene),
            Some(scene[0].uuid)
        );
        assert!(crate::model::raymarch(Vec3::new(0., 0., 4.), Vec3::NEG_Z, &scene).is_none());
    }
}

#[cfg(test)]
mod picking_regressions {
    use super::*;
    use crate::model::{raymarch, PrimitiveKind, SdfObject};

    #[test]
    fn splat_support_containing_camera_does_not_occlude_cube_at_center_depth() {
        let cube = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cloud = SdfObject::create_kind(PrimitiveKind::Box);
        cloud.imported_splats = Some(std::sync::Arc::new(GaussianCloud {
            splats: vec![Gaussian { position: Vec3::new(0., 0., -3.),
                scale: Vec3::splat(10.), rotation: Quat::IDENTITY,
                color: [1.; 3], opacity: 1., sh_rest: vec![] }],
        }));
        let origin = Vec3::new(0., 0., 4.);
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud.clone()]), Some(cube.uuid));
        // Reversing scene order must not change visibility selection.
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cloud.clone(), cube.clone()]), Some(cube.uuid));
        let data = std::sync::Arc::make_mut(cloud.imported_splats.as_mut().unwrap());
        data.splats[0].position.z = 8.;
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud.clone()]), Some(cube.uuid));
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cloud]), None);
    }

    #[test]
    fn faint_foreground_tail_leaves_cube_pickable_but_visible_cloud_selects() {
        let cube = SdfObject::create_kind(PrimitiveKind::Box);
        let mut cloud = SdfObject::create_kind(PrimitiveKind::Box);
        let sample = Gaussian { position: Vec3::new(0., 0., 1.),
            scale: Vec3::splat(0.2), rotation: Quat::IDENTITY,
            color: [1.; 3], opacity: 0.05, sh_rest: vec![] };
        cloud.imported_splats = Some(std::sync::Arc::new(GaussianCloud { splats: vec![sample.clone()] }));
        let origin = Vec3::new(0., 0., 4.);
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud.clone()]), Some(cube.uuid));
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cloud.clone()]), None);
        // Several low-opacity layers can still form a clearly visible cloud.
        std::sync::Arc::make_mut(cloud.imported_splats.as_mut().unwrap()).splats = vec![sample; 8];
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud.clone()]), Some(cloud.uuid));
        let data = std::sync::Arc::make_mut(cloud.imported_splats.as_mut().unwrap());
        data.splats.truncate(1);
        data.splats[0].opacity = 1.;
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud.clone()]), Some(cloud.uuid));
        // A high-opacity Gaussian's almost invisible outer support is not solid.
        std::sync::Arc::make_mut(cloud.imported_splats.as_mut().unwrap()).splats[0].position.x = 0.45;
        assert_eq!(raymarch(origin, Vec3::NEG_Z, &[cube.clone(), cloud]), Some(cube.uuid));
    }
}
