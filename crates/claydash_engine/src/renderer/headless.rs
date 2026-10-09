use super::*;

/// GPU and image dimensions for a renderer that never creates a window.
#[derive(Clone, Copy, Debug)]
pub struct HeadlessOptions {
    pub width: u32,
    pub height: u32,
    pub use_bvh: bool,
    /// Request a software adapter, when the selected wgpu backend provides one.
    pub force_fallback_adapter: bool,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            use_bvh: true,
            force_fallback_adapter: false,
        }
    }
}

impl HeadlessOptions {
    pub(super) fn validate(self) -> Result<(), String> {
        validate_headless_extent(
            self.width,
            self.height,
            &wgpu::Limits {
                max_texture_dimension_2d: u32::MAX,
                max_buffer_size: u64::MAX,
                ..Default::default()
            },
        )
    }
}

pub(super) fn validate_headless_extent(
    width: u32,
    height: u32,
    limits: &wgpu::Limits,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("render dimensions must be nonzero".into());
    }
    if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
        return Err("render dimensions exceed GPU texture limits".into());
    }
    let padded = u64::from(width) * 4;
    let bytes = padded
        .div_ceil(256)
        .checked_mul(256)
        .and_then(|row| row.checked_mul(u64::from(height)))
        .ok_or("render readback size overflows")?;
    // The shared capture code uses u32 row and allocation arithmetic.
    if bytes > limits.max_buffer_size || bytes > u64::from(u32::MAX) {
        return Err("render readback exceeds GPU buffer limits".into());
    }
    Ok(())
}

impl Renderer {
    /// Render a complete native-resolution image without editor UI.
    ///
    /// Native only: drives GPU progress synchronously, with a deadline for all
    /// refinement and readback. Call again to capture a changed camera/scene.
    /// Camera viewport size and origin are set to the renderer's image extent.
    /// Existing background group baking jobs can continue asynchronously.
    /// CPU shader compilation cannot be interrupted; GPU waits use the remaining deadline.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_offscreen(
        &mut self,
        camera: &Camera,
        objects: &[SdfObject],
        world: World,
        post_processing: &[crate::model::PostProcessPass],
        timeout: std::time::Duration,
    ) -> Result<CapturedFrame, String> {
        validate_headless_extent(self.config.width, self.config.height, &self.device.limits())?;
        if self.capture_pending {
            return Err("a capture is already pending".into());
        }
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .ok_or("capture timeout is too large")?;
        let mut camera = camera.clone();
        camera.viewport = self.size();
        camera.viewport_origin = Vec2::ZERO;
        let egui = egui::Context::default();
        // Explicit invalidation allows callers to mutate scene data directly.
        self.invalidate_scene();
        let result = (|| loop {
            let _remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or("offscreen capture timed out")?;
            if !self.capture_pending {
                egui.begin_pass(egui::RawInput::default());
                let mut output = egui.end_pass();
                self.render(
                    &camera,
                    objects,
                    &[],
                    [0, 0],
                    0,
                    world,
                    post_processing,
                    &egui,
                    &mut output,
                    true,
                    false,
                    true,
                    true,
                    false,
                    false,
                    false,
                    false,
                );
            }
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or("offscreen capture timed out")?;
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(remaining),
                })
                .map_err(|error| format!("offscreen GPU progress: {error}"))?;
            if let Some(frame) = self.take_capture() {
                return frame;
            }
        })();
        if result.is_err() {
            self.discard_pending_capture();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_dimensions_fail_before_gpu_allocation() {
        for (width, height) in [
            (0, 10),
            (10, 0),
            (u32::MAX, 1),
            (1, u32::MAX),
            (u32::MAX, u32::MAX),
        ] {
            assert!(HeadlessOptions {
                width,
                height,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(HeadlessOptions::default().validate().is_ok());
    }
    #[test]
    fn readback_respects_buffer_limit() {
        let limits = wgpu::Limits {
            max_buffer_size: 511,
            ..Default::default()
        };
        assert!(validate_headless_extent(1, 2, &limits).is_err());
        assert!(validate_headless_extent(1, 1, &limits).is_ok());
    }
}
