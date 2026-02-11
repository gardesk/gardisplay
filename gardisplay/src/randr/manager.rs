//! RandR manager for querying and applying display configurations.

use gartk_x11::Connection;
use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};
use x11rb::protocol::render;
use x11rb::protocol::xproto::ConnectionExt as XprotoExt;

use super::error::{RandrError, Result};
use super::types::{ModeInfo, OutputInfo};
use crate::config::MonitorConfig;

/// Convert a floating-point value to X11 Fixed (16.16 fixed-point).
fn float_to_fixed(value: f64) -> render::Fixed {
    (value * 65536.0) as i32
}

/// Create an identity transform matrix (no transformation).
fn identity_transform() -> render::Transform {
    render::Transform {
        matrix11: float_to_fixed(1.0),
        matrix12: float_to_fixed(0.0),
        matrix13: float_to_fixed(0.0),
        matrix21: float_to_fixed(0.0),
        matrix22: float_to_fixed(1.0),
        matrix23: float_to_fixed(0.0),
        matrix31: float_to_fixed(0.0),
        matrix32: float_to_fixed(0.0),
        matrix33: float_to_fixed(1.0),
    }
}

/// Create a scaling transform matrix.
/// For RandR CRTC transforms, `scale` is the UI scale factor (e.g., 2.0 = "2x bigger").
/// The transform uses the inverse: 1/scale on the diagonal.
fn scale_transform(scale: f64) -> render::Transform {
    let factor = 1.0 / scale;
    render::Transform {
        matrix11: float_to_fixed(factor),
        matrix12: float_to_fixed(0.0),
        matrix13: float_to_fixed(0.0),
        matrix21: float_to_fixed(0.0),
        matrix22: float_to_fixed(factor),
        matrix23: float_to_fixed(0.0),
        matrix31: float_to_fixed(0.0),
        matrix32: float_to_fixed(0.0),
        matrix33: float_to_fixed(1.0),
    }
}

/// Manager for RandR operations.
pub struct RandrManager {
    conn: Connection,
    root: u32,
}

impl RandrManager {
    /// Create a new RandR manager.
    pub fn new(conn: Connection) -> Result<Self> {
        let root = conn.root();

        // Verify RandR is available
        conn.inner()
            .randr_query_version(1, 5)?
            .reply()?;

        Ok(Self { conn, root })
    }

    /// Get screen resources (outputs, CRTCs, modes).
    fn get_resources(&self) -> Result<randr::GetScreenResourcesCurrentReply> {
        Ok(self
            .conn
            .inner()
            .randr_get_screen_resources_current(self.root)?
            .reply()?)
    }

    /// Get information about all outputs.
    #[allow(dead_code)] // Used for mode selection UI
    pub fn get_outputs(&self) -> Result<Vec<OutputInfo>> {
        let resources = self.get_resources()?;
        tracing::debug!(
            "get_outputs: resources has {} modes available, {} outputs",
            resources.modes.len(),
            resources.outputs.len()
        );

        let mut outputs = Vec::new();

        for &output in &resources.outputs {
            let info = self
                .conn
                .inner()
                .randr_get_output_info(output, resources.config_timestamp)?
                .reply()?;

            let name = String::from_utf8_lossy(&info.name).to_string();
            let connected = info.connection == randr::Connection::CONNECTED;

            tracing::debug!(
                "  output '{}': {} mode IDs in info.modes",
                name,
                info.modes.len()
            );

            // Get available modes
            let modes: Vec<ModeInfo> = info
                .modes
                .iter()
                .filter_map(|&mode_id| {
                    let result = self.get_mode_info(&resources, mode_id);
                    if result.is_none() {
                        tracing::debug!("    mode_id {} not found in resources.modes", mode_id);
                    }
                    result
                })
                .collect();

            tracing::debug!("    resolved {} modes for '{}'", modes.len(), name);

            // Get current mode and position if CRTC is set
            let (crtc, current_mode, position) = if info.crtc != 0 {
                let crtc_info = self
                    .conn
                    .inner()
                    .randr_get_crtc_info(info.crtc, resources.config_timestamp)?
                    .reply()?;

                let current = if crtc_info.mode != 0 {
                    self.get_mode_info(&resources, crtc_info.mode)
                } else {
                    None
                };

                (
                    Some(info.crtc),
                    current,
                    Some((crtc_info.x, crtc_info.y)),
                )
            } else {
                (None, None, None)
            };

            outputs.push(OutputInfo {
                name,
                output,
                crtc,
                connected,
                modes,
                current_mode,
                position,
                width_mm: info.mm_width,
                height_mm: info.mm_height,
            });
        }

        Ok(outputs)
    }

