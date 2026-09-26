use crate::{camera::Camera, renderer::CapturedFrame};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export async function downloadWebp(name, rgba, width, height, cancellation) {
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('2D canvas is unavailable');
    context.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
    const blob = await new Promise((resolve, reject) => {
        canvas.toBlob(value => value ? resolve(value) : reject(new Error('WebP encoding failed')), 'image/webp', 0.9);
    });
    if (blob.type !== 'image/webp') throw new Error('This browser cannot encode WebP images');
    if (cancellation.cancelled) return;
    const url = URL.createObjectURL(blob);
    try {
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = name;
        anchor.style.display = 'none';
        document.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
    } finally {
        setTimeout(() => URL.revokeObjectURL(url), 60000);
    }
}

export function createH264(width, height, fps) {
    if (!globalThis.VideoEncoder) throw new Error('This browser does not support video encoding');
    const blocks = Math.ceil(width / 16) * Math.ceil(height / 16);
    const blocksPerSecond = blocks * fps;
    let codec;
    if (blocks <= 3600 && blocksPerSecond <= 108000) codec = 'avc1.42E01F';
    else if (blocks <= 8192 && blocksPerSecond <= 245760) codec = 'avc1.42E028';
    else if (blocks <= 8704 && blocksPerSecond <= 522240) codec = 'avc1.42E02A';
    else if (blocks <= 36864 && blocksPerSecond <= 983040) codec = 'avc1.42E033';
    else throw new Error('Video dimensions or frame rate exceed H.264 limits');
    const session = {encoded: [], description: undefined, failure: undefined};
    const encoder = new VideoEncoder({
        output(chunk, metadata) {
            if (metadata?.decoderConfig?.description) {
                session.description = new Uint8Array(metadata.decoderConfig.description);
            }
            const data = new Uint8Array(chunk.byteLength);
            chunk.copyTo(data);
            session.encoded.push({data, key: chunk.type === 'key', timestamp: chunk.timestamp});
        },
        error(error) { session.failure = error; }
    });
    encoder.configure({codec, width, height, bitrate: 5000000,
        framerate: fps, avc: {format: 'avc'}});
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d', {alpha: false});
    if (!context) { encoder.close(); throw new Error('2D canvas is unavailable'); }
    return Object.assign(session, {encoder, canvas, context, width, height, fps, inputCount: 0});
}

export function pushH264(session, rgba) {
    if (session.failure) throw session.failure;
    session.context.putImageData(new ImageData(new Uint8ClampedArray(rgba), session.width, session.height), 0, 0);
    const frame = new VideoFrame(session.canvas, {
        timestamp: Math.round(session.inputCount * 1000000 / session.fps)
    });
    try {
        session.encoder.encode(frame, {keyFrame: session.inputCount === 0});
    } finally {
        frame.close();
    }
    session.inputCount++;
}

export async function finishH264(session) {
    try {
        await session.encoder.flush();
        if (session.failure) throw session.failure;
        if (!session.description || session.encoded.length !== session.inputCount)
            throw new Error('H.264 encoder returned incomplete video');
        return {description: session.description, encoded: session.encoded};
    } finally {
        if (session.encoder.state !== 'closed') session.encoder.close();
    }
}

export function closeH264(session) {
    if (session.encoder.state !== 'closed') session.encoder.close();
}

export async function yieldToBrowser() {
    await new Promise(resolve => setTimeout(resolve, 0));
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = downloadWebp)]
    async fn download_webp_js(
        name: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
        cancellation: &JsValue,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = createH264)]
    fn create_h264_js(width: u32, height: u32, fps: f32) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pushH264)]
    fn push_h264_js(session: &JsValue, rgba: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = finishH264)]
    async fn finish_h264_js(session: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = closeH264)]
    fn close_h264_js(session: &JsValue);
    #[wasm_bindgen(js_name = yieldToBrowser)]
    async fn yield_to_browser();
}

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

#[derive(Clone)]
pub struct WebCancel {
    state: JsValue,
    video_session: Option<JsValue>,
}

impl WebCancel {
    pub fn image() -> Self {
        Self {
            state: js_sys::Object::new().into(),
            video_session: None,
        }
    }

    pub fn video(video: &WebVideo) -> Self {
        Self {
            state: js_sys::Object::new().into(),
            video_session: Some(video.session.clone()),
        }
    }

