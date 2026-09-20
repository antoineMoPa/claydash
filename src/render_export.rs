use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use openh264::{
    encoder::{
        Encoder, EncoderConfig, FrameRate, FrameType, QpRange, RateControlMode, UsageType,
        VuiConfig,
    },
    formats::{RgbaSliceU8, YUVBuffer},
};

use crate::{camera::Camera, document::RenderFormat, renderer::CapturedFrame};

pub fn crop_to_viewport(frame: CapturedFrame, camera: &Camera) -> CapturedFrame {
    let x = (camera.viewport_origin.x.max(0.0) as u32).min(frame.width.saturating_sub(1));
    let y = (camera.viewport_origin.y.max(0.0) as u32).min(frame.height.saturating_sub(1));
    let width = (camera.viewport.x.max(1.0) as u32).min(frame.width - x);
    let height = (camera.viewport.y.max(1.0) as u32).min(frame.height - y);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in y..y + height {
        let start = ((row * frame.width + x) * 4) as usize;
        rgba.extend_from_slice(&frame.rgba[start..start + (width * 4) as usize]);
    }
    CapturedFrame {
        width,
        height,
        rgba,
    }
}

pub fn write_render(
    path: &Path,
    format: RenderFormat,
    frame: &CapturedFrame,
) -> Result<(), String> {
    let png = png_bytes(frame)?;
    match format {
        RenderFormat::WebP => convert_webp(path, &png),
        RenderFormat::Mp4 => Err("MP4 export requires an animation frame sequence".into()),
    }
}

pub fn write_video_frame(
    directory: &Path,
    index: u32,
    frame: &CapturedFrame,
) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(frame.rgba.len() + 8);
    bytes.extend_from_slice(&frame.width.to_le_bytes());
    bytes.extend_from_slice(&frame.height.to_le_bytes());
    bytes.extend_from_slice(&frame.rgba);
    std::fs::write(video_frame_path(directory, index), bytes).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_mp4(path: &Path, directory: &Path, fps: f32) -> Result<(), String> {
    let first = read_video_frame(&video_frame_path(directory, 0))?;
    let first = pad_for_h264(first);
    let width = u16::try_from(first.width).map_err(|_| "video width exceeds 65535 pixels")?;
    let height = u16::try_from(first.height).map_err(|_| "video height exceeds 65535 pixels")?;
    let frame_rate = fps.clamp(1.0, 240.0);
    let config = EncoderConfig::new()
        .skip_frames(false)
        .max_frame_rate(FrameRate::from_hz(frame_rate.min(30.0)))
        .usage_type(UsageType::ScreenContentRealTime)
        .rate_control_mode(RateControlMode::Off)
        .qp(QpRange::new(18, 32))
        .adaptive_quantization(false)
        .background_detection(false)
        .vui(VuiConfig::srgb());
    let mut encoder = Encoder::with_api_config(openh264::OpenH264API::from_source(), config)
        .map_err(|error| format!("could not initialize the bundled video encoder: {error}"))?;
    let first = encode_frame(&mut encoder, &first)?;
    let sps = first
        .sequence_parameter_set
        .clone()
        .ok_or("the video encoder did not produce a sequence parameter set")?;
    let pps = first
        .picture_parameter_set
        .clone()
        .ok_or("the video encoder did not produce a picture parameter set")?;

    let timescale = 90_000;
    let sample_duration = ((timescale as f32 / frame_rate).round() as u32).max(1);
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mp4_config = mp4::Mp4Config {
        major_brand: "isom"
            .parse()
            .map_err(|error| format!("invalid MP4 brand: {error}"))?,
        minor_version: 512,
        compatible_brands: ["isom", "iso2", "avc1", "mp41"]
            .into_iter()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("invalid MP4 brand: {error}"))?,
        timescale,
    };
    let mut writer = mp4::Mp4Writer::write_start(std::io::BufWriter::new(file), &mp4_config)
        .map_err(|error| format!("could not begin the MP4 file: {error}"))?;
    let track = mp4::TrackConfig {
        track_type: mp4::TrackType::Video,
        timescale,
        language: "und".to_owned(),
        media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
            width,
            height,
            seq_param_set: sps,
            pic_param_set: pps,
        }),
    };
    writer
        .add_track(&track)
        .map_err(|error| format!("could not add the MP4 video track: {error}"))?;
    write_mp4_sample(&mut writer, first, 0, sample_duration)?;

    let mut index = 1;
    loop {
        let frame_path = video_frame_path(directory, index);
        if !frame_path.exists() {
            break;
        }
        let frame = pad_for_h264(read_video_frame(&frame_path)?);
        if frame.width != u32::from(width) || frame.height != u32::from(height) {
            return Err(format!(
                "video frame {index} is {}x{}, expected {width}x{height}",
                frame.width, frame.height
            ));
        }
        let encoded = encode_frame(&mut encoder, &frame)?;
        write_mp4_sample(&mut writer, encoded, index, sample_duration)?;
        index += 1;
    }
    writer
        .write_end()
        .map_err(|error| format!("could not finish the MP4 file: {error}"))
}

fn video_frame_path(directory: &Path, index: u32) -> PathBuf {
    directory.join(format!("frame-{index:06}.rgba"))
}

fn read_video_frame(path: &Path) -> Result<CapturedFrame, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() < 8 {
        return Err(format!(
            "video frame {} has an invalid header",
            path.display()
        ));
    }
    let width = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let expected = width as usize * height as usize * 4;
    if bytes.len() - 8 != expected {
        return Err(format!(
            "video frame {} contains {} bytes, expected {expected}",
            path.display(),
            bytes.len() - 8
        ));
    }
    Ok(CapturedFrame {
        width,
        height,
        rgba: bytes[8..].to_vec(),
    })
}

