//! Simple button widget.

use gartk_core::{InputEvent, MouseButton, Point, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// A clickable button.
pub struct Button {
    rect: Rect,
    label: String,
    hovered: bool,
    pressed: bool,
    enabled: bool,
}

impl Button {
    /// Create a new button.
    pub fn new(x: i32, y: i32, width: u32, height: u32, label: &str) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            label: label.to_string(),
            hovered: false,
            pressed: false,
            enabled: true,
        }
    }

    /// Set button position.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
    }

    /// Set enabled state.
    #[allow(dead_code)]
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if a point is inside the button.
    fn contains(&self, point: Point) -> bool {
        self.rect.contains_point(point)
    }

    /// Handle input event. Returns true if button was clicked.
    pub fn handle_event(&mut self, event: &InputEvent) -> bool {
        if !self.enabled {
            return false;
        }

        match event {
            InputEvent::MouseMove(e) => {
                self.hovered = self.contains(e.position);
                false
            }
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                if self.contains(e.position) {
                    self.pressed = true;
                }
                false
            }
            InputEvent::MouseRelease(e) if e.button == Some(MouseButton::Left) => {
                let clicked = self.pressed && self.contains(e.position);
                self.pressed = false;
                clicked
            }
            _ => false,
        }
    }

    /// Render the button.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        let (bg_color, text_color) = if !self.enabled {
            (theme.item_background, theme.item_description)
        } else if self.pressed {
            (theme.selection_background, theme.foreground)
        } else if self.hovered {
            (theme.item_hover_background, theme.foreground)
        } else {
            (theme.item_background, theme.foreground)
        };

        // Background
        renderer.fill_rounded_rect(self.rect, 6.0, bg_color)?;

        // Border
        let border_color = if self.hovered && self.enabled {
            theme.selection_background
        } else {
            theme.border
        };
        renderer.stroke_rounded_rect(self.rect, 6.0, border_color, 1.0)?;

        // Label (centered using text_in_rect)
        let style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(text_color)
            .align(TextAlign::Center);
        renderer.text_in_rect(&self.label, self.rect, &style)?;

        Ok(())
    }
}
