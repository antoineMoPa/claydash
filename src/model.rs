//! Editor state layered over the reusable scene and geometry model.
pub use claydash_engine::model::*;
mod state;
pub use state::*;
#[cfg(test)]
mod tests;