fn pad_for_h264(frame: CapturedFrame) -> CapturedFrame {
    let width = frame.width + frame.width % 2;
    let height = frame.height + frame.height % 2;
    if width == frame.width && height == frame.height {
        return frame;
    }
    let mut rgba = vec![0; (width * height * 4) as usize];
    for y in 0..frame.height {
        let source_start = (y * frame.width * 4) as usize;
        let target_start = (y * width * 4) as usize;
        rgba[target_start..target_start + (frame.width * 4) as usize]
            .copy_from_slice(&frame.rgba[source_start..source_start + (frame.width * 4) as usize]);
    }
    CapturedFrame {
        width,
        height,
        rgba,
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct EncodedFrame {
    bytes: Vec<u8>,
    is_sync: bool,
    sequence_parameter_set: Option<Vec<u8>>,
    picture_parameter_set: Option<Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_frame(encoder: &mut Encoder, frame: &CapturedFrame) -> Result<EncodedFrame, String> {
    let rgba = RgbaSliceU8::new(&frame.rgba, (frame.width as usize, frame.height as usize));
    let yuv = YUVBuffer::from_rgb_source(rgba);
    let stream = encoder
        .encode(&yuv)
        .map_err(|error| format!("could not encode a video frame: {error}"))?;
    let mut bytes = Vec::new();
    let mut sequence_parameter_set = None;
    let mut picture_parameter_set = None;
    for layer_index in 0..stream.num_layers() {
        let layer = stream.layer(layer_index).unwrap();
        for nal_index in 0..layer.nal_count() {
            let nal = strip_annex_b_prefix(layer.nal_unit(nal_index).unwrap())?;
            match nal[0] & 0x1f {
                7 => sequence_parameter_set = Some(nal.to_vec()),
                8 => picture_parameter_set = Some(nal.to_vec()),
                _ => {
                    let length = u32::try_from(nal.len())
                        .map_err(|_| "an encoded video packet is too large")?;
                    bytes.extend_from_slice(&length.to_be_bytes());
                    bytes.extend_from_slice(nal);
                }
            }
        }
    }
    Ok(EncodedFrame {
        bytes,
        is_sync: matches!(stream.frame_type(), FrameType::IDR | FrameType::I),
        sequence_parameter_set,
        picture_parameter_set,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn strip_annex_b_prefix(nal: &[u8]) -> Result<&[u8], String> {
    let payload = if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        return Err("the video encoder produced an invalid H.264 packet".into());
    };
    if payload.is_empty() {
        Err("the video encoder produced an empty H.264 packet".into())
    } else {
        Ok(payload)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_mp4_sample<W: std::io::Write + std::io::Seek>(
    writer: &mut mp4::Mp4Writer<W>,
    frame: EncodedFrame,
    index: u32,
    duration: u32,
) -> Result<(), String> {
    writer
        .write_sample(
            1,
            &mp4::Mp4Sample {
                start_time: u64::from(index) * u64::from(duration),
                duration,
                rendering_offset: 0,
                is_sync: frame.is_sync,
                bytes: frame.bytes.into(),
            },
        )
        .map_err(|error| format!("could not write video frame {index} to the MP4 file: {error}"))
}

fn png_bytes(frame: &CapturedFrame) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(&frame.rgba)
            .map_err(|error| error.to_string())?;
    }
    Ok(bytes)
}

fn convert_webp(path: &Path, png: &[u8]) -> Result<(), String> {
    let source = std::env::temp_dir().join(format!("claydash-render-{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&source, png).map_err(|error| error.to_string())?;
    let result = std::process::Command::new("cwebp")
        .args(["-quiet", "-q", "90"])
        .arg(&source)
        .arg("-o")
        .arg(path)
        .output();
    let _ = std::fs::remove_file(&source);
    let output = result.map_err(|error| format!("could not start the format encoder: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_encoder_writes_webp() {
        let available = |program: &str| {
            std::process::Command::new(program)
                .arg("-version")
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !available("cwebp") {
            return;
        }
        let frame = CapturedFrame {
            width: 2,
            height: 2,
            rgba: vec![80, 120, 200, 255].repeat(4),
        };
        let directory = std::env::temp_dir().join(format!(
            "claydash-compressed-export-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("render.webp");
        write_render(&path, RenderFormat::WebP, &frame).unwrap();
        assert!(std::fs::metadata(path).unwrap().len() > 32);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bundled_encoder_writes_every_video_frame_to_a_valid_mp4() {
        let directory = std::env::temp_dir().join(format!(
            "claydash-video-export-test-{}",
            uuid::Uuid::new_v4()
        ));
        let first = CapturedFrame {
            width: 15,
            height: 15,
            rgba: vec![255, 0, 0, 255].repeat(15 * 15),
        };
        let second = CapturedFrame {
            width: 15,
            height: 15,
            rgba: vec![0, 255, 0, 255].repeat(15 * 15),
        };
        write_video_frame(&directory, 0, &first).unwrap();
        write_video_frame(&directory, 1, &second).unwrap();
        assert_ne!(
            std::fs::read(directory.join("frame-000000.rgba")).unwrap(),
            std::fs::read(directory.join("frame-000001.rgba")).unwrap()
        );
        let path = directory.join("render.mp4");
        write_mp4(&path, &directory, 24.0).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let size = file.metadata().unwrap().len();
        let video = mp4::Mp4Reader::read_header(std::io::BufReader::new(file), size).unwrap();
        let track = video.tracks().get(&1).unwrap();
        assert_eq!((track.width(), track.height()), (16, 16));
        assert_eq!(track.media_type().unwrap(), mp4::MediaType::H264);
        assert_eq!(track.sample_count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
