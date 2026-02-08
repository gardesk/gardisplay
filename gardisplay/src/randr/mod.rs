//! RandR module for querying and applying display configurations.

mod error;
mod manager;
mod types;

#[allow(unused_imports)] // These will be used for mode selection UI
pub use error::RandrError;
pub use manager::RandrManager;
#[allow(unused_imports)] // These will be used for mode selection UI
pub use types::{ModeInfo, OutputInfo};
