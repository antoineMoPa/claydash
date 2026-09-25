use web_time::Instant;

use crate::model::{
    objects, scene_cameras, set_objects, set_objects_transient, AnimatableProperty,
    AnimationBinding, AnimationData, AnimationTrack, BezierHandle, ClaydashValue, DataTree,
    Keyframe, KeyframeInterpolation, Lattice, LatticeShapeKey,
};

mod editing;
mod evaluation;
mod runtime;

pub use editing::*;
pub use evaluation::*;
pub use runtime::*;

#[cfg(test)]
mod tests;
