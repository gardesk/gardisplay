//! UI components for gardisplay.

mod confirm_overlay;
mod display_panel;
mod monitor_view;
pub mod widgets;

pub use confirm_overlay::{ConfirmOverlay, ConfirmResult};
pub use display_panel::{DisplayPanel, DisplayPanelConfig, DisplayPanelResult};
pub use monitor_view::MonitorView;
pub use widgets::{Button, Dropdown, DropdownAction, TextInput, Toggle};

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
