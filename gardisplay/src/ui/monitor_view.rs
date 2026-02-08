//! Monitor layout view - displays monitors as draggable rectangles.

use gartk_core::{Color, InputEvent, MouseButton, Point, Rect, Theme};
use gartk_render::Renderer;
use gartk_x11::Monitor;
use std::time::Instant;

use super::EventResult;

/// Snap threshold in pixels (scaled coordinates).
const SNAP_THRESHOLD: i32 = 15;

/// Double-click threshold in milliseconds.
const DOUBLE_CLICK_MS: u128 = 400;

/// Visual representation of a monitor in the layout.
#[derive(Debug, Clone)]
pub struct MonitorState {
    /// Monitor info from X11.
    pub info: Monitor,
    /// Scaled rectangle for display (updated during drag).
    pub scaled_rect: Rect,
    /// Real-world position (in actual pixels, updated after drag).
    pub real_position: Point,
}

/// State for an active drag operation.
#[derive(Debug, Clone)]
struct DragState {
    /// Index of the monitor being dragged.
    monitor_index: usize,
    /// Mouse position at drag start.
    start_mouse: Point,
    /// Monitor scaled rect at drag start.
    start_rect: Rect,
}

/// Alignment guide for snapping visualization.
#[derive(Debug, Clone, Copy)]
enum Alignment {
    Horizontal(i32), // y coordinate
    Vertical(i32),   // x coordinate
}

/// View showing all monitors as rectangles.
pub struct MonitorView {
    monitors: Vec<MonitorState>,
    selected: Option<usize>,
    hovered: Option<usize>,
    dragging: Option<DragState>,
    primary_name: Option<String>,
    view_rect: Rect,
    scale: f64,
    offset: Point,
    /// Current snap alignments (for drawing guidelines).
    alignments: Vec<Alignment>,
    /// Last click time and position for double-click detection.
    last_click: Option<(Instant, Point)>,
    /// Whether layout has been modified.
    dirty: bool,
}

impl MonitorView {
    /// Create a new monitor view.
    pub fn new(view_rect: Rect) -> Self {
        Self {
            monitors: Vec::new(),
            selected: None,
            hovered: None,
            dragging: None,
            primary_name: None,
            view_rect,
            scale: 1.0,
            offset: Point::new(0, 0),
            alignments: Vec::new(),
            last_click: None,
            dirty: false,
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
                let real_position = Point::new(info.rect.x, info.rect.y);
                MonitorState {
                    info,
                    scaled_rect,
                    real_position,
                }
            })
            .collect();

        self.dirty = false;

