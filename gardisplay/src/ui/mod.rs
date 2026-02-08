//! UI components for gardisplay.

mod monitor_view;

pub use monitor_view::MonitorView;

/// Result of handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// No action needed.
    None,
    /// Redraw the UI.
    Redraw,
    /// Quit the application.
    Quit,
}
