//! Monitor layout view - displays monitors as draggable rectangles.

use gartk_core::{Color, InputEvent, Point, Rect, Theme};
use gartk_render::Renderer;
use gartk_x11::Monitor;

use super::EventResult;

/// Visual representation of a monitor in the layout.
#[derive(Debug, Clone)]
pub struct MonitorState {
    /// Monitor info from X11.
    pub info: Monitor,
    /// Scaled rectangle for display.
    pub scaled_rect: Rect,
}

/// View showing all monitors as rectangles.
pub struct MonitorView {
    monitors: Vec<MonitorState>,
    selected: Option<usize>,
    hovered: Option<usize>,
    primary_name: Option<String>,
    view_rect: Rect,
    scale: f64,
    offset: Point,
}

impl MonitorView {
    /// Create a new monitor view.
    pub fn new(view_rect: Rect) -> Self {
        Self {
            monitors: Vec::new(),
            selected: None,
            hovered: None,
            primary_name: None,
            view_rect,
            scale: 1.0,
            offset: Point::new(0, 0),
        }
    }

    /// Update with detected monitors.
    pub fn set_monitors(&mut self, monitors: Vec<Monitor>) {
        // Find primary
        self.primary_name = monitors.iter().find(|m| m.primary).map(|m| m.name.clone());

        // Calculate scale to fit all monitors in view
        self.calculate_layout(&monitors);

        // Create monitor states
        self.monitors = monitors
            .into_iter()
            .map(|info| {
                let scaled_rect = self.scale_rect(&info.rect);
                MonitorState { info, scaled_rect }
            })
            .collect();

        tracing::debug!(
            "set {} monitors, scale={:.3}, offset=({}, {})",
            self.monitors.len(),
            self.scale,
            self.offset.x,
            self.offset.y
        );
    }

    /// Calculate scale and offset to fit monitors in view.
    fn calculate_layout(&mut self, monitors: &[Monitor]) {
        if monitors.is_empty() {
            self.scale = 1.0;
            self.offset = Point::new(0, 0);
            return;
        }

        // Find bounding box of all monitors
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        for m in monitors {
            min_x = min_x.min(m.rect.x);
            min_y = min_y.min(m.rect.y);
            max_x = max_x.max(m.rect.x + m.rect.width as i32);
            max_y = max_y.max(m.rect.y + m.rect.height as i32);
        }

        let total_width = (max_x - min_x) as f64;
        let total_height = (max_y - min_y) as f64;

        // Add padding
        let padding = 40.0;
        let available_width = self.view_rect.width as f64 - padding * 2.0;
        let available_height = self.view_rect.height as f64 - padding * 2.0;

        // Calculate scale to fit
        let scale_x = available_width / total_width;
        let scale_y = available_height / total_height;
        self.scale = scale_x.min(scale_y).min(0.15); // Cap at 15% to keep it reasonable

        // Calculate offset to center
        let scaled_width = total_width * self.scale;
        let scaled_height = total_height * self.scale;

        self.offset = Point::new(
            self.view_rect.x + ((self.view_rect.width as f64 - scaled_width) / 2.0) as i32
                - (min_x as f64 * self.scale) as i32,
            self.view_rect.y + ((self.view_rect.height as f64 - scaled_height) / 2.0) as i32
                - (min_y as f64 * self.scale) as i32,
        );
    }

    /// Scale a monitor rect to view coordinates.
    fn scale_rect(&self, rect: &Rect) -> Rect {
        Rect::new(
            self.offset.x + (rect.x as f64 * self.scale) as i32,
            self.offset.y + (rect.y as f64 * self.scale) as i32,
            (rect.width as f64 * self.scale) as u32,
            (rect.height as f64 * self.scale) as u32,
        )
    }

    /// Find monitor at a point.
    fn monitor_at_point(&self, point: Point) -> Option<usize> {
        for (i, state) in self.monitors.iter().enumerate().rev() {
            if state.scaled_rect.contains_point(point) {
                return Some(i);
            }
        }
        None
    }

    /// Handle an input event.
    pub fn handle_event(&mut self, event: &InputEvent) -> EventResult {
        match event {
            InputEvent::MouseMove(e) => {
                let new_hovered = self.monitor_at_point(e.position);
                if new_hovered != self.hovered {
                    self.hovered = new_hovered;
                    return EventResult::Redraw;
                }
            }
            InputEvent::MousePress(e) => {
                if let Some(index) = self.monitor_at_point(e.position) {
                    self.selected = Some(index);
                    return EventResult::Redraw;
                } else if self.selected.is_some() {
                    self.selected = None;
                    return EventResult::Redraw;
                }
            }
            _ => {}
        }
        EventResult::None
    }

    /// Render the monitor view.
    pub fn render(&self, renderer: &mut Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Background
        renderer.fill_rect(self.view_rect, theme.background)?;

        // Render each monitor
        for (i, state) in self.monitors.iter().enumerate() {
            self.render_monitor(renderer, theme, i, state)?;
        }

        Ok(())
    }

    /// Render a single monitor.
    fn render_monitor(
        &self,
        renderer: &Renderer,
        theme: &Theme,
        index: usize,
        state: &MonitorState,
    ) -> anyhow::Result<()> {
        let rect = state.scaled_rect;
        let is_primary = self
            .primary_name
            .as_ref()
            .is_some_and(|name| name == &state.info.name);

        // Determine colors based on state
        let (bg_color, border_color, border_width) = if Some(index) == self.selected {
            (
                theme.item_selected_background,
                theme.selection_background,
                3.0,
            )
        } else if Some(index) == self.hovered {
            (theme.item_hover_background, theme.border, 2.0)
        } else {
            (theme.item_background, theme.border, 1.0)
        };

        // Background
        renderer.fill_rounded_rect(rect, 8.0, bg_color)?;

        // Border (thicker for primary)
        let actual_border_width = if is_primary {
            border_width + 1.0
        } else {
            border_width
        };
        let actual_border_color = if is_primary {
            Color::new(0.3, 0.6, 1.0, 1.0) // Blue accent for primary
        } else {
            border_color
        };
        renderer.stroke_rounded_rect(rect, 8.0, actual_border_color, actual_border_width)?;

        // Monitor name
        let text_x = (rect.x + 10) as f64;
        let text_y = (rect.y + 10) as f64;
        renderer.text_default(&state.info.name, text_x, text_y, theme.foreground)?;

        // Resolution
        let res_text = format!("{}x{}", state.info.rect.width, state.info.rect.height);
        renderer.text_default(&res_text, text_x, text_y + 20.0, theme.item_description)?;

        // Primary indicator
        if is_primary {
            renderer.text_default("(primary)", text_x, text_y + 40.0, theme.selection_background)?;
        }

        Ok(())
    }

    /// Get the selected monitor index.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Get monitors.
    pub fn monitors(&self) -> &[MonitorState] {
        &self.monitors
    }

    /// Update view rect (e.g., on resize).
    pub fn set_view_rect(&mut self, rect: Rect) {
        self.view_rect = rect;
        // Recalculate layout
        let monitors: Vec<Monitor> = self.monitors.iter().map(|s| s.info.clone()).collect();
        self.set_monitors(monitors);
    }
}
