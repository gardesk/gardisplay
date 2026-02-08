//! IPC types for gardisplay daemon communication.

use serde::{Deserialize, Serialize};

/// Request from client to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Re-detect connected monitors.
    Detect,
    /// Apply a profile (uses default if None).
    Apply { profile: Option<String> },
    /// List available profile names.
    ListProfiles,
    /// Get current profile name.
    GetProfile,
    /// Switch to a named profile.
    SetProfile { name: String },
    /// Set brightness (0.0 - 1.0).
    SetBrightness { value: f64 },
    /// Set gamma (0.5 - 2.0).
    SetGamma { value: f64 },
    /// Control night mode.
    NightMode { action: NightModeAction },
    /// Get full current state.
    GetState,
}

/// Night mode control action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NightModeAction {
    On,
    Off,
    Toggle,
}

/// Response from daemon to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Error { message: String },
    Profile { name: String },
    Profiles { names: Vec<String> },
    State(DisplayState),
}

/// Current display state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayState {
    pub current_profile: String,
    pub monitors: Vec<MonitorInfo>,
    pub brightness: f64,
    pub gamma: f64,
    pub night_mode_active: bool,
}

/// Monitor information for IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Event sent to gar window manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    LayoutChanged { monitors: Vec<MonitorInfo> },
}
