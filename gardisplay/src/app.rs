//! Main application state and event loop.

use std::process::Child;

use anyhow::Result;
use gartk_core::{Color, InputEvent, Key, Rect, Size, Theme};
use gartk_render::{copy_surface_to_window, Renderer};
use gartk_x11::{
    detect_monitors, primary_monitor, Connection, CursorManager, CursorShape, EventLoop,
    EventLoopConfig, Monitor, Window, WindowConfig,
};
use x11rb::protocol::xproto::ConnectionExt;

use crate::config::{Config, MonitorConfig, Profile};
use crate::randr::{ModeInfo, OutputInfo, RandrManager};
use crate::ui::{
    Button, ConfirmOverlay, ConfirmResult, DisplayPanel, DisplayPanelResult, Dropdown,
    DropdownAction, EventResult, MonitorView, TextInput,
};
use crate::watchdog;

/// Window dimensions.
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

/// Footer height in pixels.
const FOOTER_HEIGHT: u32 = 100;

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
    // Display panel
    display_panel: DisplayPanel,
    randr_outputs: Vec<OutputInfo>,
    last_selected: Option<usize>,
    // Cursor management
    cursor_manager: CursorManager,
    current_cursor: CursorShape,
    // UI widgets
    current_profile: String,
    dropdown_profiles: Dropdown,
    btn_apply: Button,
    btn_revert: Button,
    btn_save: Button,
    btn_save_as: Button,
    save_as_input: Option<TextInput>,
    // Confirmation state for display changes
    confirm_overlay: Option<ConfirmOverlay>,
    pre_change_config: Option<Vec<MonitorConfig>>,
    // Watchdog process for auto-revert (independent of main process)
    #[allow(dead_code)] // Child kept alive for watchdog process
    watchdog_child: Option<Child>,
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

        // Create cursor manager
        let cursor_manager = CursorManager::new(conn.clone())?;

        // Calculate layout: monitor view (2/3) + display panel (1/3) + footer
        let view_area = WINDOW_HEIGHT - FOOTER_HEIGHT;
        let panel_height = view_area / 3;
        let monitor_view_height = view_area - panel_height;

        // Create monitor view (top 2/3)
        let view_rect = Rect::new(0, 0, WINDOW_WIDTH, monitor_view_height);
        let mut monitor_view = MonitorView::new(view_rect);

        // Create display panel (bottom 1/3 of view area)
        let panel_rect = Rect::new(0, monitor_view_height as i32, WINDOW_WIDTH, panel_height);
        let display_panel = DisplayPanel::new(panel_rect);

        // Create RandR manager (only in non-demo mode)
        let (randr, randr_outputs) = if demo {
            (None, Self::demo_outputs())
        } else {
            match RandrManager::new(conn.clone()) {
                Ok(r) => {
                    let outputs = r.get_outputs().unwrap_or_default();
                    tracing::debug!("RandR: found {} outputs", outputs.len());
                    for o in &outputs {
                        tracing::debug!(
                            "  {} connected={} modes={} current={:?}",
                            o.name,
                            o.connected,
                            o.modes.len(),
                            o.current_mode.as_ref().map(|m| format!("{}x{}@{:.0}Hz", m.width, m.height, m.refresh))
                        );
                    }
                    (Some(r), outputs)
                }
                Err(e) => {
                    tracing::warn!("failed to create RandR manager: {}", e);
                    (None, Vec::new())
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
        let controls_y = (WINDOW_HEIGHT - FOOTER_HEIGHT) as i32;

        // Buttons on the left
        let btn_apply = Button::new(10, controls_y + 60, 70, 32, "Apply");
        let btn_revert = Button::new(90, controls_y + 60, 70, 32, "Revert");
        let btn_save = Button::new(170, controls_y + 60, 60, 32, "Save");

        // Profile dropdown and Save As on the right
        let mut dropdown_profiles = Dropdown::new(WINDOW_WIDTH as i32 - 210, controls_y + 60, 100, 32);
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
            display_panel,
            randr_outputs,
            last_selected: None,
            cursor_manager,
            current_cursor: CursorShape::Default,
            current_profile,
            dropdown_profiles,
            btn_apply,
            btn_revert,
            btn_save,
            btn_save_as,
            save_as_input: None,
            confirm_overlay: None,
            pre_change_config: None,
            watchdog_child: None,
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

        // Check if the profile's monitor set matches the current monitors
        let current_monitors: Vec<&str> = view.monitors().iter().map(|s| s.info.name.as_str()).collect();
        let profile_monitors: Vec<&str> = profile.monitors.iter().map(|m| m.name.as_str()).collect();

        let monitors_match = current_monitors.len() == profile_monitors.len()
            && current_monitors.iter().all(|m| profile_monitors.contains(m));

        if !monitors_match {
            tracing::warn!(
                "profile monitors {:?} don't match current monitors {:?}, skipping position loading",
                profile_monitors,
                current_monitors
            );
            // Still set primary if specified
            if let Some(ref primary) = profile.primary {
                view.set_primary(primary);
            }
            view.recalculate_layout();
            return;
        }

        // Update monitor positions from profile
        for state in view.monitors_mut() {
            if let Some(config) = config_map.get(state.info.name.as_str()) {
                // Sanity check: don't apply positions that are clearly wrong
                // (negative positions or positions way outside screen bounds)
                if config.x >= 0 && config.y >= 0 && config.x < 10000 && config.y < 10000 {
                    state.real_position = gartk_core::Point::new(config.x, config.y);
                    tracing::debug!(
                        "loaded {} at ({}, {})",
                        state.info.name,
                        config.x,
                        config.y
                    );
                } else {
                    tracing::warn!(
                        "ignoring invalid position ({}, {}) for {}",
                        config.x,
                        config.y,
                        state.info.name
                    );
                }
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
                rect: Rect::new(0, 0, 2880, 1800),
                primary: true,
                width_mm: 301,
                height_mm: 188,
            },
            gartk_x11::Monitor {
                name: "HDMI-1".to_string(),
                rect: Rect::new(2880, 0, 1920, 1080),
                primary: false,
                width_mm: 530,
                height_mm: 300,
            },
            gartk_x11::Monitor {
                name: "DP-1".to_string(),
                rect: Rect::new(2880, 1080, 2560, 1440),
                primary: false,
                width_mm: 597,
                height_mm: 336,
            },
        ]
    }

    /// Create demo RandR outputs with available modes for UI testing.
    fn demo_outputs() -> Vec<OutputInfo> {
        vec![
            OutputInfo {
                name: "eDP-1".to_string(),
                output: 0,
                crtc: Some(0),
                connected: true,
                modes: vec![
                    ModeInfo { id: 1, width: 2880, height: 1800, refresh: 60.0 },
                    ModeInfo { id: 2, width: 2560, height: 1600, refresh: 60.0 },
                    ModeInfo { id: 3, width: 1920, height: 1200, refresh: 60.0 },
                    ModeInfo { id: 4, width: 1920, height: 1080, refresh: 60.0 },
                    ModeInfo { id: 5, width: 1680, height: 1050, refresh: 60.0 },
                    ModeInfo { id: 6, width: 1440, height: 900, refresh: 60.0 },
                    ModeInfo { id: 7, width: 1280, height: 800, refresh: 60.0 },
                ],
                current_mode: Some(ModeInfo { id: 1, width: 2880, height: 1800, refresh: 60.0 }),
                position: Some((0, 0)),
                width_mm: 301,
                height_mm: 188,
            },
            OutputInfo {
                name: "HDMI-1".to_string(),
                output: 1,
                crtc: Some(1),
                connected: true,
                modes: vec![
                    ModeInfo { id: 10, width: 3840, height: 2160, refresh: 60.0 },
                    ModeInfo { id: 11, width: 3840, height: 2160, refresh: 30.0 },
                    ModeInfo { id: 12, width: 2560, height: 1440, refresh: 60.0 },
                    ModeInfo { id: 13, width: 1920, height: 1080, refresh: 120.0 },
                    ModeInfo { id: 14, width: 1920, height: 1080, refresh: 60.0 },
                    ModeInfo { id: 15, width: 1920, height: 1080, refresh: 30.0 },
                    ModeInfo { id: 16, width: 1280, height: 720, refresh: 60.0 },
                ],
                current_mode: Some(ModeInfo { id: 14, width: 1920, height: 1080, refresh: 60.0 }),
                position: Some((2880, 0)),
                width_mm: 530,
                height_mm: 300,
            },
            OutputInfo {
                name: "DP-1".to_string(),
                output: 2,
                crtc: Some(2),
                connected: true,
                modes: vec![
                    ModeInfo { id: 20, width: 2560, height: 1440, refresh: 144.0 },
                    ModeInfo { id: 21, width: 2560, height: 1440, refresh: 120.0 },
                    ModeInfo { id: 22, width: 2560, height: 1440, refresh: 60.0 },
                    ModeInfo { id: 23, width: 1920, height: 1080, refresh: 144.0 },
                    ModeInfo { id: 24, width: 1920, height: 1080, refresh: 120.0 },
                    ModeInfo { id: 25, width: 1920, height: 1080, refresh: 60.0 },
                    ModeInfo { id: 26, width: 1280, height: 720, refresh: 60.0 },
                ],
                current_mode: Some(ModeInfo { id: 22, width: 2560, height: 1440, refresh: 60.0 }),
                position: Some((2880, 1080)),
                width_mm: 597,
                height_mm: 336,
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
        // Handle confirmation overlay first (blocks other input)
        if let Some(ref mut overlay) = self.confirm_overlay {
            match overlay.handle_event(event) {
                ConfirmResult::Confirmed => {
                    self.confirm_changes();
                    return EventResult::Redraw;
                }
                ConfirmResult::Reverted => {
                    self.revert_to_pre_change();
                    return EventResult::Redraw;
                }
                ConfirmResult::Redraw => {
                    return EventResult::Redraw;
                }
                ConfirmResult::None => {
                    // Overlay consumes all events while active
                    return EventResult::None;
                }
            }
        }

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
        let was_expanded = self.dropdown_profiles.is_expanded();
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
        // Check if dropdown state changed (expand/collapse toggle)
        if was_expanded != self.dropdown_profiles.is_expanded() {
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
            let controls_y = size.height.saturating_sub(FOOTER_HEIGHT) as i32;
            let mut input = TextInput::new(size.width as i32 - 260, controls_y + 25, 150, 32);
            input.set_placeholder("Profile name");
            input.set_active(true);
            self.save_as_input = Some(input);
            return EventResult::Redraw;
        }

        // Handle display panel events
        match self.display_panel.handle_event(event) {
            DisplayPanelResult::ConfigChanged(config) => {
                if let Some(name) = self.display_panel.selected_output() {
                    self.monitor_view.update_monitor_config(
                        name,
                        config.width,
                        config.height,
                        config.refresh,
                        config.rotation,
                        config.scale,
                        config.enabled,
                    );
                }
                return EventResult::Redraw;
            }
            DisplayPanelResult::Redraw => return EventResult::Redraw,
            DisplayPanelResult::None => {}
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
            _ => {
                let result = self.monitor_view.handle_event(event);
                // Check if selection changed and sync with display panel
                let current_selected = self.monitor_view.selected();
                if current_selected != self.last_selected {
                    self.last_selected = current_selected;
                    self.sync_panel_selection();
                }
                // Update cursor based on hover/drag state
                self.update_cursor();
                result
            }
        }
    }

    /// Update the cursor based on monitor view state.
    fn update_cursor(&mut self) {
        let shape = self.monitor_view.cursor_shape();
        if shape != self.current_cursor {
            self.current_cursor = shape;
            if let Err(e) = self.cursor_manager.set_window_cursor(self.window.id(), shape) {
                tracing::debug!("failed to set cursor: {}", e);
            }
        }
    }

    /// Refresh RandR outputs from the display server.
    fn refresh_randr_outputs(&mut self) {
        if self.demo_mode {
            return; // Demo mode uses static outputs
        }

        if let Some(ref randr) = self.randr {
            match randr.get_outputs() {
                Ok(outputs) => {
                    tracing::debug!("refresh_randr_outputs: found {} outputs", outputs.len());
                    for o in &outputs {
                        tracing::debug!(
                            "  {} connected={} modes={} current={:?}",
                            o.name,
                            o.connected,
                            o.modes.len(),
                            o.current_mode.as_ref().map(|m| format!("{}x{}@{:.0}Hz", m.width, m.height, m.refresh))
                        );
                    }
                    self.randr_outputs = outputs;
                }
                Err(e) => {
                    tracing::warn!("failed to refresh RandR outputs: {}", e);
                }
            }
        }
    }

    /// Sync the display panel with the selected monitor.
    fn sync_panel_selection(&mut self) {
        // Refresh outputs to get current mode information
        self.refresh_randr_outputs();

        if let Some(state) = self.monitor_view.selected_monitor() {
            // Find matching RandR output (may be None in demo mode)
            let output = self.randr_outputs.iter().find(|o| o.name == state.info.name);
            tracing::debug!(
                "sync_panel_selection: monitor={} output_found={} modes={}",
                state.info.name,
                output.is_some(),
                output.map(|o| o.modes.len()).unwrap_or(0)
            );

            self.display_panel.set_selected_monitor(
                Some(&state.info.name),
                output,
                state.info.rect.width,
                state.info.rect.height,
                state.refresh,
                state.rotation,
                state.scale,
                state.enabled,
            );
        } else {
            self.display_panel
                .set_selected_monitor(None, None, 0, 0, 60.0, 0, 1.0, true);
        }
    }

    /// Capture the current RandR state for potential revert.
    fn capture_current_randr_state(&self) -> Option<Vec<MonitorConfig>> {
        let randr = self.randr.as_ref()?;
        let outputs = randr.get_outputs().ok()?;

        Some(
            outputs
                .iter()
                .filter(|o| o.connected && o.current_mode.is_some())
                .map(|o| {
                    let mode = o.current_mode.as_ref().unwrap();
                    let pos = o.position.unwrap_or((0, 0));
                    MonitorConfig {
                        name: o.name.clone(),
                        enabled: true,
                        x: pos.0 as i32,
                        y: pos.1 as i32,
                        width: mode.width as u32,
                        height: mode.height as u32,
                        refresh: mode.refresh,
                        scale: 1.0,
                        rotation: 0, // TODO: capture actual rotation
                    }
                })
                .collect(),
        )
    }

    /// Apply the current layout via RandR with confirmation.
    fn apply_layout(&mut self) {
        if self.demo_mode {
            self.set_status("Demo mode - changes not applied");
            return;
        }

        let Some(ref randr) = self.randr else {
            self.set_status("RandR not available");
            return;
        };

        // Capture current state before applying changes
        self.pre_change_config = self.capture_current_randr_state();

        // Build MonitorConfig from current view state
        let primary_name = self.monitor_view.primary_name().map(|s| s.to_string());
        let configs: Vec<MonitorConfig> = self
            .monitor_view
            .monitors()
            .iter()
            .map(|state| MonitorConfig {
                name: state.info.name.clone(),
                enabled: state.enabled,
                x: state.real_position.x,
                y: state.real_position.y,
                width: state.info.rect.width,
                height: state.info.rect.height,
                refresh: state.refresh,
                scale: state.scale,
                rotation: state.rotation,
            })
            .collect();

        // IMPORTANT: Prepare the screen size BEFORE applying any configurations
        // This is essential for rotation changes which may require a larger virtual screen
        if let Err(e) = randr.prepare_screen_for_configs(&configs) {
            tracing::error!("failed to prepare screen size: {}", e);
            self.set_status(&format!("Failed to prepare screen: {}", e));
            return;
        }

        let mut success_count = 0;
        let mut error_count = 0;

        for config in &configs {
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

        // Try to shrink screen to fit (non-fatal if it fails)
        if let Err(e) = randr.shrink_screen_to_fit() {
            tracing::debug!("shrink_screen_to_fit failed (non-fatal): {}", e);
        }

        if error_count > 0 {
            self.set_status(&format!(
                "Applied {} monitor(s), {} error(s)",
                success_count, error_count
            ));
            // Don't show confirmation on error, revert immediately
            self.revert_to_pre_change();
            return;
        }

        // Start the watchdog process for auto-revert
        // This is more robust than relying on the event loop, which may crash
        // if the display change causes X connection issues
        if let Some(ref pre_config) = self.pre_change_config {
            match watchdog::start_watchdog(pre_config) {
                Ok(child) => {
                    tracing::info!("started watchdog process for auto-revert");
                    self.watchdog_child = Some(child);
                }
                Err(e) => {
                    tracing::error!("failed to start watchdog: {}", e);
                    // Continue anyway - we still have the in-process timeout
                }
            }
        }

        // Show confirmation overlay
        let size = self.window.size();
        let window_rect = Rect::new(0, 0, size.width, size.height);
        self.confirm_overlay = Some(ConfirmOverlay::new(window_rect));
        self.set_status("Confirm display settings or they will revert...");
    }

    /// Confirm the applied changes (user accepted).
    fn confirm_changes(&mut self) {
        self.confirm_overlay = None;
        self.pre_change_config = None;

        // Cancel the watchdog by removing its config file
        watchdog::cancel_watchdog();
        self.watchdog_child = None;

        // Update original_monitors to current state
        let primary_name = self.monitor_view.primary_name().map(|s| s.to_string());
        self.original_monitors = self
            .monitor_view
            .monitors()
            .iter()
            .map(|state| Monitor {
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
            })
            .collect();

        self.set_status("Display settings confirmed");
        self.monitor_view.clear_dirty();
    }

    /// Revert to pre-change configuration.
    fn revert_to_pre_change(&mut self) {
        self.confirm_overlay = None;

        // Cancel the watchdog - we're reverting manually
        watchdog::cancel_watchdog();
        self.watchdog_child = None;

        if let Some(ref configs) = self.pre_change_config.take() {
            if let Some(ref randr) = self.randr {
                // IMPORTANT: Prepare screen size BEFORE reverting
                // The pre-change config may have different dimensions than current
                if let Err(e) = randr.prepare_screen_for_configs(configs) {
                    tracing::error!("failed to prepare screen for revert: {}", e);
                    // Continue anyway - we still want to try to revert
                }

                for config in configs {
                    if let Err(e) = randr.apply_monitor(config) {
                        tracing::error!("failed to revert {}: {}", config.name, e);
                    }
                }
                let _ = randr.flush();

                // Try to shrink screen to fit
                if let Err(e) = randr.shrink_screen_to_fit() {
                    tracing::debug!("shrink_screen_to_fit failed during revert: {}", e);
                }
            }
        }

        // Reset view to original monitors
        self.monitor_view.set_monitors(self.original_monitors.clone());
        self.set_status("Display settings reverted");
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

        // Calculate new layout
        let view_area = size.height.saturating_sub(FOOTER_HEIGHT);
        let panel_height = view_area / 3;
        let monitor_view_height = view_area - panel_height;

        // Update monitor view rect (top 2/3)
        let view_rect = Rect::new(0, 0, size.width, monitor_view_height);
        self.monitor_view.set_view_rect(view_rect);

        // Update display panel rect (bottom 1/3 of view area)
        let panel_rect = Rect::new(0, monitor_view_height as i32, size.width, panel_height);
        self.display_panel.set_rect(panel_rect);

        // Reposition footer widgets
        let controls_y = size.height.saturating_sub(FOOTER_HEIGHT) as i32;

        // Buttons on the left
        self.btn_apply.set_position(10, controls_y + 60);
        self.btn_revert.set_position(90, controls_y + 60);
        self.btn_save.set_position(170, controls_y + 60);

        // Dropdown and Save As on the right
        self.dropdown_profiles.set_position(size.width as i32 - 210, controls_y + 60);
        self.btn_save_as.set_position(size.width as i32 - 100, controls_y + 60);

        // Reposition save-as input if active (above dropdown)
        if let Some(ref mut input) = self.save_as_input {
            input.set_position(size.width as i32 - 210, controls_y + 25);
        }

        // Update confirmation overlay rect if active
        if let Some(ref mut overlay) = self.confirm_overlay {
            overlay.set_rect(Rect::new(0, 0, size.width, size.height));
        }
    }

    /// Render the application.
    fn render(&mut self) -> Result<()> {
        // Clear background
        self.renderer.clear()?;

        // Render monitor view
        self.monitor_view.render(&mut self.renderer, &self.theme)?;

        // Render display panel
        self.display_panel.render(&self.renderer, &self.theme)?;

        // Render bottom controls area (footer)
        let size = self.renderer.size();
        let controls_y = size.height.saturating_sub(FOOTER_HEIGHT) as i32;
        let controls_rect = Rect::new(0, controls_y, size.width, FOOTER_HEIGHT);
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

        // Unsaved indicator (top right corner of footer)
        if self.monitor_view.is_dirty() {
            self.renderer.text_default(
                "* unsaved",
                (size.width - 70) as f64,
                (controls_y + 15) as f64,
                Color::new(1.0, 0.7, 0.3, 1.0), // Orange
            )?;
        }

        // Profile label (above dropdown on right)
        self.renderer.text_default(
            "Profile:",
            (size.width - 210) as f64,
            (controls_y + 45) as f64,
            self.theme.item_description,
        )?;

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

        // Render confirmation overlay (on top of everything)
        if let Some(ref overlay) = self.confirm_overlay {
            overlay.render(&self.renderer, &self.theme)?;
        }

        // Blit to window
        copy_surface_to_window(self.renderer.surface_mut(), &self.window, self.gc, 0, 0)?;

        Ok(())
    }
}
