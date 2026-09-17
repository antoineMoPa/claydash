//! A compact, on-demand command palette following Moon's interaction model.

use egui::{vec2, Align2, Color32, CornerRadius, Key, RichText, Stroke, StrokeKind};

const ROW_HEIGHT: f32 = 38.0;
const ROWS_HEIGHT_OF_SCREEN: f32 = 0.55;

#[derive(Clone, Debug)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub description: String,
    pub shortcut: String,
}

#[derive(Clone, Debug)]
pub struct PaletteStyle {
    pub panel: Color32,
    pub line: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub muted: Color32,
}

impl Default for PaletteStyle {
    fn default() -> Self {
        Self {
            panel: Color32::from_rgb(31, 32, 35),
            line: Color32::from_gray(67),
            selected: Color32::from_rgb(48, 51, 57),
            accent: Color32::from_rgb(104, 151, 255),
            text: Color32::WHITE,
            muted: Color32::GRAY,
        }
    }
}

pub struct CommandPalette {
    open: bool,
    query: String,
    highlighted: usize,
    highlight_query: String,
    rect: Option<egui::Rect>,
    style: PaletteStyle,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            highlighted: 0,
            highlight_query: String::new(),
            rect: None,
            style: PaletteStyle::default(),
        }
    }
}

impl CommandPalette {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.highlighted = 0;
        self.highlight_query.clear();
        self.rect = None;
    }

    pub fn dismiss(&mut self) {
        self.open = false;
        self.rect = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn rect(&self) -> Option<egui::Rect> {
        self.rect
    }

    pub fn style_mut(&mut self) -> &mut PaletteStyle {
        &mut self.style
    }

    pub fn show(&mut self, ctx: &egui::Context, commands: &[Command]) -> Option<String> {
        if !self.open {
            return None;
        }
        if self.pressed_outside(ctx) || ctx.input_mut(|input| input.key_pressed(Key::Escape)) {
            self.dismiss();
            return None;
        }

        let matches = matching_indices(commands, &self.query);
        let (move_down, move_up, accept) = ctx.input_mut(|input| {
            (
                input.key_pressed(Key::ArrowDown),
                input.key_pressed(Key::ArrowUp),
                input.key_pressed(Key::Enter),
            )
        });
        let retyped = self.highlight_query != self.query;
        if retyped {
            self.highlighted = 0;
            self.highlight_query.clone_from(&self.query);
        }
        if !matches.is_empty() {
            let last = matches.len() - 1;
            if move_down {
                self.highlighted = (self.highlighted + 1).min(last);
            }
            if move_up {
                self.highlighted = self.highlighted.saturating_sub(1);
            }
            self.highlighted = self.highlighted.min(last);
        }

        let mut chosen = accept
            .then_some(self.highlighted)
            .filter(|_| !matches.is_empty());
        let screen = ctx.viewport_rect();
        let area = egui::Area::new("command-palette".into())
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, screen.height() * 0.12))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.style.panel)
                    .stroke(Stroke::new(1.0, self.style.line))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(9))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(70),
                    })
                    .show(ui, |ui| {
                        ui.set_width(
                            (screen.width() * 0.5)
                                .clamp(360.0, 560.0)
                                .min((screen.width() - 36.0).max(120.0)),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("Execute a command…")
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(7, 5)),
                        )
                        .request_focus();
                        ui.add_space(6.0);
                        if matches.is_empty() {
                            ui.label(RichText::new("nothing matches").color(self.style.muted));
                            return;
                        }
                        let height = rows_height(matches.len(), ui.spacing().item_spacing.y)
                            .min(screen.height() * ROWS_HEIGHT_OF_SCREEN);
                        egui::ScrollArea::vertical()
                            .id_salt("command-palette-results")
                            .max_height(height)
                            .min_scrolled_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (row_index, command_index) in matches.iter().enumerate() {
                                    let highlighted = row_index == self.highlighted;
                                    let response = draw_row(
                                        ui,
                                        &commands[*command_index],
                                        highlighted,
                                        &self.style,
                                    );
                                    if highlighted && (move_down || move_up || retyped) {
                                        response.scroll_to_me(None);
                                    }
                                    if response.clicked() {
                                        chosen = Some(row_index);
                                    }
                                    if response.hovered() {
                                        self.highlighted = row_index;
                                    }
                                }
                            });
                    });
            });
        self.rect = Some(area.response.rect);

        let id = chosen
            .and_then(|row| matches.get(row))
            .map(|index| commands[*index].id.clone());
        if id.is_some() {
            self.dismiss();
        }
        id
    }

    fn pressed_outside(&self, ctx: &egui::Context) -> bool {
        let Some(rect) = self.rect else {
            return false;
        };
        ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|point| !rect.contains(point))
        })
    }
}

fn matching_indices(commands: &[Command], query: &str) -> Vec<usize> {
    let terms: Vec<String> = query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    commands
        .iter()
        .enumerate()
        .filter(|(_, command)| {
            let searchable = format!("{} {}", command.title, command.description).to_lowercase();
            terms.iter().all(|term| searchable.contains(term))
        })
        .map(|(index, _)| index)
        .collect()
}

fn rows_height(rows: usize, gap: f32) -> f32 {
    rows as f32 * ROW_HEIGHT + rows.saturating_sub(1) as f32 * gap
}

fn draw_row(
    ui: &mut egui::Ui,
    command: &Command,
    highlighted: bool,
    style: &PaletteStyle,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if highlighted {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(5), style.selected);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(5),
                Stroke::new(1.0, style.accent),
                StrokeKind::Inside,
            );
        }
        let text_right = if command.shortcut.is_empty() {
            rect.right() - 9.0
        } else {
            rect.right() - 100.0
        };
        let text_painter = ui.painter().with_clip_rect(
            egui::Rect::from_min_max(rect.min, egui::pos2(text_right, rect.bottom()))
                .intersect(ui.clip_rect()),
        );
        text_painter.text(
            rect.min + vec2(9.0, 5.0),
            Align2::LEFT_TOP,
            &command.title,
            egui::FontId::proportional(13.0),
            style.text,
        );
        text_painter.text(
            rect.min + vec2(9.0, 21.0),
            Align2::LEFT_TOP,
            &command.description,
            egui::FontId::proportional(10.0),
            style.muted,
        );
        if !command.shortcut.is_empty() {
            ui.painter().text(
                egui::pos2(rect.max.x - 9.0, rect.center().y),
                Align2::RIGHT_CENTER,
                &command.shortcut,
                egui::FontId::proportional(10.0),
                style.muted,
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_term_must_match_title_or_description() {
        let commands = vec![Command {
            id: "sphere".into(),
            title: "Add Sphere".into(),
            description: "Create a round primitive".into(),
            shortcut: String::new(),
        }];
        assert_eq!(matching_indices(&commands, "sphere round"), vec![0]);
        assert!(matching_indices(&commands, "sphere cube").is_empty());
    }
}
