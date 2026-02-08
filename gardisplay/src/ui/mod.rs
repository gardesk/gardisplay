//! UI components for gardisplay.

mod monitor_view;
pub mod widgets;

pub use monitor_view::MonitorView;
pub use widgets::{Button, Dropdown, DropdownAction, TextInput};

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
