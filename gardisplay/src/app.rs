//! Main application state and event loop.

use anyhow::Result;
use gartk_core::{Color, InputEvent, Key, Rect, Size, Theme};
use gartk_render::{copy_surface_to_window, Renderer};
use gartk_x11::{
    detect_monitors, primary_monitor, Connection, EventLoop, EventLoopConfig, Monitor, Window,
    WindowConfig,
};
use x11rb::protocol::xproto::ConnectionExt;

use crate::config::{Config, MonitorConfig, Profile};
use crate::randr::RandrManager;
use crate::ui::{Button, Dropdown, DropdownAction, EventResult, MonitorView, TextInput};

/// Window dimensions.
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

/// Main application.
pub struct App {
    #[allow(dead_code)] // Connection kept alive for X11 resources
    conn: Connection,
    window: Window,
    renderer: Renderer,
    theme: Theme,
    gc: u32,
    config: Config,
    monitor_view: MonitorView,
    randr: Option<RandrManager>,
    original_monitors: Vec<Monitor>,
    demo_mode: bool,
    status_message: Option<(String, std::time::Instant)>,
    // UI widgets
    current_profile: String,
    dropdown_profiles: Dropdown,
    btn_apply: Button,
    btn_revert: Button,
    btn_save: Button,
    btn_save_as: Button,
    save_as_input: Option<TextInput>,
}

