//! Small, camera-aware annotations burned into agent captures.
use crate::renderer::CapturedFrame;
use claydash_engine::camera::{Camera, ProjectionMode};
use glam::Vec3;

const WHITE: [u8; 3] = [244, 246, 249];
const PANEL: [u8; 3] = [19, 25, 34];

fn blend(frame: &mut CapturedFrame, x: i32, y: i32, color: [u8; 3], opacity: u8) {
    if x < 0 || y < 0 || x >= frame.width as i32 || y >= frame.height as i32 {
        return;
    }
    let at = ((y as u32 * frame.width + x as u32) * 4) as usize;
    let inverse = 255_u16 - opacity as u16;
    for channel in 0..3 {
        frame.rgba[at + channel] = ((frame.rgba[at + channel] as u16 * inverse
            + color[channel] as u16 * opacity as u16) / 255) as u8;
    }
    frame.rgba[at + 3] = 255;
}

fn rectangle(frame: &mut CapturedFrame, x: i32, y: i32, width: i32, height: i32,
    color: [u8; 3], opacity: u8) {
    for row in y..y + height {
        for column in x..x + width {
            blend(frame, column, row, color, opacity);
        }
    }
}

fn glyph(letter: char) -> [u8; 5] {
    match letter {
        'A' => [2, 5, 7, 5, 5], 'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3], 'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7], 'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3], 'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7], 'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5], 'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5], 'N' => [5, 7, 7, 7, 5],
        'O' => [2, 5, 5, 5, 2], 'P' => [6, 5, 6, 4, 4],
        'R' => [6, 5, 6, 5, 5], 'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2], 'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2], 'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5], 'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '0' => [7, 5, 5, 5, 7], '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7], '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1], '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7], '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7], '9' => [7, 5, 7, 1, 6],
        '.' => [0, 0, 0, 0, 2], '-' => [0, 0, 7, 0, 0],
        '+' => [0, 2, 7, 2, 0],
        _ => [0; 5],
    }
}

fn text_width(label: &str, scale: i32) -> i32 {
    label.chars().count() as i32 * 4 * scale - scale
}

fn write(frame: &mut CapturedFrame, label: &str, x: i32, y: i32, scale: i32,
    color: [u8; 3]) {
    for (index, letter) in label.chars().enumerate() {
        let pattern = glyph(letter);
        for (row, bits) in pattern.into_iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 {
                    continue;
                }
                rectangle(frame, x + index as i32 * 4 * scale + column * scale,
                    y + row as i32 * scale, scale, scale, color, 255);
            }
        }
    }
}

fn axis_color(axis: usize) -> [u8; 3] {
    match axis {
        0 => [244, 100, 93],
        1 => [117, 223, 136],
        _ => [117, 166, 252],
    }
}

fn axis_letter(axis: usize) -> char {
    match axis { 0 => 'X', 1 => 'Y', _ => 'Z' }
}

fn axis_label(direction: Vec3, excluded: Option<usize>) -> (usize, bool) {
    let components = [direction.x, direction.y, direction.z];
    let mut best = 0;
    let mut weight = -1.0_f32;
    for (axis, component) in components.into_iter().enumerate() {
        if Some(axis) != excluded && component.abs() > weight {
            best = axis;
            weight = component.abs();
        }
    }
    (best, components[best] >= 0.0)
}

pub(super) fn pixels_per_unit_at_target(camera: &Camera, height: u32) -> f32 {
    let distance = camera.position.distance(camera.target).max(0.0001);
    match camera.projection_mode {
        ProjectionMode::Perspective => height as f32
            / (2.0 * distance * (45.0_f32.to_radians() * 0.5).tan()),
        ProjectionMode::Orthographic => height as f32 / (0.84 * distance),
    }
}

