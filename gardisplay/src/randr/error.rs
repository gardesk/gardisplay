//! RandR error types.

use thiserror::Error;

/// Errors that can occur during RandR operations.
#[derive(Debug, Error)]
pub enum RandrError {
    #[error("output not found: {0}")]
    OutputNotFound(String),

    #[error("mode not found: {width}x{height}@{refresh:.1}Hz")]
    ModeNotFound {
        width: u32,
        height: u32,
        refresh: f64,
    },

    #[error("no available CRTC for output {0}")]
    NoCrtcAvailable(String),

    #[error("configuration failed: {0}")]
    ConfigFailed(String),

    #[error("X11 connection error: {0}")]
    Connection(#[from] x11rb::errors::ConnectionError),

    #[error("X11 reply error: {0}")]
    Reply(#[from] x11rb::errors::ReplyError),
}

pub type Result<T> = std::result::Result<T, RandrError>;
