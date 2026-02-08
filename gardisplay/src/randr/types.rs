//! RandR type definitions.

use x11rb::protocol::randr;

/// Information about a display mode (resolution + refresh rate).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used for mode selection UI
pub struct ModeInfo {
    /// X11 mode ID.
    pub id: randr::Mode,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Refresh rate in Hz.
    pub refresh: f64,
}

#[allow(dead_code)] // Used for mode selection UI
impl ModeInfo {
    /// Format as a human-readable string (e.g., "1920x1080@60Hz").
    pub fn display_string(&self) -> String {
        format!("{}x{}@{:.0}Hz", self.width, self.height, self.refresh)
    }
}

/// Information about an output (monitor connector).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used for mode selection UI
pub struct OutputInfo {
    /// Output name (e.g., "eDP-1", "HDMI-1").
    pub name: String,
    /// X11 output ID.
    pub output: randr::Output,
    /// Currently assigned CRTC (if any).
    pub crtc: Option<randr::Crtc>,
    /// Whether the output is connected.
    pub connected: bool,
    /// Available modes for this output.
    pub modes: Vec<ModeInfo>,
    /// Currently active mode (if any).
    pub current_mode: Option<ModeInfo>,
    /// Current position (if active).
    pub position: Option<(i16, i16)>,
    /// Physical width in mm.
    pub width_mm: u32,
    /// Physical height in mm.
    pub height_mm: u32,
}

#[allow(dead_code)] // Used for mode selection UI
impl OutputInfo {
    /// Check if the output is currently active (has a mode set).
    pub fn is_active(&self) -> bool {
        self.crtc.is_some() && self.current_mode.is_some()
    }

    /// Find a mode matching the given resolution and approximate refresh rate.
    pub fn find_mode(&self, width: u32, height: u32, refresh: f64) -> Option<&ModeInfo> {
        self.modes.iter().find(|m| {
            m.width as u32 == width
                && m.height as u32 == height
                && (m.refresh - refresh).abs() < 1.0
        })
    }

    /// Get the preferred mode (usually the first/native mode).
    pub fn preferred_mode(&self) -> Option<&ModeInfo> {
        self.modes.first()
    }
}