    /// Get mode information by ID.
    fn get_mode_info(
        &self,
        resources: &randr::GetScreenResourcesCurrentReply,
        mode_id: randr::Mode,
    ) -> Option<ModeInfo> {
        resources.modes.iter().find(|m| m.id == mode_id).map(|m| {
            let refresh = if m.htotal > 0 && m.vtotal > 0 {
                (m.dot_clock as f64) / (m.htotal as f64 * m.vtotal as f64)
            } else {
                0.0
            };

            ModeInfo {
                id: m.id,
                width: m.width,
                height: m.height,
                refresh,
            }
        })
    }

    /// Get the name of the primary output.
    #[allow(dead_code)] // Used for mode selection UI
    pub fn get_primary_name(&self) -> Result<Option<String>> {
        let reply = self.conn.inner().randr_get_output_primary(self.root)?.reply()?;

        if reply.output == 0 {
            return Ok(None);
        }

        let resources = self.get_resources()?;
        let info = self
            .conn
            .inner()
            .randr_get_output_info(reply.output, resources.config_timestamp)?
            .reply()?;

        Ok(Some(String::from_utf8_lossy(&info.name).to_string()))
    }

    /// Set the primary output by name.
    pub fn set_primary(&self, name: &str) -> Result<()> {
        let output = self.find_output_by_name(name)?;
        self.conn.inner().randr_set_output_primary(self.root, output)?;
        tracing::info!("set primary output to {}", name);
        Ok(())
    }

    /// Find an output by name.
    fn find_output_by_name(&self, name: &str) -> Result<randr::Output> {
        let resources = self.get_resources()?;

        for &output in &resources.outputs {
            let info = self
                .conn
                .inner()
                .randr_get_output_info(output, resources.config_timestamp)?
                .reply()?;

            let output_name = String::from_utf8_lossy(&info.name);
            if output_name == name {
                return Ok(output);
            }
        }

        Err(RandrError::OutputNotFound(name.to_string()))
    }

    /// Ensure the virtual screen is large enough to contain the given bounds.
    /// This must be called before applying configurations that might exceed the current screen size.
    pub fn ensure_screen_size(&self, required_width: u32, required_height: u32) -> Result<()> {
        // Use root window geometry to get the actual current virtual screen size.
        // GetScreenInfo returns discrete "advertised" sizes which may not reflect
        // the current virtual size set by RRSetScreenSize.
        let geom = self
            .conn
            .inner()
            .get_geometry(self.root)?
            .reply()
            .map_err(|e| RandrError::ConfigFailed(format!("get_geometry: {}", e)))?;

        let current_size = (geom.width as u32, geom.height as u32);

        tracing::debug!(
            "ensure_screen_size: current={}x{}, required={}x{}",
            current_size.0,
            current_size.1,
            required_width,
            required_height
        );

        // Only resize if needed
        if current_size.0 >= required_width && current_size.1 >= required_height {
            return Ok(());
        }

        let new_width = required_width.max(current_size.0);
        let new_height = required_height.max(current_size.1);

        // Calculate physical size in mm (approximate based on 96 DPI)
        let mm_width = (new_width as f64 * 25.4 / 96.0) as u32;
        let mm_height = (new_height as f64 * 25.4 / 96.0) as u32;

        tracing::info!(
            "resizing virtual screen to {}x{} ({}x{}mm)",
            new_width,
            new_height,
            mm_width,
            mm_height
        );

        self.conn.inner().randr_set_screen_size(
            self.root,
            new_width as u16,
            new_height as u16,
            mm_width,
            mm_height,
        )?;

        self.conn.inner().flush()?;
        Ok(())
    }

    /// Calculate the effective dimensions after rotation.
    fn rotated_dimensions(width: u32, height: u32, rotation: u32) -> (u32, u32) {
        match rotation {
            90 | 270 => (height, width), // Swap dimensions for 90/270 rotation
            _ => (width, height),        // 0 or 180 keeps same dimensions
        }
    }

