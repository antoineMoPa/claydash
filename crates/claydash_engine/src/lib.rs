//! Reusable SDF scene geometry, CPU queries, GPU rendering, Gaussian splats and meshes.
//! Editor commands and observable application state live in the claydash application.
pub mod camera;
pub mod gaussian_ply;
pub mod gaussian_surface;
pub mod model;
pub mod renderer;
pub mod scene_language;
pub mod viewport;
pub mod wireframe;
pub use camera::{Camera, ProjectionMode, SceneCamera};
pub use model::{SdfObject, World};
pub use renderer::Renderer;
#[cfg(test)]
mod test_support;

pub mod seam_guides;
