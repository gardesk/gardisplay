//! Simple text input widget.

use gartk_core::{InputEvent, Key, MouseButton, Point, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// A simple text input field.
pub struct TextInput {
    rect: Rect,
    text: String,
    cursor: usize,
    active: bool,
    placeholder: String,
}

impl TextInput {
    /// Create a new text input.
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            text: String::new(),
            cursor: 0,
            active: false,
            placeholder: String::new(),
        }
    }

    /// Set placeholder text.
    pub fn set_placeholder(&mut self, placeholder: &str) {
        self.placeholder = placeholder.to_string();
    }

    /// Set position.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
    }

    /// Set active state.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if active {
            self.cursor = self.text.len();
        }
    }

    /// Check if active.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set text.
    #[allow(dead_code)]
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
    }

    /// Clear text.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Check if point is inside.
    fn contains(&self, point: Point) -> bool {
        self.rect.contains_point(point)
    }

    /// Handle input event.
    /// Returns `Some(true)` on Enter (submit), `Some(false)` on Escape (cancel), None otherwise.
    pub fn handle_event(&mut self, event: &InputEvent) -> Option<bool> {
        match event {
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                if self.contains(e.position) {
                    self.active = true;
                    // TODO: position cursor based on click
                    self.cursor = self.text.len();
                } else {
                    self.active = false;
                }
                None
            }
            InputEvent::Key(e) if e.pressed && self.active => match e.key {
                Key::Escape => {
                    self.active = false;
                    Some(false)
                }
                Key::Return => {
                    self.active = false;
                    Some(true)
                }
                Key::Backspace => {
                    if self.cursor > 0 {
                        self.text.remove(self.cursor - 1);
                        self.cursor -= 1;
                    }
                    None
                }
                Key::Delete => {
                    if self.cursor < self.text.len() {
                        self.text.remove(self.cursor);
                    }
                    None
                }
                Key::Left => {
                    self.cursor = self.cursor.saturating_sub(1);
                    None
                }
                Key::Right => {
                    self.cursor = (self.cursor + 1).min(self.text.len());
                    None
                }
                Key::Home => {
                    self.cursor = 0;
                    None
                }
                Key::End => {
                    self.cursor = self.text.len();
                    None
                }
                Key::Char(c) => {
                    self.text.insert(self.cursor, c);
                    self.cursor += 1;
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Render the text input.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Background
        let bg = if self.active {
            theme.input_background
        } else {
            theme.item_background
        };
        renderer.fill_rounded_rect(self.rect, 6.0, bg)?;

        // Border
        let border = if self.active {
            theme.selection_background
        } else {
            theme.border
        };
        renderer.stroke_rounded_rect(self.rect, 6.0, border, 1.0)?;

        // Text or placeholder
        let (display_text, text_color) = if self.text.is_empty() && !self.active {
            (self.placeholder.as_str(), theme.input_placeholder)
        } else {
            (self.text.as_str(), theme.input_foreground)
        };

        let style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(text_color)
            .align(TextAlign::Left);

        let text_rect = Rect::new(
            self.rect.x + 8,
            self.rect.y,
            self.rect.width - 16,
            self.rect.height,
        );
        renderer.text_in_rect(display_text, text_rect, &style)?;

        // Cursor (when active)
        if self.active {
            // Rough cursor positioning (7px per char estimate)
            let cursor_x = self.rect.x as f64 + 8.0 + (self.cursor as f64 * 7.0);
            let cursor_y = self.rect.y as f64 + 6.0;
            let cursor_height = self.rect.height - 12;
            renderer.fill_rect(
                Rect::new(cursor_x as i32, cursor_y as i32, 2, cursor_height),
                theme.input_cursor,
            )?;
        }

        Ok(())
    }
}
