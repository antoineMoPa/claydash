use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Result type for object generation
pub type GenerationResult<T> = Result<T, GenerationError>;

#[derive(Debug, Clone)]
pub enum GenerationError {
    NetworkError(String),
    ApiError(String),
    ParseError(String),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            GenerationError::ApiError(msg) => write!(f, "API error: {}", msg),
            GenerationError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

/// Request/Response types for Flux Dev API
#[derive(Serialize)]
struct FluxDevRequest {
    prompt: String,
    image_size: ImageSize,
    num_inference_steps: u32,
    guidance_scale: f32,
    num_images: u32,
    enable_safety_checker: bool,
}

#[derive(Serialize)]
struct ImageSize {
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
struct FluxDevResponse {
    images: Vec<GeneratedImage>,
}

#[derive(Deserialize)]
struct GeneratedImage {
    url: String,
}

/// Request/Response types for Trellis API
#[derive(Serialize)]
struct TrellisRequest {
    image_url: String,
    format: String,
}

#[derive(Deserialize, Debug)]
struct TrellisResponse {
    model_mesh: TrellisModelMesh,
}

#[derive(Deserialize, Debug)]
struct TrellisModelMesh {
    url: String,
    #[allow(dead_code)]
    content_type: String,
    #[allow(dead_code)]
    file_name: String,
    #[allow(dead_code)]
    file_size: usize,
}

/// Final result of object generation
#[derive(Debug, Clone)]
pub struct GeneratedObject {
    pub image_url: String,
    pub model_data: Vec<u8>,
    pub filename: String,
}

/// Main object generation service
#[derive(Clone)]
pub struct ObjectGenerationService {
    fal_key: String,
}

impl ObjectGenerationService {
    pub fn new(fal_key: String) -> Self {
        Self { fal_key }
    }

    /// Generate image using Flux Dev - WASM implementation
    #[cfg(target_arch = "wasm32")]
    async fn generate_image(&self, prompt: &str) -> GenerationResult<String> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, Response};

        let request_body = FluxDevRequest {
            prompt: prompt.to_string(),
            image_size: ImageSize {
                width: 1024,
                height: 1024,
            },
            num_inference_steps: 28,
            guidance_scale: 3.5,
            num_images: 1,
            enable_safety_checker: true,
        };

        let body = serde_json::to_string(&request_body)
            .map_err(|e| GenerationError::ParseError(e.to_string()))?;

        let window = web_sys::window()
            .ok_or_else(|| GenerationError::NetworkError("No window object".to_string()))?;

        let mut opts = RequestInit::new();
        opts.method("POST");
        opts.mode(RequestMode::Cors);
        opts.body(Some(&wasm_bindgen::JsValue::from_str(&body)));

        let request = Request::new_with_str_and_init(
            "https://fal.run/fal-ai/flux/dev",
            &opts,
        ).map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        request.headers().set("Content-Type", "application/json")
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;
        request.headers().set("Authorization", &format!("Key {}", self.fal_key))
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp: Response = resp_value.dyn_into()
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let json = JsFuture::from(resp.json()
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?)
            .await
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?;

        let json_string = js_sys::JSON::stringify(&json)
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?;
        let json_str: String = json_string.into();

        let response: FluxDevResponse = serde_json::from_str(&json_str)
            .map_err(|e| GenerationError::ParseError(e.to_string()))?;

        response.images.first()
            .map(|img| img.url.clone())
            .ok_or_else(|| GenerationError::ApiError("No image generated".to_string()))
    }

    /// Generate image using Flux Dev - Native implementation
    #[cfg(not(target_arch = "wasm32"))]
    async fn generate_image(&self, prompt: &str) -> GenerationResult<String> {
        let request_body = FluxDevRequest {
            prompt: prompt.to_string(),
            image_size: ImageSize {
                width: 1024,
                height: 1024,
            },
            num_inference_steps: 28,
            guidance_scale: 3.5,
            num_images: 1,
            enable_safety_checker: true,
        };

        let client = reqwest::Client::new();
        let response = client
            .post("https://fal.run/fal-ai/flux/dev")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Key {}", self.fal_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| GenerationError::NetworkError(e.to_string()))?;

        let flux_response: FluxDevResponse = response
            .json()
            .await
            .map_err(|e| GenerationError::ParseError(e.to_string()))?;

        flux_response.images.first()
            .map(|img| img.url.clone())
            .ok_or_else(|| GenerationError::ApiError("No image generated".to_string()))
    }

