//! DPI scaling for X11.
//!
//! X11 doesn't have native HiDPI scaling like Wayland. Instead, scaling is achieved by:
//! 1. Setting Xft.dpi via X resources (affects fonts and DPI-aware apps)
//! 2. Environment variables (GDK_SCALE, QT_SCALE_FACTOR) for toolkit-specific scaling
//!
//! Note: DPI changes typically require applications to be restarted to take effect.

use std::process::Command;

/// Base DPI (96 is the X11 standard).
const BASE_DPI: u32 = 96;

/// Apply DPI scaling via xrdb.
/// Scale of 1.0 = 96 DPI, scale of 2.0 = 192 DPI, etc.
pub fn apply_dpi_scale(scale: f64) -> std::io::Result<()> {
    let dpi = (BASE_DPI as f64 * scale).round() as u32;

    tracing::info!("setting Xft.dpi to {} (scale={})", dpi, scale);

    // Build the xrdb resource string
    let resource = format!("Xft.dpi: {}\n", dpi);

    // Apply via xrdb -merge
    let output = Command::new("xrdb")
        .arg("-merge")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("xrdb failed: {}", stderr);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("xrdb failed: {}", stderr),
        ));
    }

    // Also write to stdin
    let mut child = Command::new("xrdb")
        .arg("-merge")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(resource.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "xrdb -merge failed",
        ));
    }

    tracing::info!("DPI set to {} - apps may need restart to reflect changes", dpi);
    Ok(())
}

/// Get the current DPI setting.
pub fn get_current_dpi() -> Option<u32> {
    let output = Command::new("xrdb")
        .arg("-query")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("Xft.dpi:") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() == 2 {
                return parts[1].trim().parse().ok();
            }
        }
    }

    None
}

/// Get the current scale factor based on DPI.
pub fn get_current_scale() -> f64 {
    get_current_dpi()
        .map(|dpi| dpi as f64 / BASE_DPI as f64)
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpi_calculation() {
        // scale 1.0 = 96 DPI
        assert_eq!((BASE_DPI as f64 * 1.0).round() as u32, 96);
        // scale 1.5 = 144 DPI
        assert_eq!((BASE_DPI as f64 * 1.5).round() as u32, 144);
        // scale 2.0 = 192 DPI
        assert_eq!((BASE_DPI as f64 * 2.0).round() as u32, 192);
    }
}