    /// Calculate the effective desktop dimensions after rotation and scaling.
    /// The CRTC transform scales the output, so the framebuffer (what apps see)
    /// is the mode resolution divided by the scale factor.
    /// e.g., 2880x1800 at scale 2.0 → effective 1440x900 framebuffer.
    fn effective_dimensions(width: u32, height: u32, rotation: u32, scale: f64) -> (u32, u32) {
        let (rot_w, rot_h) = Self::rotated_dimensions(width, height, rotation);
        if (scale - 1.0).abs() < 0.001 {
            (rot_w, rot_h)
        } else {
            let eff_w = (rot_w as f64 / scale).round() as u32;
            let eff_h = (rot_h as f64 / scale).round() as u32;
            (eff_w.max(1), eff_h.max(1))
        }
    }

    /// Calculate the required screen size to contain all given monitor configurations.
    pub fn calculate_required_screen_size(configs: &[MonitorConfig]) -> (u32, u32) {
        let mut max_x = 0u32;
        let mut max_y = 0u32;

        for config in configs {
            if !config.enabled {
                continue;
            }

            // Framebuffer size is based on mode resolution, not scaled
            let (eff_width, eff_height) = Self::effective_dimensions(
                config.width,
                config.height,
                config.rotation,
                config.scale,
            );

            let right = config.x.max(0) as u32 + eff_width;
            let bottom = config.y.max(0) as u32 + eff_height;

            max_x = max_x.max(right);
            max_y = max_y.max(bottom);
        }

        // Ensure minimum screen size
        (max_x.max(320), max_y.max(200))
    }

    /// Prepare the screen for a set of monitor configurations.
    /// This should be called before applying any configurations to ensure the screen is large enough.
    #[allow(dead_code)] // Available but apply_layout now uses disable→resize→apply pattern
    pub fn prepare_screen_for_configs(&self, configs: &[MonitorConfig]) -> Result<()> {
        let (required_width, required_height) = Self::calculate_required_screen_size(configs);
        tracing::info!(
            "preparing screen for {} monitors: required size {}x{}",
            configs.iter().filter(|c| c.enabled).count(),
            required_width,
            required_height
        );
        self.ensure_screen_size(required_width, required_height)
    }

    /// Read the current CRTC transform scale factor.
    /// Returns 1.0 if no transform or identity transform.
    pub fn get_crtc_scale(&self, crtc: randr::Crtc) -> f64 {
        let Ok(cookie) = self.conn.inner().randr_get_crtc_transform(crtc) else {
            return 1.0;
        };
        let Ok(reply) = cookie.reply() else {
            return 1.0;
        };

        // The current transform matrix11 is 1/scale in 16.16 fixed-point
        let matrix11 = reply.current_transform.matrix11;
        if matrix11 <= 0 {
            return 1.0;
        }

        let factor = matrix11 as f64 / 65536.0;
        if (factor - 1.0).abs() < 0.001 {
            1.0
        } else {
            1.0 / factor // Convert back to UI scale
        }
    }

    /// Shrink the screen to the minimum size required for the current outputs.
    /// Call this after applying all configurations to clean up excess virtual screen space.
    /// Accounts for CRTC transforms (scaling) when computing effective dimensions.
    pub fn shrink_screen_to_fit(&self) -> Result<()> {
        let outputs = self.get_outputs()?;

        let mut max_x = 0u32;
        let mut max_y = 0u32;

        for output in &outputs {
            if !output.connected {
                continue;
            }
            if let (Some(mode), Some(pos)) = (&output.current_mode, output.position) {
                // Check if this output has a scale transform
                let scale = output.crtc.map(|c| self.get_crtc_scale(c)).unwrap_or(1.0);
                let (eff_w, eff_h) =
                    Self::effective_dimensions(mode.width as u32, mode.height as u32, 0, scale);
                let right = pos.0.max(0) as u32 + eff_w;
                let bottom = pos.1.max(0) as u32 + eff_h;
                max_x = max_x.max(right);
                max_y = max_y.max(bottom);
            }
        }

        // Ensure minimum size
        let target_width = max_x.max(320);
        let target_height = max_y.max(200);

        let mm_width = (target_width as f64 * 25.4 / 96.0) as u32;
        let mm_height = (target_height as f64 * 25.4 / 96.0) as u32;

        tracing::info!(
            "shrinking virtual screen to {}x{} ({}x{}mm)",
            target_width,
            target_height,
            mm_width,
            mm_height
        );

        self.conn.inner().randr_set_screen_size(
            self.root,
            target_width as u16,
            target_height as u16,
            mm_width,
            mm_height,
        )?;

        self.conn.inner().flush()?;
        Ok(())
    }

