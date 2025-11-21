use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    Equal,
}

impl ComparisonOperator {
    pub fn compare(&self, value: f64, threshold: f64) -> bool {
        match self {
            ComparisonOperator::GreaterThan => value > threshold,
            ComparisonOperator::LessThan => value < threshold,
            ComparisonOperator::Equal => (value - threshold).abs() < f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttServerConfig {
    pub id: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttMonitorConfig {
    pub id: String,
    pub name: String,
    pub server_id: String,
    pub topic: String,
    #[serde(default)]
    pub json_path: Option<String>, // e.g., "$.Volts.1" or "Volts.1", or None for raw numeric values
    pub threshold: f64,
    pub operator: ComparisonOperator,
    pub safe_when_true: bool, // if true, safe when comparison is true; if false, unsafe when true
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64, // Mark unsafe if no data received in this many seconds (default 300)
    #[serde(default)]
    pub hold_time_seconds: u64, // Minimum time a condition must hold before changing state (0 = immediate)
    #[serde(default = "default_enabled")]
    pub enabled: bool, // If false, monitor is ignored
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlpacaMonitorConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub device_type: String, // e.g., "safetymonitor", "switch", etc.
    pub device_number: u32,
    pub property: String, // e.g., "issafe", custom property path
    pub threshold: f64,
    pub operator: ComparisonOperator,
    pub safe_when_true: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64, // Mark unsafe if no data received in this many seconds (default 300)
    #[serde(default)]
    pub hold_time_seconds: u64, // Minimum time a condition must hold before changing state (0 = immediate)
    #[serde(default = "default_enabled")]
    pub enabled: bool, // If false, monitor is ignored
}

fn default_timeout() -> u64 {
    300
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub mqtt_servers: HashMap<String, MqttServerConfig>,
    #[serde(default)]
    pub mqtt_monitors: HashMap<String, MqttMonitorConfig>,
    #[serde(default)]
    pub alpaca_monitors: HashMap<String, AlpacaMonitorConfig>,
    #[serde(default)]
    pub server_port: u16,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub logging_enabled: bool,
}

impl Default for ComparisonOperator {
    fn default() -> Self {
        ComparisonOperator::GreaterThan
    }
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            mqtt_servers: HashMap::new(),
            mqtt_monitors: HashMap::new(),
            alpaca_monitors: HashMap::new(),
            server_port: 8080,
            device_name: "LLAMA Safety Monitor".to_string(),
            location: String::new(),
            logging_enabled: false,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate that all MQTT monitors reference valid servers
        for monitor in self.mqtt_monitors.values() {
            if !self.mqtt_servers.contains_key(&monitor.server_id) {
                return Err(anyhow::anyhow!(
                    "MQTT monitor '{}' references non-existent server '{}'",
                    monitor.id, monitor.server_id
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorStatus {
    pub id: String,
    pub name: String,
    pub monitor_type: String,
    pub is_safe: bool,
    pub current_value: Option<f64>,
    pub threshold: f64,
    pub last_update: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
    pub raw_payload: Option<String>,
    pub enabled: bool,
    pub hold_time_seconds: u64,
    pub pending_is_safe: Option<bool>,
    pub pending_since: Option<chrono::DateTime<chrono::Utc>>,
}
