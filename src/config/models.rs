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
    // Safety vs Switch inclusion
    #[serde(default = "default_include_in_safety")]
    pub include_in_safety: bool, // Include this monitor in overall safety evaluation (default true)
    #[serde(default)]
    pub include_in_switch: bool, // Expose this monitor as an ASCOM Switch (default false)
    #[serde(default)]
    pub switch_name: Option<String>, // Display name for the switch (defaults to monitor name)
    #[serde(default)]
    pub switch_timeout_seconds: Option<u64>, // Switch-specific timeout (defaults to timeout_seconds)
    #[serde(default)]
    pub switch_hold_time_seconds: Option<u64>, // Switch-specific hold time (defaults to hold_time_seconds)
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
    // Safety vs Switch inclusion
    #[serde(default = "default_include_in_safety")]
    pub include_in_safety: bool, // Include this monitor in overall safety evaluation (default true)
    #[serde(default)]
    pub include_in_switch: bool, // Expose this monitor as an ASCOM Switch (default false)
    #[serde(default)]
    pub switch_name: Option<String>, // Display name for the switch (defaults to monitor name)
    #[serde(default)]
    pub switch_timeout_seconds: Option<u64>, // Switch-specific timeout (defaults to timeout_seconds)
    #[serde(default)]
    pub switch_hold_time_seconds: Option<u64>, // Switch-specific hold time (defaults to hold_time_seconds)
}

fn default_timeout() -> u64 {
    300
}

fn default_enabled() -> bool {
    true
}

fn default_include_in_safety() -> bool {
    true
}

fn default_server_interface() -> String {
    "127.0.0.1".to_string()
}

fn default_udp_port() -> u16 {
    50222
}

/// Configuration for the ASCOM Switch device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchDeviceConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>, // Device name (defaults to "LLAMA Switch Device")
    #[serde(default)]
    pub description: Option<String>, // Device description
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool, // If true, device starts connected
}