    /// Apply a monitor configuration.
    pub fn apply_monitor(&self, config: &MonitorConfig) -> Result<()> {
        if !config.enabled {
            return self.disable_output(&config.name);
        }

        let resources = self.get_resources()?;
        let output = self.find_output_by_name(&config.name)?;

        let output_info = self
            .conn
            .inner()
            .randr_get_output_info(output, resources.config_timestamp)?
            .reply()?;

        // Find matching mode
        let mode_id = self.find_mode_id(&resources, &output_info.modes, config)?;

        // Find available CRTC
        let crtc = self.find_available_crtc(&resources, &output_info, output)?;

        // Calculate effective dimensions after rotation
        // (scale is handled by transform, not mode resolution)
        let (eff_width, eff_height) = Self::effective_dimensions(
            config.width,
            config.height,
            config.rotation,
            config.scale,
        );

        // Calculate required screen size to contain this monitor
        let required_width = config.x.max(0) as u32 + eff_width;
        let required_height = config.y.max(0) as u32 + eff_height;

        tracing::debug!(
            "apply_monitor {}: {}x{} rot={} scale={:.2} -> effective {}x{} at ({}, {})",
            config.name,
            config.width,
            config.height,
            config.rotation,
            config.scale,
            eff_width,
            eff_height,
            config.x,
            config.y
        );

        // Ensure screen is large enough BEFORE applying the CRTC config
        self.ensure_screen_size(required_width, required_height)?;

        // Re-fetch resources after screen resize (timestamps may have changed)
        let resources = self.get_resources()?;

        // Convert rotation
        let rotation = match config.rotation {
            90 => randr::Rotation::ROTATE90,
            180 => randr::Rotation::ROTATE180,
            270 => randr::Rotation::ROTATE270,
            _ => randr::Rotation::ROTATE0,
        };

        // Set CRTC transform BEFORE set_crtc_config.
        // Per the RandR spec, SetCrtcTransform stores a "pending" transform.
        // The next SetCrtcConfig call activates it. So we must set the transform first.
        if (config.scale - 1.0).abs() > 0.001 {
            let transform = scale_transform(config.scale);
            let filter = if (config.scale.round() - config.scale).abs() < 0.001 {
                b"nearest".as_slice() // Integer scale: pixel-perfect
            } else {
                b"bilinear".as_slice() // Fractional scale: smooth interpolation
            };
            self.conn
                .inner()
                .randr_set_crtc_transform(crtc, transform, filter, &[])?;
            tracing::info!(
                "set pending CRTC transform {:.2}x for {} (filter={})",
                config.scale,
                config.name,
                String::from_utf8_lossy(filter)
            );
        } else {
            // Reset to identity transform
            let transform = identity_transform();
            self.conn
                .inner()
                .randr_set_crtc_transform(crtc, transform, b"nearest", &[])?;
        }

        // Apply configuration — this activates the pending transform
        let result = self
            .conn
            .inner()
            .randr_set_crtc_config(
                crtc,
                resources.timestamp,
                resources.config_timestamp,
                config.x as i16,
                config.y as i16,
                mode_id,
                rotation,
                &[output],
            )?
            .reply()?;

        if result.status != randr::SetConfig::SUCCESS {
            return Err(RandrError::ConfigFailed(format!(
                "set_crtc_config returned {:?}",
                result.status
            )));
        }

        tracing::info!(
            "applied config for {}: {}x{} rot={} scale={} at ({}, {})",
            config.name,
            config.width,
            config.height,
            config.rotation,
            config.scale,
            config.x,
            config.y
        );

        Ok(())
    }

    /// Disable an output.
    pub fn disable_output(&self, name: &str) -> Result<()> {
        let resources = self.get_resources()?;
        let output = self.find_output_by_name(name)?;

        let output_info = self
            .conn
            .inner()
            .randr_get_output_info(output, resources.config_timestamp)?
            .reply()?;

        if output_info.crtc == 0 {
            // Already disabled
            return Ok(());
        }

        // Disable by setting mode to 0
        self.conn
            .inner()
            .randr_set_crtc_config(
                output_info.crtc,
                resources.timestamp,
                resources.config_timestamp,
                0,
                0,
                0, // mode 0 = disable
                randr::Rotation::ROTATE0,
                &[],
            )?
            .reply()?;

        tracing::info!("disabled output {}", name);
        Ok(())
    }