    pub fn cancel(&self) {
        let _ = js_sys::Reflect::set(&self.state, &JsValue::from_str("cancelled"), &JsValue::TRUE);
        if let Some(session) = &self.video_session {
            close_h264_js(session);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        js_sys::Reflect::get(&self.state, &JsValue::from_str("cancelled"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }
}

pub async fn download_webp(
    name: &str,
    frame: &CapturedFrame,
    cancel: &WebCancel,
) -> Result<(), String> {
    download_webp_js(name, &frame.rgba, frame.width, frame.height, &cancel.state)
        .await
        .map_err(|error| format!("{error:?}"))
}

pub struct WebVideo {
    session: JsValue,
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
}

impl Drop for WebVideo {
    fn drop(&mut self) {
        close_h264_js(&self.session);
    }
}

impl WebVideo {
    pub fn new(frame: &CapturedFrame, fps: f32) -> Result<Self, String> {
        let width = frame.width + frame.width % 2;
        let height = frame.height + frame.height % 2;
        let session = create_h264_js(width, height, fps).map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            session,
            source_width: frame.width,
            source_height: frame.height,
            width,
            height,
        })
    }

    pub fn push(&mut self, frame: &CapturedFrame) -> Result<(), String> {
        if frame.width != self.source_width || frame.height != self.source_height {
            return Err("the viewport changed size during video export".into());
        }
        let mut pixels = vec![0; (self.width * self.height * 4) as usize];
        for row in 0..frame.height {
            let source = (row * frame.width * 4) as usize;
            let target = (row * self.width * 4) as usize;
            pixels[target..target + (frame.width * 4) as usize]
                .copy_from_slice(&frame.rgba[source..source + (frame.width * 4) as usize]);
        }
        push_h264_js(&self.session, &pixels).map_err(|error| format!("{error:?}"))
    }

    pub async fn finish(self, name: &str, fps: f32, cancel: &WebCancel) -> Result<(), String> {
        let output = finish_h264_js(&self.session)
            .await
            .map_err(|error| format!("{error:?}"))?;
        if cancel.is_cancelled() {
            return Ok(());
        }
        write_mp4_download(name, output, self.width, self.height, fps, cancel).await
    }
}

async fn write_mp4_download(
    name: &str,
    output: JsValue,
    width: u32,
    height: u32,
    fps: f32,
    cancel: &WebCancel,
) -> Result<(), String> {
    let width16 = u16::try_from(width).map_err(|_| "video is too wide")?;
    let height16 = u16::try_from(height).map_err(|_| "video is too tall")?;
    let field = |name| {
        js_sys::Reflect::get(&output, &JsValue::from_str(name))
            .map_err(|error| format!("{error:?}"))
    };
    let description = js_sys::Uint8Array::new(&field("description")?).to_vec();
    let (sps, pps) = avc_parameters(&description)?;
    let samples = js_sys::Array::from(&field("encoded")?);
    let timescale = 90_000_u32;
    let duration = ((timescale as f32 / fps).round() as u32).max(1);
    let config = mp4::Mp4Config {
        major_brand: "isom".parse().map_err(|error| format!("{error}"))?,
        minor_version: 512,
        compatible_brands: ["isom", "iso2", "avc1", "mp41"]
            .into_iter()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("{error}"))?,
        timescale,
    };
    let mut writer = mp4::Mp4Writer::write_start(std::io::Cursor::new(Vec::new()), &config)
        .map_err(|error| format!("could not begin MP4: {error}"))?;
    writer
        .add_track(&mp4::TrackConfig {
            track_type: mp4::TrackType::Video,
            timescale,
            language: "und".to_owned(),
            media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
                width: width16,
                height: height16,
                seq_param_set: sps,
                pic_param_set: pps,
            }),
        })
        .map_err(|error| format!("could not add video track: {error}"))?;
    for (index, sample) in samples.iter().enumerate() {
        if index % 8 == 0 {
            yield_to_browser().await;
            if cancel.is_cancelled() {
                return Ok(());
            }
        }
        let data = js_sys::Uint8Array::new(
            &js_sys::Reflect::get(&sample, &JsValue::from_str("data"))
                .map_err(|error| format!("{error:?}"))?,
        )
        .to_vec();
        let key = js_sys::Reflect::get(&sample, &JsValue::from_str("key"))
            .map_err(|error| format!("{error:?}"))?
            .as_bool()
            .unwrap_or(false);
        let timestamp = js_sys::Reflect::get(&sample, &JsValue::from_str("timestamp"))
            .map_err(|error| format!("{error:?}"))?
            .as_f64()
            .ok_or("the video encoder returned an invalid timestamp")?;
        let presentation_time = (timestamp * timescale as f64 / 1_000_000.0).round() as i64;
        let decode_time = index as i64 * duration as i64;
        let rendering_offset = i32::try_from(presentation_time - decode_time)
            .map_err(|_| "the video composition offset is too large")?;
        writer
            .write_sample(
                1,
                &mp4::Mp4Sample {
                    start_time: index as u64 * duration as u64,
                    duration,
                    rendering_offset,
                    is_sync: key,
                    bytes: data.into(),
                },
            )
            .map_err(|error| format!("could not write video frame {index}: {error}"))?;
    }
    writer
        .write_end()
        .map_err(|error| format!("could not finish MP4: {error}"))?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    let bytes = writer.into_writer().into_inner();
    crate::document::download_bytes(name, &bytes)
}

fn avc_parameters(description: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if description.len() < 8
        || description[0] != 1
        || description[4] & 3 != 3
        || description[5] & 31 == 0
    {
        return Err("H.264 encoder returned invalid codec metadata".into());
    }
    let sps_length = u16::from_be_bytes([description[6], description[7]]) as usize;
    let pps_count = 8 + sps_length;
    if description.len() < pps_count + 3 || description[pps_count] == 0 {
        return Err("H.264 encoder returned incomplete codec metadata".into());
    }
    let pps_length =
        u16::from_be_bytes([description[pps_count + 1], description[pps_count + 2]]) as usize;
    if description.len() < pps_count + 3 + pps_length {
        return Err("H.264 encoder returned incomplete picture parameters".into());
    }
    Ok((
        description[8..pps_count].to_vec(),
        description[pps_count + 3..pps_count + 3 + pps_length].to_vec(),
    ))
}
