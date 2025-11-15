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
    /// Last known safe state before monitor reconfiguration
    /// Holds true if we were safe, false otherwise. None means never determined.
    last_stable_safe_state: Option<bool>,
}

impl MonitorState {
    pub fn new() -> Self {
        Self {
            mqtt_manager: MqttMonitorManager::new(),
            alpaca_manager: AlpacaMonitorManager::new(),
            last_stable_safe_state: None,
        }
    }

    pub async fn initialize(&mut self, config: &AppConfig) -> Result<()> {
        // Initialize MQTT monitors (only enabled ones)
        for (_, mqtt_config) in &config.mqtt_monitors {
            if !mqtt_config.enabled {
                continue; // Skip disabled monitors
            }
            if let Some(server_config) = config.mqtt_servers.get(&mqtt_config.server_id) {
                self.mqtt_manager
                    .add_monitor(mqtt_config.clone(), server_config.clone())
                    .await?;
            }
        }

        // Initialize ASCOM Alpaca monitors (only enabled ones)
        for (_, alpaca_config) in &config.alpaca_monitors {
            if !alpaca_config.enabled {
                continue; // Skip disabled monitors
            }
            self.alpaca_manager
                .add_monitor(alpaca_config.clone())
                .await?;
        }

        Ok(())
    }

    pub async fn reload(&mut self, config: &AppConfig) -> Result<()> {
        // Capture current stable state before reload (if all monitors have data)
        if self.all_monitors_have_data() {
            let current_safe = self.is_safe_ignoring_pending();
            self.last_stable_safe_state = Some(current_safe);
        }

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
            // No monitors configured/enabled - cannot determine safety, report unsafe
            return false;
        }

        // Check if any monitors are still initializing (no data yet)
        let has_initializing = all_statuses.iter().any(|s| s.last_update.is_none());

        if has_initializing {
            // If we have monitors that are initializing, use graceful handling
            // Check the monitors that DO have data
            let monitors_with_data: Vec<_> = all_statuses
                .iter()
                .filter(|s| s.last_update.is_some())
                .collect();

            if monitors_with_data.is_empty() {
                // All monitors are initializing, return last stable state or default unsafe
                // (We can't claim it's safe if we have no data)
                return self.last_stable_safe_state.unwrap_or(false);
            }

            // Check if any monitor with data is unsafe
            let any_unsafe_with_data = monitors_with_data.iter().any(|s| !s.is_safe || s.error.is_some());

            if any_unsafe_with_data {
                // We have real unsafe data, report unsafe immediately
                return false;
            }

            // All monitors with data are safe, but some are still initializing
            // Hold the last stable state or default to unsafe (conservative approach)
            return self.last_stable_safe_state.unwrap_or(false);
        }

        // All monitors have data - calculate normal safety
        let is_safe = all_statuses.iter().all(|status| {
            status.is_safe && status.error.is_none()
        });

        // This is now a stable state, we can update our last known state
        // (This would ideally be done in a separate method, but we're in a read-only context)

        is_safe
    }

    /// Check safety ignoring monitors that are pending (used during reload)
    fn is_safe_ignoring_pending(&self) -> bool {
        let all_statuses = self.get_all_statuses();

        if all_statuses.is_empty() {
            return false; // No monitors = unsafe
        }

        // Only check monitors that have data
        let monitors_with_data: Vec<_> = all_statuses
            .iter()
            .filter(|s| s.last_update.is_some())
            .collect();

        if monitors_with_data.is_empty() {
            // No data yet from any monitors - can't determine safety
            // This would only happen during initial startup, defer to last_stable_safe_state
            return self.last_stable_safe_state.unwrap_or(false);
        }

        monitors_with_data.iter().all(|status| {
            status.is_safe && status.error.is_none()
        })
    }

    /// Check if all configured monitors have received at least one update
    fn all_monitors_have_data(&self) -> bool {
        let all_statuses = self.get_all_statuses();
        !all_statuses.is_empty() && all_statuses.iter().all(|s| s.last_update.is_some())
    }

    /// Get structured safety comments about unsafe monitors
    pub fn get_safety_comments(&self) -> Option<crate::alpaca::models::SafetyComments> {
        use crate::alpaca::models::{MonitorComment, SafetyComments};

        let all_statuses = self.get_all_statuses();

        if all_statuses.is_empty() {
            return None;
        }

        let mut unsafe_monitors = Vec::new();

        for status in all_statuses {
            if let Some(error) = &status.error {
                unsafe_monitors.push(MonitorComment {
                    monitor: status.name.clone(),
                    reason: format!("Error - {}", error),
                    value: status.current_value,
                    threshold: status.threshold,
                });
            } else if !status.is_safe {
                unsafe_monitors.push(MonitorComment {
                    monitor: status.name.clone(),
                    reason: "Unsafe condition".to_string(),
                    value: status.current_value,
                    threshold: status.threshold,
                });
            }
        }

        if unsafe_monitors.is_empty() {
            None
        } else {
            Some(SafetyComments { unsafe_monitors })
        }
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
