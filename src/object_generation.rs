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

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_generate_save_and_load_scene() {
        use crate::claydash_data::ClaydashValue;
        use crate::bevy_sdf_object::SDFObject;
        use observable_key_value_tree::ObservableKVTree;

        println!("\n🧪 Testing generate, save, and load scene workflow");

        // Load .env file for test
        let _ = dotenvy::dotenv();

        let fal_key = std::env::var("FAL_KEY")
            .expect("FAL_KEY must be set in .env file for this test");

        let service = ObjectGenerationService::new(fal_key);

        // Create a Tokio runtime for the test
        let runtime = tokio::runtime::Runtime::new().unwrap();

        // Step 1: Generate an object
        println!("\n📝 Step 1: Generating 3D object...");
        let generated_object = runtime.block_on(async {
            service.generate_object("a small blue sphere").await
        }).expect("Generation should succeed");

        println!("✅ Generated: {}", generated_object.filename);

        // Step 2: Save the model to disk
        println!("\n💾 Step 2: Saving model to disk...");
        let models_dir = std::path::Path::new("assets/generated_models");
        std::fs::create_dir_all(models_dir).expect("Should create directory");
        let model_path = models_dir.join(&generated_object.filename);
        std::fs::write(&model_path, &generated_object.model_data)
            .expect("Should write model file");
        println!("✅ Saved to: {:?}", model_path);

        // Step 3: Create a scene with some SDF objects
        println!("\n🎨 Step 3: Creating scene with SDF objects...");
        let mut tree = ObservableKVTree::<ClaydashValue>::default();

        let mut sphere = SDFObject::create(1); // TYPE_SPHERE
        sphere.transform.translation = Vec3::new(0.0, 0.0, 0.0);
        sphere.color = Vec4::new(1.0, 0.0, 0.0, 1.0);

        let mut box_obj = SDFObject::create(2); // TYPE_BOX
        box_obj.transform.translation = Vec3::new(1.0, 0.0, 0.0);
        box_obj.color = Vec4::new(0.0, 1.0, 0.0, 1.0);

        let sdf_objects = vec![sphere, box_obj];

        tree.set_path("scene.sdf_objects", ClaydashValue::VecSDFObject(sdf_objects));
        tree.set_path("scene.selected_uuids", ClaydashValue::VecUuid(Vec::new()));

        println!("✅ Created scene with {} SDF objects", 2);

        // Step 4: Serialize the scene
        println!("\n📦 Step 4: Serializing scene...");
        let scene_tree = tree.get_tree("scene");
        let serialized = serde_json::to_vec(&scene_tree)
            .expect("Should serialize scene");

        println!("✅ Serialized to {} bytes", serialized.len());

        // Print the JSON for debugging
        let json_str = serde_json::to_string_pretty(&scene_tree)
            .expect("Should convert to JSON string");
        println!("\n📄 Serialized JSON:");
        println!("{}", json_str);

        // Step 5: Deserialize the scene
        println!("\n📬 Step 5: Deserializing scene...");
        let deserialized: ObservableKVTree<ClaydashValue> = serde_json::from_slice(&serialized)
            .expect("Should deserialize scene");

        println!("✅ Deserialized successfully");

        // Step 6: Verify the deserialized data
        println!("\n✔️  Step 6: Verifying deserialized data...");
        let loaded_objects = deserialized.get_path("sdf_objects");
        match loaded_objects {
            ClaydashValue::VecSDFObject(objects) => {
                assert_eq!(objects.len(), 2, "Should have 2 SDF objects");
                println!("✅ Loaded {} SDF objects", objects.len());

                // Verify first object
                assert_eq!(objects[0].transform.translation, Vec3::new(0.0, 0.0, 0.0));
                assert_eq!(objects[0].object_type, 1); // TYPE_SPHERE
                println!("✅ Object 1 verified (sphere at origin)");

                // Verify second object
                assert_eq!(objects[1].transform.translation, Vec3::new(1.0, 0.0, 0.0));
                assert_eq!(objects[1].object_type, 2); // TYPE_BOX
                println!("✅ Object 2 verified (box at x=1)");
            }
            _ => panic!("Expected VecSDFObject, got something else"),
        }

        // Step 7: Clean up
        println!("\n🧹 Step 7: Cleaning up...");
        std::fs::remove_file(&model_path).ok();
        println!("✅ Cleaned up test files");

        println!("\n🎉 All steps completed successfully!");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_save_and_load_scene_with_generated_model() {
        use crate::claydash_data::{ClaydashValue, SceneObject};
        use observable_key_value_tree::ObservableKVTree;

        println!("\n🧪 Testing save and load scene with generated model");

        // Step 1: Create a scene tree with a generated model
        println!("\n📝 Step 1: Creating scene with generated model...");
        let mut tree = ObservableKVTree::<ClaydashValue>::default();

        let generated_model = SceneObject::Model {
            uuid: uuid::Uuid::new_v4(),
            asset_path: "generated_models/test_model.glb".to_string(),
            transform: Transform::from_xyz(1.0, 2.0, 3.0),
            prompt: Some("a test cube".to_string()),
        };

        let mut empty_scene = ObservableKVTree::<ClaydashValue>::default();
        empty_scene.set_path("sdf_objects", ClaydashValue::VecSDFObject(Vec::new()));
        empty_scene.set_path("selected_uuids", ClaydashValue::VecUuid(Vec::new()));
        empty_scene.set_path("objects", ClaydashValue::VecSceneObject(vec![generated_model.clone()]));

        tree.set_tree("scene", empty_scene);

        println!("✅ Created scene with 1 generated model");

        // Step 2: Serialize the scene
        println!("\n📦 Step 2: Serializing scene...");
        let scene_tree = tree.get_tree("scene").expect("Scene should exist");
        let serialized = serde_json::to_vec(&scene_tree)
            .expect("Should serialize scene");

        println!("✅ Serialized to {} bytes", serialized.len());

        // Print the JSON for debugging
        let json_str = serde_json::to_string_pretty(&scene_tree)
            .expect("Should convert to JSON string");
        println!("\n📄 Serialized JSON:");
        println!("{}", json_str);

        // Step 3: Deserialize the scene
        println!("\n📬 Step 3: Deserializing scene...");
        let deserialized: ObservableKVTree<ClaydashValue> = serde_json::from_slice(&serialized)
            .expect("Should deserialize scene");

        println!("✅ Deserialized successfully");

        // Step 4: Verify the loaded objects
        println!("\n✔️  Step 4: Verifying loaded generated model...");
        let loaded_objects = deserialized.get_path("objects");
        match loaded_objects {
            ClaydashValue::VecSceneObject(objects) => {
                assert_eq!(objects.len(), 1, "Should have 1 scene object");
                println!("✅ Loaded {} scene object(s)", objects.len());

                // Verify it's a model
                match &objects[0] {
                    SceneObject::Model { uuid, asset_path, transform, prompt } => {
                        println!("✅ Object is a Model");
                        println!("   UUID: {}", uuid);
                        println!("   Asset path: {}", asset_path);
                        println!("   Transform: {:?}", transform);
                        println!("   Prompt: {:?}", prompt);

                        assert_eq!(asset_path, "generated_models/test_model.glb");
                        assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
                        assert_eq!(prompt.as_ref().map(|s| s.as_str()), Some("a test cube"));
                        println!("✅ All fields verified");
                    }
                    SceneObject::SDF(_) => {
                        panic!("Expected Model, got SDF");
                    }
                }
            }
            _ => panic!("Expected VecSceneObject, got something else"),
        }

        println!("\n🎉 Save/load test completed successfully!");
    }
}