    /// Find a matching mode ID for the given config.
    fn find_mode_id(
        &self,
        resources: &randr::GetScreenResourcesCurrentReply,
        output_modes: &[randr::Mode],
        config: &MonitorConfig,
    ) -> Result<randr::Mode> {
        for &mode_id in output_modes {
            if let Some(mode) = self.get_mode_info(resources, mode_id) {
                if mode.width as u32 == config.width
                    && mode.height as u32 == config.height
                    && (mode.refresh - config.refresh).abs() < 1.0
                {
                    return Ok(mode_id);
                }
            }
        }

        // Fallback: find mode with matching resolution (any refresh)
        for &mode_id in output_modes {
            if let Some(mode) = self.get_mode_info(resources, mode_id) {
                if mode.width as u32 == config.width && mode.height as u32 == config.height {
                    tracing::warn!(
                        "exact refresh rate not found, using {}x{}@{:.1}Hz",
                        mode.width,
                        mode.height,
                        mode.refresh
                    );
                    return Ok(mode_id);
                }
            }
        }

        Err(RandrError::ModeNotFound {
            width: config.width,
            height: config.height,
            refresh: config.refresh,
        })
    }

    /// Find an available CRTC for the output.
    fn find_available_crtc(
        &self,
        resources: &randr::GetScreenResourcesCurrentReply,
        output_info: &randr::GetOutputInfoReply,
        output: randr::Output,
    ) -> Result<randr::Crtc> {
        // First, check if output already has a CRTC
        if output_info.crtc != 0 {
            return Ok(output_info.crtc);
        }

        // Find a free CRTC that the output can use
        for &crtc in &output_info.crtcs {
            let crtc_info = self
                .conn
                .inner()
                .randr_get_crtc_info(crtc, resources.config_timestamp)?
                .reply()?;

            // CRTC is free if it has no outputs
            if crtc_info.outputs.is_empty() {
                return Ok(crtc);
            }
        }

        // Try to find any CRTC we can use
        for &crtc in &resources.crtcs {
            let crtc_info = self
                .conn
                .inner()
                .randr_get_crtc_info(crtc, resources.config_timestamp)?
                .reply()?;

            if crtc_info.outputs.is_empty() && crtc_info.possible.contains(&output) {
                return Ok(crtc);
            }
        }

        let name = String::from_utf8_lossy(&output_info.name).to_string();
        Err(RandrError::NoCrtcAvailable(name))
    }

    /// Set the screen to an exact size.
    /// All CRTCs should be disabled first to avoid validation failures.
    #[allow(dead_code)] // Available for direct screen resize
    pub fn resize_screen(&self, width: u32, height: u32) -> Result<()> {
        let mm_width = (width as f64 * 25.4 / 96.0) as u32;
        let mm_height = (height as f64 * 25.4 / 96.0) as u32;

        tracing::info!(
            "resizing screen to {}x{} ({}x{}mm)",
            width,
            height,
            mm_width,
            mm_height
        );

        self.conn.inner().randr_set_screen_size(
            self.root,
            width as u16,
            height as u16,
            mm_width,
            mm_height,
        )?;
        self.conn.inner().flush()?;
        Ok(())
    }

    /// Disable all connected outputs (set CRTCs to mode 0).
    #[allow(dead_code)] // Available for screen resize operations
    pub fn disable_all_crtcs(&self) -> Result<()> {
        let resources = self.get_resources()?;

        for &crtc in &resources.crtcs {
            let crtc_info = self
                .conn
                .inner()
                .randr_get_crtc_info(crtc, resources.config_timestamp)?
                .reply()?;

            // Only disable active CRTCs (those with a mode set)
            if crtc_info.mode != 0 {
                self.conn
                    .inner()
                    .randr_set_crtc_config(
                        crtc,
                        resources.timestamp,
                        resources.config_timestamp,
                        0,
                        0,
                        0, // mode 0 = disable
                        randr::Rotation::ROTATE0,
                        &[],
                    )?
                    .reply()?;
            }
        }

        self.conn.inner().flush()?;
        tracing::debug!("disabled all active CRTCs");
        Ok(())
    }

    /// Flush pending X11 requests.
    pub fn flush(&self) -> Result<()> {
        self.conn.inner().flush()?;
        Ok(())
    }
}
