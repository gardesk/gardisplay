//! Dropdown widget with profile management (select, rename, delete).

use gartk_core::{InputEvent, Key, MouseButton, Point, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// Action returned by dropdown on user interaction.
#[derive(Debug, Clone)]
pub enum DropdownAction {
    /// Profile at index selected.
    Select(usize),
    /// Rename profile at index to new name.
    Rename(usize, String),
    /// Delete profile at index.
    Delete(usize),
}

/// A dropdown for selecting and managing profiles.
pub struct Dropdown {
    /// Button rect (collapsed state).
    rect: Rect,
    /// Width for expanded dropdown (may be wider than button).
    expanded_width: u32,
    /// Item height.
    item_height: u32,
    /// Profile names.
    items: Vec<String>,
    /// Currently selected index.
    selected: usize,
    /// Whether dropdown is expanded.
    expanded: bool,
    /// Hovered item index.
    hovered_item: Option<usize>,
    /// Hovered action button: (item_index, "rename" | "delete").
    hovered_action: Option<(usize, Action)>,
    /// Currently renaming: (index, current_text, cursor_pos).
    renaming: Option<(usize, String, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Rename,
    Delete,
}

impl Dropdown {
    /// Create a new dropdown.
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            expanded_width: width.max(180),
            item_height: height,
            items: Vec::new(),
            selected: 0,
            expanded: false,
            hovered_item: None,
            hovered_action: None,
            renaming: None,
        }
    }

    /// Set the list of items.
    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
    }

    /// Set selected index.
    #[allow(dead_code)]
    pub fn set_selected(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = index;
        }
    }

    /// Set selected by name.
    pub fn set_selected_by_name(&mut self, name: &str) {
        if let Some(idx) = self.items.iter().position(|s| s == name) {
            self.selected = idx;
        }
    }

    /// Get the selected item name.
    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(|s| s.as_str())
    }

    /// Check if expanded.
    #[allow(dead_code)]
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Set position.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
    }

    /// Get expanded dropdown rect.
    fn expanded_rect(&self) -> Rect {
        let list_height = (self.items.len() as u32 * self.item_height).max(self.item_height);
        // Expand upward from button
        Rect::new(
            self.rect.x,
            self.rect.y - list_height as i32,
            self.expanded_width,
            list_height,
        )
    }

    /// Get rect for an item at index.
    fn item_rect(&self, index: usize) -> Rect {
        let exp = self.expanded_rect();
        Rect::new(
            exp.x,
            exp.y + (index as i32 * self.item_height as i32),
            exp.width,
            self.item_height,
        )
    }

    /// Get rect for rename button in item.
    fn rename_button_rect(&self, item_rect: Rect) -> Rect {
        let size = self.item_height - 8;
        Rect::new(
            item_rect.x + item_rect.width as i32 - 2 * size as i32 - 16,
            item_rect.y + 4,
            size,
            size,
        )
    }

    /// Get rect for delete button in item.
    fn delete_button_rect(&self, item_rect: Rect) -> Rect {
        let size = self.item_height - 8;
        Rect::new(
            item_rect.x + item_rect.width as i32 - size as i32 - 8,
            item_rect.y + 4,
            size,
            size,
        )
    }

    /// Find item at point.
    fn item_at_point(&self, point: Point) -> Option<usize> {
        if !self.expanded {
            return None;
        }
        let exp = self.expanded_rect();
        if !exp.contains_point(point) {
            return None;
        }
        let relative_y = point.y - exp.y;
        let index = relative_y / self.item_height as i32;
        if index >= 0 && (index as usize) < self.items.len() {
            Some(index as usize)
        } else {
            None
        }
    }

    /// Handle input event. Returns action if user completed one.
    pub fn handle_event(&mut self, event: &InputEvent) -> Option<DropdownAction> {
        // Handle renaming mode first
        if let Some((idx, ref mut text, ref mut cursor)) = self.renaming {
            match event {
                InputEvent::Key(e) if e.pressed => match e.key {
                    Key::Escape => {
                        self.renaming = None;
                        return None;
                    }
                    Key::Return => {
                        let new_name = text.clone();
                        let index = idx;
                        self.renaming = None;
                        self.expanded = false;
                        return Some(DropdownAction::Rename(index, new_name));
                    }
                    Key::Backspace => {
                        if *cursor > 0 {
                            text.remove(*cursor - 1);
                            *cursor -= 1;
                        }
                        return None;
                    }
                    Key::Delete => {
                        if *cursor < text.len() {
                            text.remove(*cursor);
                        }
                        return None;
                    }
                    Key::Left => {
                        *cursor = cursor.saturating_sub(1);
                        return None;
                    }
                    Key::Right => {
                        *cursor = (*cursor + 1).min(text.len());
                        return None;
                    }
                    Key::Char(c) => {
                        text.insert(*cursor, c);
                        *cursor += 1;
                        return None;
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }

        match event {
            InputEvent::MouseMove(e) => {
                self.hovered_item = self.item_at_point(e.position);
                self.hovered_action = None;

                // Check action buttons if hovering an item
                if let Some(idx) = self.hovered_item {
                    let item_rect = self.item_rect(idx);
                    let rename_rect = self.rename_button_rect(item_rect);
                    let delete_rect = self.delete_button_rect(item_rect);

                    if rename_rect.contains_point(e.position) {
                        self.hovered_action = Some((idx, Action::Rename));
                    } else if delete_rect.contains_point(e.position) {
                        self.hovered_action = Some((idx, Action::Delete));
                    }
                }
                None
            }
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                // Check if clicking the main button
                if self.rect.contains_point(e.position) {
                    self.expanded = !self.expanded;
                    return None;
                }

                // Check if clicking in expanded area
                if self.expanded {
                    if let Some(idx) = self.item_at_point(e.position) {
                        let item_rect = self.item_rect(idx);
                        let rename_rect = self.rename_button_rect(item_rect);
                        let delete_rect = self.delete_button_rect(item_rect);

                        if rename_rect.contains_point(e.position) {
                            // Start renaming
                            let current_name = self.items[idx].clone();
                            self.renaming = Some((idx, current_name.clone(), current_name.len()));
                            return None;
                        } else if delete_rect.contains_point(e.position) {
                            // Delete
                            self.expanded = false;
                            return Some(DropdownAction::Delete(idx));
                        } else {
                            // Select
                            self.selected = idx;
                            self.expanded = false;
                            return Some(DropdownAction::Select(idx));
                        }
                    } else {
                        // Clicked outside - close
                        self.expanded = false;
                    }
                }
                None
            }
            InputEvent::Key(e) if e.pressed && e.key == Key::Escape => {
                if self.expanded {
                    self.expanded = false;
                }
                None
            }
            _ => None,
        }
    }

    /// Render the dropdown.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Draw main button
        let bg = if self.expanded {
            theme.item_selected_background
        } else {
            theme.item_background
        };
        renderer.fill_rounded_rect(self.rect, 6.0, bg)?;
        renderer.stroke_rounded_rect(self.rect, 6.0, theme.border, 1.0)?;

        // Draw selected item text (left-aligned)
        let text = self.selected_item().unwrap_or("(none)");
        let text_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.foreground)
            .align(TextAlign::Left);
        let text_rect = Rect::new(
            self.rect.x + 8,
            self.rect.y,
            self.rect.width - 28, // Leave room for arrow
            self.rect.height,
        );
        renderer.text_in_rect(text, text_rect, &text_style)?;

        // Draw dropdown arrow (fixed position on right)
        let arrow_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.foreground)
            .align(TextAlign::Center);
        let arrow_rect = Rect::new(
            self.rect.x + self.rect.width as i32 - 20,
            self.rect.y,
            16,
            self.rect.height,
        );
        renderer.text_in_rect("\u{25BC}", arrow_rect, &arrow_style)?; // ▼

        // Draw expanded list
        if self.expanded {
            let exp = self.expanded_rect();

            // Background
            renderer.fill_rounded_rect(exp, 6.0, theme.background)?;
            renderer.stroke_rounded_rect(exp, 6.0, theme.border, 1.0)?;

            for (i, item) in self.items.iter().enumerate() {
                let item_rect = self.item_rect(i);
                let is_selected = i == self.selected;
                let is_hovered = self.hovered_item == Some(i);
                let is_renaming = self.renaming.as_ref().map(|(idx, _, _)| *idx) == Some(i);

                // Item background
                let item_bg = if is_renaming {
                    theme.input_background
                } else if is_selected {
                    theme.item_selected_background
                } else if is_hovered {
                    theme.item_hover_background
                } else {
                    theme.background
                };
                renderer.fill_rect(item_rect, item_bg)?;

                if is_renaming {
                    // Render text input for renaming
                    if let Some((_, ref text, cursor)) = self.renaming {
                        let input_style = TextStyle::new()
                            .font_family(&theme.font_family)
                            .font_size(theme.font_size)
                            .color(theme.input_foreground)
                            .align(TextAlign::Left);
                        let text_rect = Rect::new(
                            item_rect.x + 8,
                            item_rect.y,
                            item_rect.width - 16,
                            item_rect.height,
                        );
                        renderer.text_in_rect(text, text_rect, &input_style)?;

                        // Draw cursor
                        let cursor_x = item_rect.x as f64 + 8.0 + (cursor as f64 * 7.0);
                        let cursor_y = item_rect.y as f64 + 4.0;
                        renderer.fill_rect(
                            Rect::new(cursor_x as i32, cursor_y as i32, 2, self.item_height - 8),
                            theme.input_cursor,
                        )?;
                    }
                } else {
                    // Item text
                    let text_color = if is_selected {
                        theme.item_selected_foreground
                    } else {
                        theme.item_foreground
                    };
                    let item_style = TextStyle::new()
                        .font_family(&theme.font_family)
                        .font_size(theme.font_size)
                        .color(text_color)
                        .align(TextAlign::Left);
                    let text_rect = Rect::new(
                        item_rect.x + 8,
                        item_rect.y,
                        item_rect.width - 60, // Leave room for buttons
                        item_rect.height,
                    );
                    renderer.text_in_rect(item, text_rect, &item_style)?;

                    // Action buttons (only on hover)
                    if is_hovered {
                        let rename_rect = self.rename_button_rect(item_rect);
                        let delete_rect = self.delete_button_rect(item_rect);

                        // Rename button
                        let rename_hovered =
                            self.hovered_action == Some((i, Action::Rename));
                        let rename_bg = if rename_hovered {
                            theme.item_hover_background
                        } else {
                            theme.item_background
                        };
                        renderer.fill_rounded_rect(rename_rect, 4.0, rename_bg)?;
                        let rename_style = TextStyle::new()
                            .font_family(&theme.font_family)
                            .font_size(theme.font_size - 2.0)
                            .color(theme.item_description)
                            .align(TextAlign::Center);
                        renderer.text_in_rect("\u{270E}", rename_rect, &rename_style)?; // ✎

                        // Delete button
                        let delete_hovered =
                            self.hovered_action == Some((i, Action::Delete));
                        let delete_bg = if delete_hovered {
                            theme.item_hover_background
                        } else {
                            theme.item_background
                        };
                        renderer.fill_rounded_rect(delete_rect, 4.0, delete_bg)?;
                        let delete_style = TextStyle::new()
                            .font_family(&theme.font_family)
                            .font_size(theme.font_size - 2.0)
                            .color(theme.item_description)
                            .align(TextAlign::Center);
                        renderer.text_in_rect("\u{1F5D1}", delete_rect, &delete_style)?; // 🗑
                    }
                }
            }
        }

        Ok(())
    }
}
