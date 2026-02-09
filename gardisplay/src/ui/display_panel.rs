//! Display settings panel for selected monitor.

use std::collections::HashSet;

use gartk_core::{InputEvent, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

use crate::randr::{ModeInfo, OutputInfo};
use crate::ui::widgets::{Dropdown, Toggle};

/// Configuration values from the display panel.
#[derive(Debug, Clone)]
pub struct DisplayPanelConfig {
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub rotation: u32,
    pub scale: f64,
    pub enabled: bool,
}

/// Result of handling a panel event.
#[derive(Debug)]
pub enum DisplayPanelResult {
    /// No action needed.
    None,
    /// Redraw the UI.
    Redraw,
    /// Configuration changed.
    ConfigChanged(DisplayPanelConfig),
}

/// Display settings panel for the selected monitor.
pub struct DisplayPanel {
    rect: Rect,
    // Widgets
    resolution_dropdown: Dropdown,
    refresh_dropdown: Dropdown,
    rotation_dropdown: Dropdown,
    scale_dropdown: Dropdown,
    enabled_toggle: Toggle,
    // State
    selected_output: Option<String>,
    available_modes: Vec<ModeInfo>,
    // Current values
    current_width: u32,
    current_height: u32,
    current_refresh: f64,
    current_rotation: u32,
    current_scale: f64,
    current_enabled: bool,
}

impl DisplayPanel {
    /// Create a new display panel.
    pub fn new(rect: Rect) -> Self {
        // Calculate widget positions based on panel rect
        let row1_y = rect.y + 40;
        let row2_y = rect.y + 80;
        let col1_x = rect.x + 100;
        let col2_x = rect.x + (rect.width as i32 / 2) + 80;
        let dropdown_width = 120;
        let dropdown_height = 28;

        let resolution_dropdown = Dropdown::new(col1_x, row1_y, dropdown_width, dropdown_height);
        let refresh_dropdown = Dropdown::new(col2_x, row1_y, 80, dropdown_height);
        let rotation_dropdown = Dropdown::new(col1_x, row2_y, 80, dropdown_height);
        let scale_dropdown = Dropdown::new(col2_x, row2_y, 80, dropdown_height);
        let enabled_toggle = Toggle::new(
            rect.x + rect.width as i32 - 150,
            rect.y + 8,
            140,
            28,
            "Enabled",
        );

        Self {
            rect,
            resolution_dropdown,
            refresh_dropdown,
            rotation_dropdown,
            scale_dropdown,
            enabled_toggle,
            selected_output: None,
            available_modes: Vec::new(),
            current_width: 0,
            current_height: 0,
            current_refresh: 60.0,
            current_rotation: 0,
            current_scale: 1.0,
            current_enabled: true,
        }
    }

    /// Update panel to show settings for the selected monitor.
    ///
    /// `name` is the monitor name (required for selection).
    /// `output` is optional RandR data for populating available modes.
    pub fn set_selected_monitor(
        &mut self,
        name: Option<&str>,
        output: Option<&OutputInfo>,
        width: u32,
        height: u32,
        refresh: f64,
        rotation: u32,
        scale: f64,
        enabled: bool,
    ) {
        if let Some(monitor_name) = name {
            self.selected_output = Some(monitor_name.to_string());

            // Set current values
            self.current_width = width;
            self.current_height = height;
            self.current_refresh = refresh;
            self.current_rotation = rotation;
            self.current_scale = scale;
            self.current_enabled = enabled;

            // Populate from RandR if available, otherwise use current values
            if let Some(output) = output {
                self.available_modes = output.modes.clone();

                // Populate resolution dropdown with unique resolutions
                let resolutions: Vec<String> = output
                    .modes
                    .iter()
                    .map(|m| format!("{}x{}", m.width, m.height))
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                let mut sorted_resolutions: Vec<String> = resolutions;
                sorted_resolutions.sort_by(|a, b| {
                    let parse_res = |s: &str| -> u64 {
                        let parts: Vec<&str> = s.split('x').collect();
                        if parts.len() == 2 {
                            parts[0].parse::<u64>().unwrap_or(0)
                                * parts[1].parse::<u64>().unwrap_or(0)
                        } else {
                            0
                        }
                    };
                    parse_res(b).cmp(&parse_res(a))
                });
                self.resolution_dropdown.set_items(sorted_resolutions);

                // Populate refresh rates for current resolution
                self.update_refresh_dropdown(width, height);
            } else {
                // Demo mode: just show current resolution/refresh
                self.available_modes.clear();
                self.resolution_dropdown.set_items(vec![format!("{}x{}", width, height)]);
                self.refresh_dropdown.set_items(vec![format!("{:.0}Hz", refresh)]);
            }

            // Set current resolution
            let current_res = format!("{}x{}", width, height);
            self.resolution_dropdown.set_selected_by_name(&current_res);

            // Set current refresh
            let current_refresh_str = format!("{:.0}Hz", refresh);
            self.refresh_dropdown.set_selected_by_name(&current_refresh_str);

            // Rotation options (always available)
            self.rotation_dropdown
                .set_items(vec!["0".to_string(), "90".to_string(), "180".to_string(), "270".to_string()]);
            self.rotation_dropdown
                .set_selected_by_name(&rotation.to_string());

            // Scale options (always available)
            self.scale_dropdown.set_items(vec![
                "1x".to_string(),
                "1.25x".to_string(),
                "1.5x".to_string(),
                "1.75x".to_string(),
                "2x".to_string(),
            ]);
            let scale_str = format!("{}x", scale);
            self.scale_dropdown.set_selected_by_name(&scale_str);

            // Enable toggle
            self.enabled_toggle.set_value(enabled);
        } else {
            self.selected_output = None;
            self.available_modes.clear();
            self.resolution_dropdown.set_items(Vec::new());
            self.refresh_dropdown.set_items(Vec::new());
        }
    }

    /// Update refresh rate dropdown for the given resolution.
    fn update_refresh_dropdown(&mut self, width: u32, height: u32) {
        let refreshes: Vec<String> = self
            .available_modes
            .iter()
            .filter(|m| m.width as u32 == width && m.height as u32 == height)
            .map(|m| format!("{:.0}Hz", m.refresh))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut sorted_refreshes: Vec<String> = refreshes;
        sorted_refreshes.sort_by(|a, b| {
            let parse_hz = |s: &str| -> f64 {
                s.trim_end_matches("Hz").parse::<f64>().unwrap_or(0.0)
            };
            parse_hz(b).partial_cmp(&parse_hz(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        self.refresh_dropdown.set_items(sorted_refreshes);
    }

    /// Get the current configuration.
    pub fn get_config(&self) -> Option<DisplayPanelConfig> {
        if self.selected_output.is_some() {
            Some(DisplayPanelConfig {
                width: self.current_width,
                height: self.current_height,
                refresh: self.current_refresh,
                rotation: self.current_rotation,
                scale: self.current_scale,
                enabled: self.current_enabled,
            })
        } else {
            None
        }
    }

    /// Get the selected output name.
    pub fn selected_output(&self) -> Option<&str> {
        self.selected_output.as_deref()
    }

    /// Set the panel rect.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;

        // Recalculate widget positions
        let row1_y = rect.y + 40;
        let row2_y = rect.y + 80;
        let col1_x = rect.x + 100;
        let col2_x = rect.x + (rect.width as i32 / 2) + 80;

        self.resolution_dropdown.set_position(col1_x, row1_y);
        self.refresh_dropdown.set_position(col2_x, row1_y);
        self.rotation_dropdown.set_position(col1_x, row2_y);
        self.scale_dropdown.set_position(col2_x, row2_y);
        self.enabled_toggle
            .set_position(rect.x + rect.width as i32 - 150, rect.y + 8);
    }

    /// Handle an input event.
    pub fn handle_event(&mut self, event: &InputEvent) -> DisplayPanelResult {
        // Only handle events if we have a selected output
        if self.selected_output.is_none() {
            return DisplayPanelResult::None;
        }

        let mut config_changed = false;

        // Handle enable toggle
        if self.enabled_toggle.handle_event(event) {
            self.current_enabled = self.enabled_toggle.value();
            config_changed = true;
        }

        // Handle resolution dropdown
        if let Some(action) = self.resolution_dropdown.handle_event(event) {
            if let crate::ui::widgets::DropdownAction::Select(_) = action {
                if let Some(res) = self.resolution_dropdown.selected_item() {
                    // Parse resolution
                    let parts: Vec<&str> = res.split('x').collect();
                    if parts.len() == 2 {
                        if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                            self.current_width = w;
                            self.current_height = h;
                            // Update refresh rates for new resolution
                            self.update_refresh_dropdown(w, h);
                            // Select first available refresh rate
                            if let Some(first_refresh) = self.refresh_dropdown.selected_item() {
                                let hz_str = first_refresh.trim_end_matches("Hz");
                                if let Ok(hz) = hz_str.parse::<f64>() {
                                    self.current_refresh = hz;
                                }
                            }
                            config_changed = true;
                        }
                    }
                }
            }
            return if config_changed {
                DisplayPanelResult::ConfigChanged(self.get_config().unwrap())
            } else {
                DisplayPanelResult::Redraw
            };
        }

        // Handle refresh dropdown
        if let Some(action) = self.refresh_dropdown.handle_event(event) {
            if let crate::ui::widgets::DropdownAction::Select(_) = action {
                if let Some(refresh) = self.refresh_dropdown.selected_item() {
                    let hz_str = refresh.trim_end_matches("Hz");
                    if let Ok(hz) = hz_str.parse::<f64>() {
                        self.current_refresh = hz;
                        config_changed = true;
                    }
                }
            }
            return if config_changed {
                DisplayPanelResult::ConfigChanged(self.get_config().unwrap())
            } else {
                DisplayPanelResult::Redraw
            };
        }

        // Handle rotation dropdown
        if let Some(action) = self.rotation_dropdown.handle_event(event) {
            if let crate::ui::widgets::DropdownAction::Select(_) = action {
                if let Some(rot) = self.rotation_dropdown.selected_item() {
                    if let Ok(r) = rot.parse::<u32>() {
                        self.current_rotation = r;
                        config_changed = true;
                    }
                }
            }
            return if config_changed {
                DisplayPanelResult::ConfigChanged(self.get_config().unwrap())
            } else {
                DisplayPanelResult::Redraw
            };
        }

        // Handle scale dropdown
        if let Some(action) = self.scale_dropdown.handle_event(event) {
            if let crate::ui::widgets::DropdownAction::Select(_) = action {
                if let Some(scale) = self.scale_dropdown.selected_item() {
                    let scale_str = scale.trim_end_matches('x');
                    if let Ok(s) = scale_str.parse::<f64>() {
                        self.current_scale = s;
                        config_changed = true;
                    }
                }
            }
            return if config_changed {
                DisplayPanelResult::ConfigChanged(self.get_config().unwrap())
            } else {
                DisplayPanelResult::Redraw
            };
        }

        // Check dropdown expand/collapse state changes
        if self.resolution_dropdown.is_expanded()
            || self.refresh_dropdown.is_expanded()
            || self.rotation_dropdown.is_expanded()
            || self.scale_dropdown.is_expanded()
        {
            return DisplayPanelResult::Redraw;
        }

        if config_changed {
            DisplayPanelResult::ConfigChanged(self.get_config().unwrap())
        } else {
            DisplayPanelResult::None
        }
    }

    /// Render the panel.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Panel background
        renderer.fill_rect(self.rect, theme.background)?;

        // Top border
        renderer.line(
            self.rect.x as f64,
            self.rect.y as f64,
            (self.rect.x + self.rect.width as i32) as f64,
            self.rect.y as f64,
            theme.border,
            1.0,
        )?;

        let label_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.item_description)
            .align(TextAlign::Left);

        let title_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size + 2.0)
            .color(theme.foreground)
            .align(TextAlign::Left);

        if let Some(ref name) = self.selected_output {
            // Title: Monitor name
            let title_rect = Rect::new(self.rect.x + 10, self.rect.y + 8, 200, 28);
            renderer.text_in_rect(name, title_rect, &title_style)?;

            // Enable toggle
            self.enabled_toggle.render(renderer, theme)?;

            // Row 1: Resolution and Refresh
            let row1_y = self.rect.y + 45;
            renderer.text_in_rect(
                "Resolution:",
                Rect::new(self.rect.x + 10, row1_y, 90, 28),
                &label_style,
            )?;
            self.resolution_dropdown.render(renderer, theme)?;

            renderer.text_in_rect(
                "Refresh:",
                Rect::new(self.rect.x + (self.rect.width as i32 / 2) + 10, row1_y, 70, 28),
                &label_style,
            )?;
            self.refresh_dropdown.render(renderer, theme)?;

            // Row 2: Rotation and Scale
            let row2_y = self.rect.y + 85;
            renderer.text_in_rect(
                "Rotation:",
                Rect::new(self.rect.x + 10, row2_y, 90, 28),
                &label_style,
            )?;
            self.rotation_dropdown.render(renderer, theme)?;

            renderer.text_in_rect(
                "Scale:",
                Rect::new(self.rect.x + (self.rect.width as i32 / 2) + 10, row2_y, 70, 28),
                &label_style,
            )?;
            self.scale_dropdown.render(renderer, theme)?;
        } else {
            // No monitor selected
            let hint_style = TextStyle::new()
                .font_family(&theme.font_family)
                .font_size(theme.font_size)
                .color(theme.item_description)
                .align(TextAlign::Center);
            renderer.text_in_rect(
                "Select a monitor to configure",
                self.rect,
                &hint_style,
            )?;
        }

        Ok(())
    }
}
