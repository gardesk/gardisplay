//! RandR manager for querying and applying display configurations.

use gartk_x11::Connection;
use x11rb::connection::Connection as X11Connection;
use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};

use super::error::{RandrError, Result};
use super::types::{ModeInfo, OutputInfo};
use crate::config::MonitorConfig;

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
        let mut outputs = Vec::new();

        for &output in &resources.outputs {
            let info = self
                .conn
                .inner()
                .randr_get_output_info(output, resources.config_timestamp)?
                .reply()?;

            let name = String::from_utf8_lossy(&info.name).to_string();
            let connected = info.connection == randr::Connection::CONNECTED;

            // Get available modes
            let modes: Vec<ModeInfo> = info
                .modes
                .iter()
                .filter_map(|&mode_id| self.get_mode_info(&resources, mode_id))
                .collect();

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

        // Convert rotation
        let rotation = match config.rotation {
            90 => randr::Rotation::ROTATE90,
            180 => randr::Rotation::ROTATE180,
            270 => randr::Rotation::ROTATE270,
            _ => randr::Rotation::ROTATE0,
        };

        // Apply configuration
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
            "applied config for {}: {}x{} at ({}, {})",
            config.name,
            config.width,
            config.height,
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

    /// Flush pending X11 requests.
    pub fn flush(&self) -> Result<()> {
        self.conn.inner().flush()?;
        Ok(())
    }
}
