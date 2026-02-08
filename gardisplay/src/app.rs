//! Main application state and event loop.

use anyhow::Result;
use gartk_core::{Color, InputEvent, Key, Rect, Size, Theme};
use gartk_render::{copy_surface_to_window, Renderer};
use gartk_x11::{
    detect_monitors, primary_monitor, Connection, EventLoop, EventLoopConfig, Monitor, Window,
    WindowConfig,
};
use x11rb::protocol::xproto::ConnectionExt;

use crate::config::{Config, MonitorConfig};
use crate::randr::RandrManager;
use crate::ui::{EventResult, MonitorView};

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
    #[allow(dead_code)] // Used for profile management
    config: Config,
    monitor_view: MonitorView,
    randr: Option<RandrManager>,
    original_monitors: Vec<Monitor>,
    demo_mode: bool,
    status_message: Option<(String, std::time::Instant)>,
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
        })
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

        // Instructions
        self.renderer.text_default(
            "Drag monitors to arrange | Double-click to set primary",
            10.0,
            (controls_y + 20) as f64,
            self.theme.foreground,
        )?;

        // Keyboard shortcuts
        self.renderer.text_default(
            "Enter: Apply | Backspace: Revert | Q/Esc: Quit",
            10.0,
            (controls_y + 40) as f64,
            self.theme.item_description,
        )?;

        // Status message (show for 3 seconds)
        if let Some((ref msg, instant)) = self.status_message {
            if instant.elapsed().as_secs() < 3 {
                // Green for success, theme color otherwise
                let color = if msg.contains("error") {
                    Color::new(1.0, 0.4, 0.4, 1.0) // Red
                } else {
                    Color::new(0.4, 0.8, 0.4, 1.0) // Green
                };
                self.renderer
                    .text_default(msg, 10.0, (controls_y + 70) as f64, color)?;
            }
        }

        // Dirty indicator
        if self.monitor_view.is_dirty() {
            self.renderer.text_default(
                "(unsaved changes)",
                (size.width - 150) as f64,
                (controls_y + 20) as f64,
                Color::new(1.0, 0.7, 0.3, 1.0), // Orange
            )?;
        }

        // Blit to window
        copy_surface_to_window(self.renderer.surface_mut(), &self.window, self.gc, 0, 0)?;

        Ok(())
    }
}