impl App {
    /// Create a new application instance.
    pub fn new(config: Config, demo: bool) -> Result<Self> {
        // Connect to X11
        let conn = Connection::connect(None)?;
        tracing::info!("connected to X11 display");

        // Get monitor for positioning
        let monitor = primary_monitor(&conn).unwrap_or_else(|_| {
            tracing::warn!("could not get primary monitor, using defaults");
            gartk_x11::Monitor {
                name: "default".to_string(),
                rect: Rect::new(0, 0, 1920, 1080),
                primary: true,
                width_mm: 0,
                height_mm: 0,
            }
        });

        // Center window on monitor
        let x = monitor.rect.x + (monitor.rect.width as i32 - WINDOW_WIDTH as i32) / 2;
        let y = monitor.rect.y + (monitor.rect.height as i32 - WINDOW_HEIGHT as i32) / 2;

        // Create window
        let window = Window::create(
            conn.clone(),
            WindowConfig::new()
                .title("gardisplay")
                .class("gardisplay")
                .position(x, y)
                .size(WINDOW_WIDTH, WINDOW_HEIGHT),
        )?;
        window.map()?;
        tracing::info!(
            "created window {}x{} at ({}, {})",
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            x,
            y
        );

        // Create graphics context for blitting
        let gc = conn.generate_id()?;
        conn.inner()
            .create_gc(gc, window.id(), &Default::default())?;

        // Create renderer with theme
        let theme = Theme::dark();
        let renderer = Renderer::with_theme(WINDOW_WIDTH, WINDOW_HEIGHT, theme.clone())?;

        // Create monitor view
        let view_rect = Rect::new(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT - 100); // Leave room for controls
        let mut monitor_view = MonitorView::new(view_rect);

        // Create RandR manager (only in non-demo mode)
        let randr = if demo {
            None
        } else {
            match RandrManager::new(conn.clone()) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!("failed to create RandR manager: {}", e);
                    None
                }
            }
        };

        // Detect or create demo monitors
        let monitors = if demo {
            tracing::info!("demo mode: using fake monitors");
            Self::demo_monitors()
        } else {
            detect_monitors(&conn)?
        };
        tracing::info!("using {} monitors", monitors.len());
        for m in &monitors {
            tracing::debug!(
                "  {} {}x{} at ({}, {}) {}",
                m.name,
                m.rect.width,
                m.rect.height,
                m.rect.x,
                m.rect.y,
                if m.primary { "(primary)" } else { "" }
            );
        }

        // Store original state for reverting
        let original_monitors = monitors.clone();
        monitor_view.set_monitors(monitors);

        // Try to load saved profile
        if let Some(profile) = config.profiles.get(&config.general.default_profile) {
            tracing::info!("loading profile '{}'", config.general.default_profile);
            Self::apply_profile_to_view(&mut monitor_view, profile);
        }

        // Create UI widgets (positioned in controls area)
        let controls_y = (WINDOW_HEIGHT - 100) as i32;

        // Buttons on the left
        let btn_apply = Button::new(10, controls_y + 60, 70, 32, "Apply");
        let btn_revert = Button::new(90, controls_y + 60, 70, 32, "Revert");
        let btn_save = Button::new(170, controls_y + 60, 60, 32, "Save");

        // Profile dropdown and Save As on the right
        let mut dropdown_profiles = Dropdown::new(WINDOW_WIDTH as i32 - 260, controls_y + 60, 150, 32);
        let profile_names: Vec<String> = config.profiles.keys().cloned().collect();
        let current_profile = config.general.default_profile.clone();
        if profile_names.is_empty() {
            dropdown_profiles.set_items(vec!["default".to_string()]);
        } else {
            dropdown_profiles.set_items(profile_names);
        }
        dropdown_profiles.set_selected_by_name(&current_profile);
        let btn_save_as = Button::new(WINDOW_WIDTH as i32 - 100, controls_y + 60, 90, 32, "Save As");

        Ok(Self {
            conn,
            window,
            renderer,
            theme,
            gc,
            config,
            monitor_view,
            randr,
            original_monitors,
            demo_mode: demo,
            status_message: None,
            current_profile,
            dropdown_profiles,
            btn_apply,
            btn_revert,
            btn_save,
            btn_save_as,
            save_as_input: None,
        })
    }

    /// Apply a saved profile to the monitor view.
    fn apply_profile_to_view(view: &mut MonitorView, profile: &Profile) {
        // Build a map of monitor name -> config
        let config_map: std::collections::HashMap<&str, &MonitorConfig> = profile
            .monitors
            .iter()
            .map(|m| (m.name.as_str(), m))
            .collect();

        // Update monitor positions from profile
        for state in view.monitors_mut() {
            if let Some(config) = config_map.get(state.info.name.as_str()) {
                state.real_position = gartk_core::Point::new(config.x, config.y);
                tracing::debug!(
                    "loaded {} at ({}, {})",
                    state.info.name,
                    config.x,
                    config.y
                );
            }
        }

        // Set primary monitor
        if let Some(ref primary) = profile.primary {
            view.set_primary(primary);
        }

        // Recalculate scaling
        view.recalculate_layout();
    }

    /// Create demo monitors for UI testing.
    fn demo_monitors() -> Vec<gartk_x11::Monitor> {
        vec![
            gartk_x11::Monitor {
                name: "eDP-1".to_string(),
                rect: Rect::new(0, 0, 2560, 1600),
                primary: true,
                width_mm: 290,
                height_mm: 180,
            },
            gartk_x11::Monitor {
                name: "HDMI-1".to_string(),
                rect: Rect::new(2560, 0, 1920, 1080),
                primary: false,
                width_mm: 530,
                height_mm: 300,
            },
            gartk_x11::Monitor {
                name: "DP-1".to_string(),
                rect: Rect::new(2560, 1080, 1920, 1080),
                primary: false,
                width_mm: 530,
                height_mm: 300,
            },
        ]
    }

    /// Run the application event loop.
    pub fn run(&mut self) -> Result<()> {
        // Initial render
        self.render()?;

        let mut event_loop = EventLoop::new(&self.window, EventLoopConfig::default())?;

        event_loop.run(|loop_state, event| {
            match self.handle_event(&event) {
                EventResult::Quit => return Ok(false),
                EventResult::Redraw => {
                    loop_state.request_redraw();
                }
                EventResult::None => {}
            }

            // Render on idle if needed
            if matches!(event, InputEvent::Idle) && loop_state.needs_redraw() {
                if let Err(e) = self.render() {
                    tracing::error!("render error: {}", e);
                }
                loop_state.redraw_done();
            }

            Ok(true)
        })?;

        tracing::info!("exiting");
        Ok(())
    }

    /// Handle an input event.
    fn handle_event(&mut self, event: &InputEvent) -> EventResult {
        // Handle save-as text input first (captures keyboard when active)
        if let Some(ref mut input) = self.save_as_input {
            if let Some(submitted) = input.handle_event(event) {
                if submitted {
                    let name = input.text().to_string();
                    if !name.is_empty() {
                        self.save_profile_as(&name);
                    }
                }
                self.save_as_input = None;
                return EventResult::Redraw;
            }
            // Text input is active, consume the event
            return EventResult::Redraw;
        }

        // Handle dropdown events
        if let Some(action) = self.dropdown_profiles.handle_event(event) {
            match action {
                DropdownAction::Select(_idx) => {
                    if let Some(name) = self.dropdown_profiles.selected_item() {
                        let name = name.to_string();
                        self.load_profile(&name);
                    }
                }
                DropdownAction::Rename(_idx, new_name) => {
                    if let Some(old_name) = self.dropdown_profiles.selected_item() {
                        let old = old_name.to_string();
                        self.rename_profile(&old, &new_name);
                    }
                }
                DropdownAction::Delete(idx) => {
                    // Get name before modifying
                    let names: Vec<String> = self.config.profiles.keys().cloned().collect();
                    if let Some(name) = names.get(idx) {
                        self.delete_profile(name);
                    }
                }
            }
            return EventResult::Redraw;
        }

        // Handle button clicks
        if self.btn_apply.handle_event(event) {
            self.apply_layout();
            return EventResult::Redraw;
        }
        if self.btn_revert.handle_event(event) {
            self.revert_layout();
            return EventResult::Redraw;
        }
        if self.btn_save.handle_event(event) {
            self.save_profile();
            return EventResult::Redraw;
        }
        if self.btn_save_as.handle_event(event) {
            // Show save-as input (appears above the dropdown)
            let size = self.renderer.size();
            let controls_y = size.height.saturating_sub(100) as i32;
            let mut input = TextInput::new(size.width as i32 - 260, controls_y + 25, 150, 32);
            input.set_placeholder("Profile name");
            input.set_active(true);
            self.save_as_input = Some(input);
            return EventResult::Redraw;
        }

        match event {
            InputEvent::CloseRequested => EventResult::Quit,
            InputEvent::Key(e) if e.pressed && e.key == Key::Escape => EventResult::Quit,
            InputEvent::Key(e) if e.pressed && e.key == Key::Char('q') => EventResult::Quit,
            // Apply: Ctrl+A or Enter
            InputEvent::Key(e)
                if e.pressed
                    && (e.key == Key::Return
                        || (e.modifiers.ctrl && e.key == Key::Char('a'))) =>
            {
                self.apply_layout();
                EventResult::Redraw
            }
            // Revert: Ctrl+R or Backspace
            InputEvent::Key(e)
                if e.pressed
                    && (e.key == Key::Backspace
                        || (e.modifiers.ctrl && e.key == Key::Char('r'))) =>
            {
                self.revert_layout();
                EventResult::Redraw
            }
            // Save: Ctrl+S
            InputEvent::Key(e) if e.pressed && e.modifiers.ctrl && e.key == Key::Char('s') => {
                self.save_profile();
                EventResult::Redraw
            }
            InputEvent::Resize { width, height } => {
                self.handle_resize(Size::new(*width, *height));
                EventResult::Redraw
            }
            InputEvent::Expose => EventResult::Redraw,
            _ => self.monitor_view.handle_event(event),
        }
    }

    /// Apply the current layout via RandR.
    fn apply_layout(&mut self) {
        if self.demo_mode {
            self.set_status("Demo mode - changes not applied");
            return;
        }

        let Some(ref randr) = self.randr else {
            self.set_status("RandR not available");
            return;
        };

        // Build MonitorConfig from current view state
        // Collect all data we need before releasing the borrow
        let primary_name = self.monitor_view.primary_name().map(|s| s.to_string());
        let configs: Vec<(MonitorConfig, Monitor)> = self
            .monitor_view
            .monitors()
            .iter()
            .map(|state| {
                let config = MonitorConfig {
                    name: state.info.name.clone(),
                    enabled: true,
                    x: state.real_position.x,
                    y: state.real_position.y,
                    width: state.info.rect.width,
                    height: state.info.rect.height,
                    refresh: 60.0, // TODO: get actual refresh rate
                    scale: 1.0,
                    rotation: 0,
                };
                let monitor = Monitor {
                    name: state.info.name.clone(),
                    rect: Rect::new(
                        state.real_position.x,
                        state.real_position.y,
                        state.info.rect.width,
                        state.info.rect.height,
                    ),
                    primary: primary_name.as_ref() == Some(&state.info.name),
                    width_mm: state.info.width_mm,
                    height_mm: state.info.height_mm,
                };
                (config, monitor)
            })
            .collect();

        let mut success_count = 0;
        let mut error_count = 0;

        for (config, _) in &configs {
            match randr.apply_monitor(config) {
                Ok(()) => success_count += 1,
                Err(e) => {
                    tracing::error!("failed to apply config for {}: {}", config.name, e);
                    error_count += 1;
                }
            }
        }

        // Set primary
        if let Some(ref name) = primary_name {
            if let Err(e) = randr.set_primary(name) {
                tracing::error!("failed to set primary: {}", e);
            }
        }

        if let Err(e) = randr.flush() {
            tracing::error!("failed to flush: {}", e);
        }

        if error_count == 0 {
            self.set_status(&format!("Applied {} monitor(s)", success_count));
            // Update original state after successful apply
            self.original_monitors = configs.into_iter().map(|(_, m)| m).collect();
        } else {
            self.set_status(&format!(
                "Applied {} monitor(s), {} error(s)",
                success_count, error_count
            ));
        }
    }

    /// Revert to the original layout.
    fn revert_layout(&mut self) {
        if self.demo_mode {
            // In demo mode, just reset the view
            self.monitor_view.set_monitors(self.original_monitors.clone());
            self.set_status("Reverted to original layout");
            return;
        }

        // Restore original monitors in view
        self.monitor_view.set_monitors(self.original_monitors.clone());

        // Apply via RandR
        if let Some(ref randr) = self.randr {
            for m in &self.original_monitors {
                let config = MonitorConfig {
                    name: m.name.clone(),
                    enabled: true,
                    x: m.rect.x,
                    y: m.rect.y,
                    width: m.rect.width,
                    height: m.rect.height,
                    refresh: 60.0,
                    scale: 1.0,
                    rotation: 0,
                };

                if let Err(e) = randr.apply_monitor(&config) {
                    tracing::error!("failed to revert {}: {}", m.name, e);
                }
            }

            // Restore primary
            if let Some(m) = self.original_monitors.iter().find(|m| m.primary) {
                if let Err(e) = randr.set_primary(&m.name) {
                    tracing::error!("failed to restore primary: {}", e);
                }
            }

            let _ = randr.flush();
        }

        self.set_status("Reverted to original layout");
    }

    /// Set a status message to display temporarily.
    fn set_status(&mut self, message: &str) {
        tracing::info!("{}", message);
        self.status_message = Some((message.to_string(), std::time::Instant::now()));
    }

    /// Save current layout as a profile.
    fn save_profile(&mut self) {
        let primary_name = self.monitor_view.primary_name().map(|s| s.to_string());

        // Build profile from current layout
        let monitors: Vec<MonitorConfig> = self
            .monitor_view
            .monitors()
            .iter()
            .map(|state| MonitorConfig {
                name: state.info.name.clone(),
                enabled: true,
                x: state.real_position.x,
                y: state.real_position.y,
                width: state.info.rect.width,
                height: state.info.rect.height,
                refresh: 60.0,
                scale: 1.0,
                rotation: 0,
            })
            .collect();

        let profile = Profile {
            primary: primary_name,
            monitors,
        };

        // Save to default profile
        self.config
            .profiles
            .insert("default".to_string(), profile);

        match self.config.save() {
            Ok(()) => {
                self.monitor_view.clear_dirty();
                self.set_status(&format!("Saved profile '{}'", self.current_profile));
            }
            Err(e) => {
                tracing::error!("failed to save profile: {}", e);
                self.set_status("Failed to save profile");
            }
        }
    }

    /// Save current layout as a new named profile.
    fn save_profile_as(&mut self, name: &str) {
        let primary_name = self.monitor_view.primary_name().map(|s| s.to_string());

        let monitors: Vec<MonitorConfig> = self
            .monitor_view
            .monitors()
            .iter()
            .map(|state| MonitorConfig {
                name: state.info.name.clone(),
                enabled: true,
                x: state.real_position.x,
                y: state.real_position.y,
                width: state.info.rect.width,
                height: state.info.rect.height,
                refresh: 60.0,
                scale: 1.0,
                rotation: 0,
            })
            .collect();

        let profile = Profile {
            primary: primary_name,
            monitors,
        };

        self.config.profiles.insert(name.to_string(), profile);
        self.config.general.default_profile = name.to_string();
        self.current_profile = name.to_string();

        match self.config.save() {
            Ok(()) => {
                self.monitor_view.clear_dirty();
                self.refresh_profile_list();
                self.set_status(&format!("Created profile '{}'", name));
            }
            Err(e) => {
                tracing::error!("failed to save profile: {}", e);
                self.set_status("Failed to save profile");
            }
        }
    }

    /// Load a profile by name.
    fn load_profile(&mut self, name: &str) {
        if let Some(profile) = self.config.profiles.get(name) {
            Self::apply_profile_to_view(&mut self.monitor_view, profile);
            self.current_profile = name.to_string();
            self.config.general.default_profile = name.to_string();
            let _ = self.config.save(); // Save default profile selection
            self.set_status(&format!("Loaded profile '{}'", name));
        } else {
            self.set_status(&format!("Profile '{}' not found", name));
        }
    }

    /// Rename a profile.
    fn rename_profile(&mut self, old_name: &str, new_name: &str) {
        if old_name == new_name {
            return;
        }

        if let Some(profile) = self.config.profiles.remove(old_name) {
            self.config.profiles.insert(new_name.to_string(), profile);

            // Update current profile if renamed
            if self.current_profile == old_name {
                self.current_profile = new_name.to_string();
                self.config.general.default_profile = new_name.to_string();
            }

            match self.config.save() {
                Ok(()) => {
                    self.refresh_profile_list();
                    self.set_status(&format!("Renamed '{}' to '{}'", old_name, new_name));
                }
                Err(e) => {
                    tracing::error!("failed to save after rename: {}", e);
                    self.set_status("Failed to rename profile");
                }
            }
        }
    }

    /// Delete a profile.
    fn delete_profile(&mut self, name: &str) {
        // Don't delete if it's the only profile
        if self.config.profiles.len() <= 1 {
            self.set_status("Cannot delete the only profile");
            return;
        }

        self.config.profiles.remove(name);

        // Switch to another profile if we deleted the current one
        if self.current_profile == name {
            if let Some(other_name) = self.config.profiles.keys().next() {
                let other = other_name.clone();
                self.load_profile(&other);
            }
        }

        match self.config.save() {
            Ok(()) => {
                self.refresh_profile_list();
                self.set_status(&format!("Deleted profile '{}'", name));
            }
            Err(e) => {
                tracing::error!("failed to save after delete: {}", e);
                self.set_status("Failed to delete profile");
            }
        }
    }

    /// Refresh the profile dropdown list.
    fn refresh_profile_list(&mut self) {
        let names: Vec<String> = self.config.profiles.keys().cloned().collect();
        self.dropdown_profiles.set_items(names);
        self.dropdown_profiles.set_selected_by_name(&self.current_profile);
    }

    /// Handle window resize.
    fn handle_resize(&mut self, size: Size) {
        tracing::debug!("resize to {}x{}", size.width, size.height);

        // Resize renderer
        if let Err(e) = self.renderer.resize(size.width, size.height) {
            tracing::error!("failed to resize renderer: {}", e);
            return;
        }

        // Update monitor view rect
        let view_rect = Rect::new(0, 0, size.width, size.height.saturating_sub(100));
        self.monitor_view.set_view_rect(view_rect);

        // Reposition widgets
        let controls_y = size.height.saturating_sub(100) as i32;

        // Buttons on the left
        self.btn_apply.set_position(10, controls_y + 60);
        self.btn_revert.set_position(90, controls_y + 60);
        self.btn_save.set_position(170, controls_y + 60);

        // Dropdown and Save As on the right
        self.dropdown_profiles.set_position(size.width as i32 - 260, controls_y + 60);
        self.btn_save_as.set_position(size.width as i32 - 100, controls_y + 60);

        // Reposition save-as input if active (next to dropdown)
        if let Some(ref mut input) = self.save_as_input {
            input.set_position(size.width as i32 - 260, controls_y + 25);
        }
    }

    /// Render the application.
    fn render(&mut self) -> Result<()> {
        // Clear background
        self.renderer.clear()?;

        // Render monitor view
        self.monitor_view.render(&mut self.renderer, &self.theme)?;

        // Render bottom controls area
        let size = self.renderer.size();
        let controls_y = size.height.saturating_sub(100) as i32;
        let controls_rect = Rect::new(0, controls_y, size.width, 100);
        self.renderer
            .fill_rect(controls_rect, self.theme.background)?;

        // Separator line
        self.renderer.line(
            0.0,
            controls_y as f64,
            size.width as f64,
            controls_y as f64,
            self.theme.border,
            1.0,
        )?;

        // Left side: Instructions and status
        self.renderer.text_default(
            "Drag monitors to arrange | Double-click: set primary",
            10.0,
            (controls_y + 15) as f64,
            self.theme.foreground,
        )?;

        // Keyboard shortcuts hint
        self.renderer.text_default(
            "Ctrl+S: Save | Ctrl+A: Apply | Ctrl+R: Revert",
            10.0,
            (controls_y + 35) as f64,
            self.theme.item_description,
        )?;

        // Status message (show for 3 seconds) - after the left buttons
        if let Some((ref msg, instant)) = self.status_message {
            if instant.elapsed().as_secs() < 3 {
                let color = if msg.contains("error") || msg.contains("Failed") {
                    Color::new(1.0, 0.4, 0.4, 1.0) // Red
                } else {
                    Color::new(0.4, 0.8, 0.4, 1.0) // Green
                };
                self.renderer
                    .text_default(msg, 250.0, (controls_y + 70) as f64, color)?;
            }
        }

        // Profile label and dirty indicator (above dropdown on right)
        if self.monitor_view.is_dirty() {
            // Show "Profile: * unsaved" when dirty
            self.renderer.text_default(
                "Profile:",
                (size.width - 260) as f64,
                (controls_y + 45) as f64,
                self.theme.item_description,
            )?;
            self.renderer.text_default(
                "* unsaved",
                (size.width - 200) as f64,
                (controls_y + 45) as f64,
                Color::new(1.0, 0.7, 0.3, 1.0), // Orange
            )?;
        } else {
            self.renderer.text_default(
                "Profile:",
                (size.width - 260) as f64,
                (controls_y + 45) as f64,
                self.theme.item_description,
            )?;
        }

        // Render dropdown
        self.dropdown_profiles.render(&self.renderer, &self.theme)?;

        // Render save-as input if active (over dropdown area)
        if let Some(ref input) = self.save_as_input {
            input.render(&self.renderer, &self.theme)?;
        }

        // Render buttons
        self.btn_apply.render(&self.renderer, &self.theme)?;
        self.btn_revert.render(&self.renderer, &self.theme)?;
        self.btn_save.render(&self.renderer, &self.theme)?;
        self.btn_save_as.render(&self.renderer, &self.theme)?;

        // Blit to window
        copy_surface_to_window(self.renderer.surface_mut(), &self.window, self.gc, 0, 0)?;

        Ok(())
    }
}