    /// Generate 3D model using Trellis - WASM implementation
    #[cfg(target_arch = "wasm32")]
    async fn generate_model(&self, image_url: &str) -> GenerationResult<Vec<u8>> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, Response};

        let request_body = TrellisRequest {
            image_url: image_url.to_string(),
            format: "glb".to_string(),
        };

        let body = serde_json::to_string(&request_body)
            .map_err(|e| GenerationError::ParseError(e.to_string()))?;

        let window = web_sys::window()
            .ok_or_else(|| GenerationError::NetworkError("No window object".to_string()))?;

        let mut opts = RequestInit::new();
        opts.method("POST");
        opts.mode(RequestMode::Cors);
        opts.body(Some(&wasm_bindgen::JsValue::from_str(&body)));

        let request = Request::new_with_str_and_init(
            "https://fal.run/fal-ai/trellis",
            &opts,
        ).map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        request.headers().set("Content-Type", "application/json")
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;
        request.headers().set("Authorization", &format!("Key {}", self.fal_key))
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp: Response = resp_value.dyn_into()
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let json = JsFuture::from(resp.json()
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?)
            .await
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?;

        let json_string = js_sys::JSON::stringify(&json)
            .map_err(|e| GenerationError::ParseError(format!("{:?}", e)))?;
        let json_str: String = json_string.into();

        let response: TrellisResponse = serde_json::from_str(&json_str)
            .map_err(|e| GenerationError::ParseError(format!("Trellis response parse error: {}. Response: {}", e, json_str)))?;

        // Extract model URL
        let model_url = response.model_mesh.url;

        // Download the GLB file
        self.download_file_wasm(&model_url).await
    }

    /// Generate 3D model using Trellis - Native implementation
    #[cfg(not(target_arch = "wasm32"))]
    async fn generate_model(&self, image_url: &str) -> GenerationResult<Vec<u8>> {
        let request_body = TrellisRequest {
            image_url: image_url.to_string(),
            format: "glb".to_string(),
        };

        let client = reqwest::Client::new();
        let response = client
            .post("https://fal.run/fal-ai/trellis")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Key {}", self.fal_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| GenerationError::NetworkError(e.to_string()))?;

        // Get response text first for better error messages
        let response_text = response.text().await
            .map_err(|e| GenerationError::NetworkError(e.to_string()))?;

        let trellis_response: TrellisResponse = serde_json::from_str(&response_text)
            .map_err(|e| GenerationError::ParseError(format!("Trellis response parse error: {}. Response: {}", e, response_text)))?;

        // Extract model URL
        let model_url = trellis_response.model_mesh.url;

        // Download the GLB file
        self.download_file_native(&model_url).await
    }

    /// Download file from URL - WASM implementation
    #[cfg(target_arch = "wasm32")]
    async fn download_file_wasm(&self, url: &str) -> GenerationResult<Vec<u8>> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, Response};

        let window = web_sys::window()
            .ok_or_else(|| GenerationError::NetworkError("No window object".to_string()))?;

        let mut opts = RequestInit::new();
        opts.method("GET");
        opts.mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts)
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let resp: Response = resp_value.dyn_into()
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let array_buffer = JsFuture::from(resp.array_buffer()
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?)
            .await
            .map_err(|e| GenerationError::NetworkError(format!("{:?}", e)))?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let mut bytes = vec![0; uint8_array.length() as usize];
        uint8_array.copy_to(&mut bytes);

        Ok(bytes)
    }

    /// Download file from URL - Native implementation
    #[cfg(not(target_arch = "wasm32"))]
    async fn download_file_native(&self, url: &str) -> GenerationResult<Vec<u8>> {
        let client = reqwest::Client::new();
        let bytes = client
            .get(url)
            .send()
            .await
            .map_err(|e| GenerationError::NetworkError(e.to_string()))?
            .bytes()
            .await
            .map_err(|e| GenerationError::NetworkError(e.to_string()))?;

        Ok(bytes.to_vec())
    }

    /// Complete pipeline: prompt -> image -> 3D model
    pub async fn generate_object(&self, prompt: &str) -> GenerationResult<GeneratedObject> {
        info!("🎨 Generating image from prompt: {}", prompt);
        eprintln!("🎨 Generating image from prompt: {}", prompt);

        let image_url = match self.generate_image(prompt).await {
            Ok(url) => {
                info!("✅ Image generated: {}", url);
                eprintln!("✅ Image generated: {}", url);
                url
            }
            Err(e) => {
                error!("❌ Image generation failed: {}", e);
                eprintln!("❌ Image generation failed: {}", e);
                return Err(e);
            }
        };

        info!("🎲 Generating 3D model from image...");
        eprintln!("🎲 Generating 3D model from image...");

        let model_data = match self.generate_model(&image_url).await {
            Ok(data) => {
                info!("✅ 3D model generated, size: {} bytes", data.len());
                eprintln!("✅ 3D model generated, size: {} bytes", data.len());
                data
            }
            Err(e) => {
                error!("❌ 3D model generation failed: {}", e);
                eprintln!("❌ 3D model generation failed: {}", e);
                return Err(e);
            }
        };

        // Generate filename from prompt
        let sanitized_prompt = prompt
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(30)
            .collect::<String>();

        #[cfg(target_arch = "wasm32")]
        let timestamp = js_sys::Date::now() as u64;

        #[cfg(not(target_arch = "wasm32"))]
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let filename = format!("{}_{}.glb", sanitized_prompt, timestamp);

        info!("✅ 3D model generated successfully: {}", filename);
        eprintln!("✅ 3D model generated successfully: {}", filename);

        Ok(GeneratedObject {
            image_url,
            model_data,
            filename,
        })
    }
}

