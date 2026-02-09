//! Toggle switch widget.

use gartk_core::{Color, InputEvent, MouseButton, Point, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// A toggle switch (on/off).
pub struct Toggle {
    rect: Rect,
    label: String,
    value: bool,
    hovered: bool,
}

impl Toggle {
    /// Create a new toggle.
    pub fn new(x: i32, y: i32, width: u32, height: u32, label: &str) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            label: label.to_string(),
            value: true,
            hovered: false,
        }
    }

    /// Set toggle position.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
    }

    /// Get current value.
    pub fn value(&self) -> bool {
        self.value
    }

    /// Set toggle value.
    pub fn set_value(&mut self, value: bool) {
        self.value = value;
    }

    /// Check if a point is inside the toggle.
    fn contains(&self, point: Point) -> bool {
        self.rect.contains_point(point)
    }

    /// Handle input event. Returns true if toggled.
    pub fn handle_event(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::MouseMove(e) => {
                self.hovered = self.contains(e.position);
                false
            }
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                if self.contains(e.position) {
                    self.value = !self.value;
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Render the toggle.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Layout: [Label] [==O] or [Label] [O==]
        let track_width = 40u32;
        let track_height = 20u32;
        let knob_radius = 8.0;

        // Label on the left
        let label_rect = Rect::new(
            self.rect.x,
            self.rect.y,
            self.rect.width.saturating_sub(track_width + 8),
            self.rect.height,
        );
        let style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.foreground)
            .align(TextAlign::Left);
        renderer.text_in_rect(&self.label, label_rect, &style)?;

        // Track on the right
        let track_x = self.rect.x + self.rect.width as i32 - track_width as i32;
        let track_y = self.rect.y + (self.rect.height as i32 - track_height as i32) / 2;
        let track_rect = Rect::new(track_x, track_y, track_width, track_height);

        // Track color based on value
        let track_color = if self.value {
            Color::new(0.2, 0.7, 0.4, 1.0) // Green when on
        } else {
            theme.item_background
        };

        renderer.fill_rounded_rect(track_rect, (track_height / 2) as f64, track_color)?;

        // Border
        let border_color = if self.hovered {
            theme.selection_background
        } else {
            theme.border
        };
        renderer.stroke_rounded_rect(track_rect, (track_height / 2) as f64, border_color, 1.0)?;

        // Knob
        let knob_x = if self.value {
            track_x as f64 + track_width as f64 - knob_radius - 4.0
        } else {
            track_x as f64 + knob_radius + 4.0
        };
        let knob_y = track_y as f64 + track_height as f64 / 2.0;

        renderer.fill_circle(knob_x, knob_y, knob_radius, theme.foreground)?;

        Ok(())
    }
}
