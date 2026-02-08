//! Configuration loading and types.

mod types;

pub use types::*;

use std::path::PathBuf;

/// Load configuration from file or default location.
pub fn load_config(path: Option<&str>) -> anyhow::Result<Config> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => config_path()?,
    };

    if path.exists() {
        tracing::info!("loading config from {}", path.display());
        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    } else {
        tracing::info!("no config file found, using defaults");
        Ok(Config::default())
    }
}

/// Get default config file path.
pub fn config_path() -> anyhow::Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?;
    Ok(config_dir.join("gardisplay").join("config.toml"))
}

impl Config {
    /// Save configuration to file.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;

        tracing::info!("saved config to {}", path.display());
        Ok(())
    }
}