fn scale_step(pixels_per_unit: f32, width: u32) -> (f32, i32) {
    let wanted = (width as f32 * 0.25).clamp(24.0, 96.0);
    let maximum = (width as f32 * 0.35).max(24.0);
    let mut selected = (1.0_f32, pixels_per_unit.round() as i32);
    let mut best = f32::INFINITY;
    for exponent in -6..=6 {
        let decade = 10_f32.powi(exponent);
        for multiple in [1.0, 2.0, 5.0] {
            let units = multiple * decade;
            let pixels = units * pixels_per_unit;
            if (12.0..=maximum).contains(&pixels) {
                let difference = (pixels - wanted).abs();
                if difference < best {
                    best = difference;
                    selected = (units, pixels.round() as i32);
                }
            }
        }
    }
    selected
}

pub(super) fn annotate(frame: &mut CapturedFrame, camera: &Camera, title: &str) {
    if frame.width < 80 || frame.height < 80 {
        return;
    }
    let width = frame.width as i32;
    let height = frame.height as i32;
    let compact = frame.width < 180;
    let type_scale = if compact { 1 } else { 2 };
    let title = title.to_ascii_uppercase();
    let title_width = text_width(&title, type_scale);
    let title_x = (width - title_width) / 2;
    rectangle(frame, title_x - 9, 8, title_width + 18, 5 * type_scale + 12, PANEL, 180);
    write(frame, &title, title_x, 14, type_scale, WHITE);

    let forward = (camera.target - camera.position).normalize_or_zero();
    let right = forward.cross(camera.up).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    let (horizontal_axis, horizontal_positive) = axis_label(right, None);
    let (vertical_axis, vertical_positive) = axis_label(up, Some(horizontal_axis));
    let marker = (width / 10).clamp(13, 28);
    let left = if compact { 8 } else { 15 };
    let bottom = height - if compact { 11 } else { 17 };
    if !compact {
        rectangle(frame, 6, bottom - marker - 17, marker + 39, marker + 30, PANEL, 155);
    }
    rectangle(frame, left, bottom - marker, 3, marker + 2,
        axis_color(vertical_axis), 255);
    rectangle(frame, left, bottom, marker + 2, 3,
        axis_color(horizontal_axis), 255);
    let horizontal_label = format!("{}{}", if horizontal_positive { '+' } else { '-' },
        axis_letter(horizontal_axis));
    let vertical_label = format!("{}{}", if vertical_positive { '+' } else { '-' },
        axis_letter(vertical_axis));
    write(frame, &horizontal_label, left + marker + 5, bottom - 3, 1,
        axis_color(horizontal_axis));
    write(frame, &vertical_label, left - 2, bottom - marker - 10, 1,
        axis_color(vertical_axis));

    let (units, bar_length) = scale_step(pixels_per_unit_at_target(camera, frame.height), frame.width);
    let bar_right = width - if compact { 8 } else { 15 };
    let bar_left = bar_right - bar_length;
    let amount = if units >= 1.0 {
        (units as i32).to_string()
    } else {
        units.to_string()
    };
    let label = format!("{} {}", amount, if units == 1.0 { "UNIT" } else { "UNITS" });
    let label_width = text_width(&label, 1);
    if !compact {
        rectangle(frame, bar_left - 14, bottom - 27,
            bar_length + 28, 42, PANEL, 155);
    }
    write(frame, &label, bar_left + (bar_length - label_width) / 2,
        bottom - 19, 1, WHITE);
    rectangle(frame, bar_left, bottom, bar_length, 2, WHITE, 255);
    rectangle(frame, bar_left - 1, bottom - 5, 3, 12, WHITE, 255);
    rectangle(frame, bar_right - 1, bottom - 5, 3, 12, WHITE, 255);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_bar_projects_one_world_unit_at_target_depth() {
        let mut camera = Camera::new();
        camera.position = Vec3::new(0.0, 0.0, 10.0);
        camera.target = Vec3::ZERO;
        camera.projection_mode = ProjectionMode::Perspective;
        let expected = 200.0 / (20.0 * (22.5_f32.to_radians()).tan());
        assert!((pixels_per_unit_at_target(&camera, 200) - expected).abs() < 0.001);
        camera.projection_mode = ProjectionMode::Orthographic;
        assert!((pixels_per_unit_at_target(&camera, 200) - 200.0 / 8.4).abs() < 0.001);
    }
}
