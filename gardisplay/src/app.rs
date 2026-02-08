//! Main application state and event loop.

use anyhow::Result;
use gartk_core::{InputEvent, Key, Rect, Size, Theme};
use gartk_render::{copy_surface_to_window, Renderer};
use gartk_x11::{
    detect_monitors, primary_monitor, Connection, EventLoop, EventLoopConfig, Window, WindowConfig,
};
use x11rb::protocol::xproto::ConnectionExt;

use crate::config::Config;
use crate::ui::{EventResult, MonitorView};

/// Window dimensions.
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

/// Main application.
pub struct App {
    #[allow(dead_code)] // Used in Sprint 3 for RandR operations
    conn: Connection,
    window: Window,
    renderer: Renderer,
    theme: Theme,
    gc: u32,
    #[allow(dead_code)] // Used in Sprint 3 for profile management
    config: Config,
    monitor_view: MonitorView,
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
        monitor_view.set_monitors(monitors);

        Ok(Self {
            conn,
            window,
            renderer,
            theme,
            gc,
            config,
            monitor_view,
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
            InputEvent::Resize { width, height } => {
                self.handle_resize(Size::new(*width, *height));
                EventResult::Redraw
            }
            InputEvent::Expose => EventResult::Redraw,
            _ => self.monitor_view.handle_event(event),
        }
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

        // Title in controls area
        self.renderer.text_default(
            "gardisplay - Drag monitors to arrange",
            10.0,
            (controls_y + 20) as f64,
            self.theme.foreground,
        )?;

        // Blit to window
        copy_surface_to_window(self.renderer.surface_mut(), &self.window, self.gc, 0, 0)?;

        Ok(())
    }
}
