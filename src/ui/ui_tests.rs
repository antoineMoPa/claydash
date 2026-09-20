#[cfg(test)]
mod tests {
    use super::*;

    include!("ui_tests/animation_and_inspector.rs");
    include!("ui_tests/scene_and_chrome.rs");
    include!("ui_tests/primitive_gizmos.rs");
}
