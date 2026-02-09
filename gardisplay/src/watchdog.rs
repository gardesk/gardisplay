//! Watchdog process for auto-reverting display changes.
//!
//! This module spawns a separate process that will revert display changes
//! if not canceled within a timeout. This is more robust than relying on the
//! main process's event loop, which may crash or lose its X connection when
//! display settings change.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::config::MonitorConfig;

/// Timeout in seconds for auto-revert.
const REVERT_TIMEOUT_SECS: u32 = 15;

/// Get the path to the revert config file.
fn revert_config_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("gardisplay-revert.json")
}

/// Start the watchdog process that will revert display settings after timeout.
/// Returns the child process handle if successful.
pub fn start_watchdog(configs: &[MonitorConfig]) -> std::io::Result<Child> {
    let config_path = revert_config_path();

    // Serialize the revert config to JSON
    let json = serde_json::to_string(configs).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;

    // Write to temp file
    let mut file = fs::File::create(&config_path)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;

    tracing::info!(
        "watchdog: wrote revert config to {:?} ({} monitors)",
        config_path,
        configs.len()
    );

    // Get the DISPLAY environment variable
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());

    // Spawn the watchdog script as a detached process
    // We use bash to create a simple watchdog that:
    // 1. Sleeps for the timeout
    // 2. Checks if the config file still exists
    // 3. If so, applies xrandr commands to revert
    // 4. Deletes the config file
    let script = format!(
        r#"
        sleep {timeout}
        if [ -f "{config_path}" ]; then
            echo "gardisplay watchdog: reverting display settings..."
            {xrandr_commands}
            rm -f "{config_path}"
            echo "gardisplay watchdog: revert complete"
        fi
        "#,
        timeout = REVERT_TIMEOUT_SECS,
        config_path = config_path.display(),
        xrandr_commands = generate_xrandr_commands(configs),
    );

    tracing::debug!("watchdog script:\n{}", script);

    let child = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("DISPLAY", display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    tracing::info!("watchdog: started with PID {}", child.id());
    Ok(child)
}

/// Cancel the watchdog by deleting the config file.
/// The watchdog process will check for this and exit without reverting.
pub fn cancel_watchdog() {
    let config_path = revert_config_path();
    if config_path.exists() {
        if let Err(e) = fs::remove_file(&config_path) {
            tracing::warn!("watchdog: failed to remove config file: {}", e);
        } else {
            tracing::info!("watchdog: canceled (removed config file)");
        }
    }
}

/// Generate xrandr commands to restore the given configurations.
fn generate_xrandr_commands(configs: &[MonitorConfig]) -> String {
    let mut commands = Vec::new();

    for config in configs {
        if !config.enabled {
            commands.push(format!("xrandr --output {} --off", config.name));
            continue;
        }

        let rotation = match config.rotation {
            90 => "left",
            180 => "inverted",
            270 => "right",
            _ => "normal",
        };

        // Always include scale to reset any transforms
        // scale 1x1 resets to identity transform
        let scale = if (config.scale - 1.0).abs() > 0.001 {
            config.scale
        } else {
            1.0
        };

        commands.push(format!(
            "xrandr --output {} --mode {}x{} --pos {}x{} --rotate {} --scale {}x{}",
            config.name,
            config.width,
            config.height,
            config.x,
            config.y,
            rotation,
            scale,
            scale
        ));
    }

    commands.join("\n            ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_xrandr_commands() {
        let configs = vec![
            MonitorConfig {
                name: "eDP-1".to_string(),
                enabled: true,
                x: 0,
                y: 0,
                width: 2880,
                height: 1800,
                refresh: 60.0,
                scale: 1.0,
                rotation: 0,
            },
            MonitorConfig {
                name: "HDMI-1".to_string(),
                enabled: true,
                x: 2880,
                y: 0,
                width: 1920,
                height: 1080,
                refresh: 60.0,
                scale: 1.0,
                rotation: 90,
            },
        ];

        let commands = generate_xrandr_commands(&configs);
        assert!(commands.contains("xrandr --output eDP-1 --mode 2880x1800 --pos 0x0 --rotate normal"));
        assert!(commands.contains("xrandr --output HDMI-1 --mode 1920x1080 --pos 2880x0 --rotate left"));
    }
}
