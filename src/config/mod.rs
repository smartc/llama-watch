pub mod models;

use anyhow::{Context, Result};
use models::AppConfig;
use std::path::Path;
use tokio::fs;

const CONFIG_FILE: &str = "config.json";

pub async fn load_config() -> Result<AppConfig> {
    let path = Path::new(CONFIG_FILE);

    if !path.exists() {
        let config = AppConfig::new();
        save_config(&config).await?;
        return Ok(config);
    }

    let content = fs::read_to_string(path)
        .await
        .context("Failed to read config file")?;

    let config: AppConfig = serde_json::from_str(&content)
        .context("Failed to parse config file")?;

    config.validate()
        .context("Config validation failed")?;

    Ok(config)
}

pub async fn save_config(config: &AppConfig) -> Result<()> {
    config.validate()
        .context("Config validation failed")?;

    let content = serde_json::to_string_pretty(config)
        .context("Failed to serialize config")?;

    fs::write(CONFIG_FILE, content)
        .await
        .context("Failed to write config file")?;

    Ok(())
}