impl Default for SwitchDeviceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: None,
            description: None,
            auto_connect: true,
        }
    }
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
    pub weather_devices: HashMap<u32, WeatherDeviceConfig>, // Key is device_number
    #[serde(default)]
    pub switches: HashMap<String, SwitchConfig>, // Explicit switch definitions with aggregation
    #[serde(default)]
    pub switch_device: SwitchDeviceConfig, // ASCOM Switch device configuration
    #[serde(default)]
    pub server_port: u16,
    #[serde(default = "default_server_interface")]
    pub server_interface: String, // IP address to bind to (e.g., "127.0.0.1" or "0.0.0.0")
    #[serde(default = "default_udp_port")]
    pub tempest_udp_port: u16, // Port for Tempest UDP broadcasts (default 50222)
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
            weather_devices: HashMap::new(),
            switches: HashMap::new(),
            switch_device: SwitchDeviceConfig::default(),
            server_port: 8080,
            server_interface: default_server_interface(),
            tempest_udp_port: default_udp_port(),
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

        // Validate that all switch sources reference valid monitors
        for switch in self.switches.values() {
            if switch.sources.is_empty() {
                return Err(anyhow::anyhow!(
                    "Switch '{}' has no sources defined",
                    switch.id
                ));
            }

            for source_id in &switch.sources {
                let mqtt_exists = self.mqtt_monitors.contains_key(source_id);
                let alpaca_exists = self.alpaca_monitors.contains_key(source_id);
                if !mqtt_exists && !alpaca_exists {
                    return Err(anyhow::anyhow!(
                        "Switch '{}' references non-existent monitor '{}'",
                        switch.id, source_id
                    ));
                }
            }

            // Validate MinOf logic has a reasonable count
            if let SwitchLogic::MinOf { count } = &switch.logic {
                if *count == 0 {
                    return Err(anyhow::anyhow!(
                        "Switch '{}' has min_of count of 0, which would always trigger",
                        switch.id
                    ));
                }
                if *count > switch.sources.len() as u32 {
                    return Err(anyhow::anyhow!(
                        "Switch '{}' has min_of count {} but only {} sources",
                        switch.id, count, switch.sources.len()
                    ));
                }
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
    // Safety/Switch inclusion flags
    pub include_in_safety: bool,
    pub include_in_switch: bool,
    pub switch_name: Option<String>,
    // Switch-specific state (independent of safety state)
    pub switch_is_safe: bool,
    pub switch_timeout_seconds: u64,
    pub switch_hold_time_seconds: u64,
    pub switch_pending_is_safe: Option<bool>,
    pub switch_pending_since: Option<chrono::DateTime<chrono::Utc>>,
    pub switch_error: Option<String>,
}

// Weather device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherDeviceConfig {
    pub device_number: u32,
    pub name: String,
    pub description: String,
    pub source: WeatherDataSource,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool, // If true, device starts in connected state
    #[serde(default)]
    pub safety_thresholds: Option<WeatherSafetyThresholds>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WeatherDataSource {
    Tempest {
        serial_number: Option<String>, // If None, accept any Tempest device
    },
    WeatherUnderground {
        station_id: String,
        api_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeatherSafetyThresholds {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub hold_time_seconds: u64,
    #[serde(default)]
    pub temperature: Option<ThresholdConfig>,
    #[serde(default)]
    pub humidity: Option<ThresholdConfig>,
    #[serde(default)]
    pub sky_brightness: Option<ThresholdConfig>,
    #[serde(default)]
    pub wind_speed: Option<ThresholdConfig>,
    #[serde(default)]
    pub wind_gust: Option<ThresholdConfig>,
    #[serde(default)]
    pub rain_rate: Option<ThresholdConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub threshold: f64,
    pub operator: ComparisonOperator,
    pub safe_when_true: bool,
    #[serde(default)]
    pub timeout_seconds: Option<u64>, // Per-measurement timeout (overrides global if set)
}

fn default_auto_connect() -> bool {
    true
}

/// Logic for combining multiple sources into a single switch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SwitchLogic {
    /// OR logic - switch is triggered if ANY source is triggered (unsafe/true)
    Any,
    /// AND logic - switch is triggered only if ALL sources are triggered (unsafe/true)
    All,
    /// N-out-of-M logic - switch is triggered if at least N sources are triggered
    #[serde(rename = "min_of")]
    MinOf { count: u32 },
}

impl Default for SwitchLogic {
    fn default() -> Self {
        SwitchLogic::Any
    }
}

impl SwitchLogic {
    /// Evaluate the logic given a list of source states (true = triggered/unsafe)
    pub fn evaluate(&self, source_states: &[bool]) -> bool {
        if source_states.is_empty() {
            return false;
        }

        let triggered_count = source_states.iter().filter(|&&s| s).count() as u32;

        match self {
            SwitchLogic::Any => triggered_count > 0,
            SwitchLogic::All => triggered_count == source_states.len() as u32,
            SwitchLogic::MinOf { count } => triggered_count >= *count,
        }
    }

    /// Get a human-readable description of the logic
    pub fn description(&self) -> String {
        match self {
            SwitchLogic::Any => "ANY source triggered".to_string(),
            SwitchLogic::All => "ALL sources triggered".to_string(),
            SwitchLogic::MinOf { count } => format!("at least {} sources triggered", count),
        }
    }
}

/// Configuration for an explicit switch that aggregates multiple monitor sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// List of monitor IDs to aggregate (can reference MQTT or Alpaca monitors)
    pub sources: Vec<String>,
    /// How to combine source states
    #[serde(default)]
    pub logic: SwitchLogic,
    /// Timeout for the aggregated switch (sources without data within this time are considered unsafe)
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// Hold time for state transitions (prevents flapping)
    #[serde(default)]
    pub hold_time_seconds: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Status of an aggregated switch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchStatus {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Current state: true = triggered (unsafe condition detected)
    pub is_triggered: bool,
    /// The logic used to combine sources
    pub logic: SwitchLogic,
    /// Status of each source
    pub source_states: Vec<SwitchSourceStatus>,
    /// Pending state change (during hold time)
    pub pending_is_triggered: Option<bool>,
    pub pending_since: Option<chrono::DateTime<chrono::Utc>>,
    pub hold_time_seconds: u64,
    /// Error if any source is missing or timed out
    pub error: Option<String>,
    pub enabled: bool,
    pub last_update: Option<chrono::DateTime<chrono::Utc>>,
}

/// Status of a single source within a switch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchSourceStatus {
    pub monitor_id: String,
    pub monitor_name: String,
    /// true = this source is triggered (condition met that would make it unsafe)
    pub is_triggered: bool,
    pub current_value: Option<f64>,
    pub threshold: f64,
    pub last_update: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
}
