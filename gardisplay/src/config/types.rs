//! Configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub effects: EffectsConfig,
    #[serde(default)]
    pub night_mode: NightModeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            profiles: HashMap::new(),
            effects: EffectsConfig::default(),
            night_mode: NightModeConfig::default(),
        }
    }
}

/// General settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_profile_name")]
    pub default_profile: String,
}

fn default_profile_name() -> String {
    "default".to_string()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_profile: default_profile_name(),
        }
    }
}

/// A named monitor profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub primary: Option<String>,
    #[serde(default)]
    pub monitors: Vec<MonitorConfig>,
}

/// Per-monitor configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_refresh")]
    pub refresh: f64,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub rotation: u32,
}

fn default_true() -> bool {
    true
}

fn default_refresh() -> f64 {
    60.0
}

fn default_scale() -> f64 {
    1.0
}

/// Display effects settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectsConfig {
    #[serde(default = "default_brightness")]
    pub brightness: f64,
    #[serde(default = "default_gamma")]
    pub gamma: f64,
}

fn default_brightness() -> f64 {
    1.0
}

fn default_gamma() -> f64 {
    1.0
}

impl Default for EffectsConfig {
    fn default() -> Self {
        Self {
            brightness: default_brightness(),
            gamma: default_gamma(),
        }
    }
}

/// Night mode settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NightModeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_schedule")]
    pub schedule: String,
    #[serde(default = "default_start_time")]
    pub start_time: String,
    #[serde(default = "default_end_time")]
    pub end_time: String,
    #[serde(default = "default_temperature")]
    pub temperature: u32,
    #[serde(default = "default_transition")]
    pub transition_minutes: u32,
}

fn default_schedule() -> String {
    "sunset".to_string()
}

fn default_start_time() -> String {
    "20:00".to_string()
}

fn default_end_time() -> String {
    "06:00".to_string()
}

fn default_temperature() -> u32 {
    4500
}

fn default_transition() -> u32 {
    30
}

impl Default for NightModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: default_schedule(),
            start_time: default_start_time(),
            end_time: default_end_time(),
            temperature: default_temperature(),
            transition_minutes: default_transition(),
        }
    }
}