        tracing::debug!(
            "set {} monitors, scale={:.4}, offset=({}, {})",
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
        self.scale = scale_x.min(scale_y).min(0.2); // Cap at 20%

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
            ((rect.width as f64 * self.scale) as u32).max(1),
            ((rect.height as f64 * self.scale) as u32).max(1),
        )
    }

    /// Convert scaled coordinates back to real coordinates.
    fn unscale_point(&self, point: Point) -> Point {
        Point::new(
            ((point.x - self.offset.x) as f64 / self.scale) as i32,
            ((point.y - self.offset.y) as f64 / self.scale) as i32,
        )
    }

    /// Find monitor at a point.
    fn monitor_at_point(&self, point: Point) -> Option<usize> {
        // Check in reverse order (topmost first, dragged monitor is always on top)
        if let Some(ref drag) = self.dragging {
            if self.monitors[drag.monitor_index].scaled_rect.contains_point(point) {
                return Some(drag.monitor_index);
            }
        }

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
            InputEvent::MouseMove(e) => self.handle_mouse_move(e.position),
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                self.handle_mouse_press(e.position)
            }
            InputEvent::MouseRelease(e) if e.button == Some(MouseButton::Left) => {
                self.handle_mouse_release(e.position)
            }
            _ => EventResult::None,
        }
    }

    /// Handle mouse movement.
    fn handle_mouse_move(&mut self, position: Point) -> EventResult {
        if let Some(ref drag) = self.dragging {
            // Calculate delta from drag start
            let delta_x = position.x - drag.start_mouse.x;
            let delta_y = position.y - drag.start_mouse.y;

            // Update monitor position
            let idx = drag.monitor_index;
            self.monitors[idx].scaled_rect.x = drag.start_rect.x + delta_x;
            self.monitors[idx].scaled_rect.y = drag.start_rect.y + delta_y;

            // Check for snap alignments
            self.update_snap_alignments(idx);

            self.dirty = true;
            return EventResult::Redraw;
        }

        // Update hover state
        let new_hovered = self.monitor_at_point(position);
        if new_hovered != self.hovered {
            self.hovered = new_hovered;
            return EventResult::Redraw;
        }

        EventResult::None
    }

    /// Handle mouse press.
    fn handle_mouse_press(&mut self, position: Point) -> EventResult {
        // Check for double-click
        if let Some((last_time, last_pos)) = self.last_click {
            let elapsed = last_time.elapsed().as_millis();
            let dist = ((position.x - last_pos.x).abs() + (position.y - last_pos.y).abs()) as u128;

            if elapsed < DOUBLE_CLICK_MS && dist < 10 {
                // Double-click detected
                if let Some(index) = self.monitor_at_point(position) {
                    return self.toggle_primary(index);
                }
            }
        }

        self.last_click = Some((Instant::now(), position));

        if let Some(index) = self.monitor_at_point(position) {
            self.selected = Some(index);
            self.dragging = Some(DragState {
                monitor_index: index,
                start_mouse: position,
                start_rect: self.monitors[index].scaled_rect,
            });
            return EventResult::Redraw;
        } else if self.selected.is_some() {
            self.selected = None;
            return EventResult::Redraw;
        }

        EventResult::None
    }

    /// Handle mouse release.
    fn handle_mouse_release(&mut self, _position: Point) -> EventResult {
        if let Some(drag) = self.dragging.take() {
            // Apply snapping
            self.apply_snap(drag.monitor_index);

            // Update real position from scaled position
            let idx = drag.monitor_index;
            let scaled_rect = self.monitors[idx].scaled_rect;
            self.monitors[idx].real_position = self.unscale_point(Point::new(scaled_rect.x, scaled_rect.y));

            // Update the info rect as well
            self.monitors[idx].info.rect.x = self.monitors[idx].real_position.x;
            self.monitors[idx].info.rect.y = self.monitors[idx].real_position.y;

            // Clear alignments
            self.alignments.clear();

            tracing::debug!(
                "moved {} to real position ({}, {})",
                self.monitors[idx].info.name,
                self.monitors[idx].real_position.x,
                self.monitors[idx].real_position.y
            );

            return EventResult::Redraw;
        }
        EventResult::None
    }

    /// Toggle primary monitor designation.
    fn toggle_primary(&mut self, index: usize) -> EventResult {
        let name = &self.monitors[index].info.name;

        if self.primary_name.as_ref() == Some(name) {
            // Already primary, could unset or do nothing
            tracing::debug!("{} is already primary", name);
        } else {
            self.primary_name = Some(name.clone());
            self.dirty = true;
            tracing::debug!("set {} as primary", name);
        }

        EventResult::Redraw
    }

    /// Update snap alignment guides for a dragged monitor.
    fn update_snap_alignments(&mut self, dragged_idx: usize) {
        self.alignments.clear();

        let dragged = &self.monitors[dragged_idx].scaled_rect;

        for (i, other) in self.monitors.iter().enumerate() {
            if i == dragged_idx {
                continue;
            }

            let other_rect = &other.scaled_rect;

            // Check horizontal alignments (top/bottom edges)
            // Dragged top to other bottom
            if (dragged.y - (other_rect.y + other_rect.height as i32)).abs() < SNAP_THRESHOLD {
                self.alignments
                    .push(Alignment::Horizontal(other_rect.y + other_rect.height as i32));
            }
            // Dragged bottom to other top
            if ((dragged.y + dragged.height as i32) - other_rect.y).abs() < SNAP_THRESHOLD {
                self.alignments.push(Alignment::Horizontal(other_rect.y));
            }
            // Dragged top to other top (align)
            if (dragged.y - other_rect.y).abs() < SNAP_THRESHOLD {
                self.alignments.push(Alignment::Horizontal(other_rect.y));
            }
            // Dragged bottom to other bottom (align)
            if ((dragged.y + dragged.height as i32) - (other_rect.y + other_rect.height as i32)).abs() < SNAP_THRESHOLD {
                self.alignments
                    .push(Alignment::Horizontal(other_rect.y + other_rect.height as i32));
            }

            // Check vertical alignments (left/right edges)
            // Dragged left to other right
            if (dragged.x - (other_rect.x + other_rect.width as i32)).abs() < SNAP_THRESHOLD {
                self.alignments
                    .push(Alignment::Vertical(other_rect.x + other_rect.width as i32));
            }
            // Dragged right to other left
            if ((dragged.x + dragged.width as i32) - other_rect.x).abs() < SNAP_THRESHOLD {
                self.alignments.push(Alignment::Vertical(other_rect.x));
            }
            // Dragged left to other left (align)
            if (dragged.x - other_rect.x).abs() < SNAP_THRESHOLD {
                self.alignments.push(Alignment::Vertical(other_rect.x));
            }
            // Dragged right to other right (align)
            if ((dragged.x + dragged.width as i32) - (other_rect.x + other_rect.width as i32)).abs() < SNAP_THRESHOLD {
                self.alignments
                    .push(Alignment::Vertical(other_rect.x + other_rect.width as i32));
            }
        }
    }

    /// Apply snapping to a monitor after drag ends.
    /// Ensures no gaps - monitors must always be adjacent to at least one other.
    fn apply_snap(&mut self, dragged_idx: usize) {
        if self.monitors.len() < 2 {
            return;
        }

        // Always snap to nearest to ensure adjacency, using directional awareness
        let dragged = self.monitors[dragged_idx].scaled_rect;
        self.snap_to_nearest(dragged_idx, dragged);
    }

    /// Check if a rect overlaps with any monitor except the specified one.
    fn would_overlap(&self, rect: Rect, exclude_idx: usize) -> bool {
        for (i, other) in self.monitors.iter().enumerate() {
            if i == exclude_idx {
                continue;
            }
            if Self::rects_overlap(rect, other.scaled_rect) {
                return true;
            }
        }
        false
    }

    /// Check if two rects overlap (share any interior area).
    fn rects_overlap(a: Rect, b: Rect) -> bool {
        let a_right = a.x + a.width as i32;
        let a_bottom = a.y + a.height as i32;
        let b_right = b.x + b.width as i32;
        let b_bottom = b.y + b.height as i32;

        a.x < b_right && a_right > b.x && a.y < b_bottom && a_bottom > b.y
    }

    /// Snap a monitor to be adjacent to the nearest other monitor.
    /// Uses directional awareness - snaps to the side the monitor was dropped on.
    /// Ensures no overlaps with other monitors.
    fn snap_to_nearest(&mut self, dragged_idx: usize, dragged: Rect) {
        let dragged_center_x = dragged.x + dragged.width as i32 / 2;
        let dragged_center_y = dragged.y + dragged.height as i32 / 2;

        let mut best_snap: Option<(i32, i32, i32)> = None; // (new_x, new_y, distance)

        for (i, other) in self.monitors.iter().enumerate() {
            if i == dragged_idx {
                continue;
            }

            let other_rect = other.scaled_rect;
            let other_center_x = other_rect.x + other_rect.width as i32 / 2;
            let other_center_y = other_rect.y + other_rect.height as i32 / 2;

            // Determine which side of the other monitor we're on
            let dx = dragged_center_x - other_center_x;
            let dy = dragged_center_y - other_center_y;

            // Try all 4 sides and pick the best non-overlapping position
            let candidates = [
                // Right of other
                (other_rect.x + other_rect.width as i32, other_rect.y),
                // Left of other
                (other_rect.x - dragged.width as i32, other_rect.y),
                // Below other
                (other_rect.x, other_rect.y + other_rect.height as i32),
                // Above other
                (other_rect.x, other_rect.y - dragged.height as i32),
            ];

            // Score each candidate based on direction preference
            for (new_x, new_y) in candidates {
                let candidate_rect = Rect::new(new_x, new_y, dragged.width, dragged.height);

                // Skip if this position would overlap with another monitor
                if self.would_overlap(candidate_rect, dragged_idx) {
                    continue;
                }

                // Calculate distance with direction weighting
                let new_center_x = new_x + dragged.width as i32 / 2;
                let new_center_y = new_y + dragged.height as i32 / 2;

                // Base distance
                let mut dist = (dragged_center_x - new_center_x).abs()
                    + (dragged_center_y - new_center_y).abs();

                // Penalize positions that don't match the drag direction
                let snap_dx = new_center_x - other_center_x;
                let snap_dy = new_center_y - other_center_y;

                // If we're dragging more horizontally, prefer horizontal snaps
                if dx.abs() > dy.abs() {
                    if (dx > 0) != (snap_dx > 0) {
                        dist += 1000; // Penalize wrong horizontal direction
                    }
                } else {
                    if (dy > 0) != (snap_dy > 0) {
                        dist += 1000; // Penalize wrong vertical direction
                    }
                }

                if best_snap.map_or(true, |(_, _, best_dist)| dist < best_dist) {
                    best_snap = Some((new_x, new_y, dist));
                }
            }
        }

        if let Some((new_x, new_y, _)) = best_snap {
            self.monitors[dragged_idx].scaled_rect.x = new_x;
            self.monitors[dragged_idx].scaled_rect.y = new_y;
        }
    }

    /// Render the monitor view.
    pub fn render(&self, renderer: &mut Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Background
        renderer.fill_rect(self.view_rect, theme.background)?;

        // Render alignment guidelines first (below monitors)
        self.render_guidelines(renderer)?;

        // Render each monitor (dragged one last so it's on top)
        let dragged_idx = self.dragging.as_ref().map(|d| d.monitor_index);

        for (i, state) in self.monitors.iter().enumerate() {
            if Some(i) != dragged_idx {
                self.render_monitor(renderer, theme, i, state, false)?;
            }
        }

        // Render dragged monitor on top
        if let Some(idx) = dragged_idx {
            self.render_monitor(renderer, theme, idx, &self.monitors[idx], true)?;
        }

        Ok(())
    }

    /// Render alignment guidelines.
    fn render_guidelines(&self, renderer: &Renderer) -> anyhow::Result<()> {
        let guideline_color = Color::new(0.2, 0.6, 1.0, 0.6); // Blue, semi-transparent

        for alignment in &self.alignments {
            match alignment {
                Alignment::Horizontal(y) => {
                    renderer.line(
                        self.view_rect.x as f64,
                        *y as f64,
                        (self.view_rect.x + self.view_rect.width as i32) as f64,
                        *y as f64,
                        guideline_color,
                        1.0,
                    )?;
                }
                Alignment::Vertical(x) => {
                    renderer.line(
                        *x as f64,
                        self.view_rect.y as f64,
                        *x as f64,
                        (self.view_rect.y + self.view_rect.height as i32) as f64,
                        guideline_color,
                        1.0,
                    )?;
                }
            }
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
        is_dragging: bool,
    ) -> anyhow::Result<()> {
        let rect = state.scaled_rect;
        let is_primary = self
            .primary_name
            .as_ref()
            .is_some_and(|name| name == &state.info.name);

        // Determine colors based on state
        let (bg_color, border_color, border_width) = if is_dragging {
            // Dragging - semi-transparent with accent
            (
                theme.item_selected_background.with_alpha(0.8),
                Color::new(0.3, 0.7, 1.0, 1.0),
                3.0,
            )
        } else if Some(index) == self.selected {
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
        let actual_border_color = if is_primary && !is_dragging {
            Color::new(0.3, 0.6, 1.0, 1.0) // Blue accent for primary
        } else {
            border_color
        };
        renderer.stroke_rounded_rect(rect, 8.0, actual_border_color, actual_border_width)?;

        // Only draw text if monitor is large enough
        if rect.width > 60 && rect.height > 50 {
            // Monitor name
            let text_x = (rect.x + 10) as f64;
            let text_y = (rect.y + 10) as f64;
            renderer.text_default(&state.info.name, text_x, text_y, theme.foreground)?;

            // Resolution
            if rect.height > 70 {
                let res_text = format!("{}x{}", state.info.rect.width, state.info.rect.height);
                renderer.text_default(&res_text, text_x, text_y + 18.0, theme.item_description)?;
            }

            // Primary indicator
            if is_primary && rect.height > 90 {
                renderer.text_default(
                    "(primary)",
                    text_x,
                    text_y + 36.0,
                    theme.selection_background,
                )?;
            }
        }

        Ok(())
    }

    /// Get the selected monitor index.
    #[allow(dead_code)] // Used in Sprint 3 for RandR application
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Get monitors.
    #[allow(dead_code)] // Used in Sprint 3 for RandR application
    pub fn monitors(&self) -> &[MonitorState] {
        &self.monitors
    }

    /// Get mutable monitors.
    pub fn monitors_mut(&mut self) -> &mut [MonitorState] {
        &mut self.monitors
    }

    /// Set primary monitor by name.
    pub fn set_primary(&mut self, name: &str) {
        self.primary_name = Some(name.to_string());
    }

    /// Recalculate layout from real positions.
    pub fn recalculate_layout(&mut self) {
        self.recalculate_scaled_rects();
    }

    /// Check if layout has been modified.
    #[allow(dead_code)] // Used in Sprint 3 for RandR application
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get primary monitor name.
    #[allow(dead_code)] // Used in Sprint 3 for RandR application
    pub fn primary_name(&self) -> Option<&str> {
        self.primary_name.as_deref()
    }

    /// Update view rect (e.g., on resize).
    pub fn set_view_rect(&mut self, rect: Rect) {
        self.view_rect = rect;
        // Recalculate layout preserving real positions
        self.recalculate_scaled_rects();
    }

    /// Recalculate scaled rects from real positions.
    fn recalculate_scaled_rects(&mut self) {
        if self.monitors.is_empty() {
            return;
        }

        // Rebuild Monitor vec from current state
        let monitors: Vec<Monitor> = self
            .monitors
            .iter()
            .map(|s| {
                let mut m = s.info.clone();
                m.rect.x = s.real_position.x;
                m.rect.y = s.real_position.y;
                m
            })
            .collect();

        // Recalculate layout
        self.calculate_layout(&monitors);

        // Capture scale and offset before mutable borrow
        let scale = self.scale;
        let offset = self.offset;

        // Update scaled rects
        for state in &mut self.monitors {
            let rect = Rect::new(
                state.real_position.x,
                state.real_position.y,
                state.info.rect.width,
                state.info.rect.height,
            );
            state.scaled_rect = Rect::new(
                offset.x + (rect.x as f64 * scale) as i32,
                offset.y + (rect.y as f64 * scale) as i32,
                ((rect.width as f64 * scale) as u32).max(1),
                ((rect.height as f64 * scale) as u32).max(1),
            );
        }
    }
}