/// Resource to hold the generation service
#[derive(Resource)]
pub struct ObjectGeneration {
    pub service: ObjectGenerationService,
}

/// Plugin for object generation
pub struct ObjectGenerationPlugin;

impl Plugin for ObjectGenerationPlugin {
    fn build(&self, app: &mut App) {
        // Get FAL_KEY from environment
        let fal_key = std::env::var("FAL_KEY")
            .unwrap_or_else(|_| "your_fal_ai_api_key_here".to_string());

        let service = ObjectGenerationService::new(fal_key);

        app.insert_resource(ObjectGeneration { service });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_full_object_generation_pipeline() {
        // Load .env file for test
        let _ = dotenvy::dotenv();

        let fal_key = std::env::var("FAL_KEY")
            .expect("FAL_KEY must be set in .env file for this test");

        let service = ObjectGenerationService::new(fal_key);

        // Create a Tokio runtime for the test
        let runtime = tokio::runtime::Runtime::new().unwrap();

        // Run the full generation pipeline
        let result = runtime.block_on(async {
            service.generate_object("a simple red cube").await
        });

        // Check the result
        match result {
            Ok(generated_object) => {
                println!("✅ Generation successful!");
                println!("   Image URL: {}", generated_object.image_url);
                println!("   Filename: {}", generated_object.filename);
                println!("   Model size: {} bytes", generated_object.model_data.len());

                // Verify we got actual data
                assert!(!generated_object.image_url.is_empty(), "Image URL should not be empty");
                assert!(!generated_object.filename.is_empty(), "Filename should not be empty");
                assert!(generated_object.model_data.len() > 0, "Model data should not be empty");

                // Verify it's a valid GLB file (starts with glTF magic number)
                let magic = &generated_object.model_data[0..4];
                assert_eq!(magic, b"glTF", "Model should be a valid GLB file");

                println!("✅ All assertions passed!");
            }
            Err(e) => {
                panic!("❌ Generation failed: {}", e);
            }
        }
    }
}
