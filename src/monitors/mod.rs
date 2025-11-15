pub mod alpaca_monitor;
pub mod mqtt_monitor;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::models::{AppConfig, MonitorStatus};
use alpaca_monitor::AlpacaMonitorManager;
use mqtt_monitor::MqttMonitorManager;

pub struct MonitorState {
    mqtt_manager: MqttMonitorManager,
    alpaca_manager: AlpacaMonitorManager,
}

impl MonitorState {
    pub fn new() -> Self {
        Self {
            mqtt_manager: MqttMonitorManager::new(),
            alpaca_manager: AlpacaMonitorManager::new(),
        }
    }

    pub async fn initialize(&mut self, config: &AppConfig) -> Result<()> {
        // Initialize MQTT monitors
        for (_, mqtt_config) in &config.mqtt_monitors {
            if let Some(server_config) = config.mqtt_servers.get(&mqtt_config.server_id) {
                self.mqtt_manager
                    .add_monitor(mqtt_config.clone(), server_config.clone())
                    .await?;
            }
        }

        // Initialize ASCOM Alpaca monitors
        for (_, alpaca_config) in &config.alpaca_monitors {
            self.alpaca_manager
                .add_monitor(alpaca_config.clone())
                .await?;
        }

        Ok(())
    }

    pub async fn reload(&mut self, config: &AppConfig) -> Result<()> {
        // Shutdown all existing monitors
        self.mqtt_manager.shutdown().await;
        self.alpaca_manager.shutdown().await;

        // Clear and reinitialize
        self.mqtt_manager = MqttMonitorManager::new();
        self.alpaca_manager = AlpacaMonitorManager::new();

        // Initialize with new config
        self.initialize(config).await
    }

    pub fn is_safe(&self) -> bool {
        let all_statuses = self.get_all_statuses();

        if all_statuses.is_empty() {
            // No monitors configured - default to safe
            return true;
        }

        // All monitors must be safe and have no errors
        all_statuses.iter().all(|status| {
            status.is_safe && status.error.is_none()
        })
    }

    pub fn get_all_statuses(&self) -> Vec<MonitorStatus> {
        let mut statuses = Vec::new();
        statuses.extend(self.mqtt_manager.get_statuses());
        statuses.extend(self.alpaca_manager.get_statuses());
        statuses
    }

    pub fn get_status(&self, id: &str) -> Option<MonitorStatus> {
        self.mqtt_manager
            .get_status(id)
            .or_else(|| self.alpaca_manager.get_status(id))
    }

    /// Check if monitors are ready (configured and have received at least one value)
    pub fn is_ready(&self) -> bool {
        let statuses = self.get_all_statuses();

        // Must have at least one monitor configured
        if statuses.is_empty() {
            return false;
        }

        // All monitors must have received at least one update
        statuses.iter().all(|status| status.last_update.is_some())
    }
}

pub type SharedMonitorState = Arc<RwLock<MonitorState>>;
