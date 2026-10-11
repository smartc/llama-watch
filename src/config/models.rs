use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::weather::observation::WeatherProperty;

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
    pub timeout_seconds: i64, // Mark unsafe if no data received in this many seconds (default 300, <0 = ignore timeout)
    #[serde(default)]
    pub hold_time_seconds: u64, // Minimum time a condition must hold before changing state (0 = immediate)
    #[serde(default = "default_enabled")]
    pub enabled: bool, // If false, monitor is ignored
    #[serde(default = "default_include_in_safety")]
    pub include_in_safety: bool, // Include this monitor in overall safety evaluation (default true)
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
    pub timeout_seconds: i64, // Mark unsafe if no data received in this many seconds (default 300, <0 = ignore timeout)
    #[serde(default)]
    pub hold_time_seconds: u64, // Minimum time a condition must hold before changing state (0 = immediate)
    #[serde(default = "default_enabled")]
    pub enabled: bool, // If false, monitor is ignored
    #[serde(default = "default_include_in_safety")]
    pub include_in_safety: bool, // Include this monitor in overall safety evaluation (default true)
}

fn default_timeout() -> i64 {
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
    pub weather_devices: HashMap<u32, WeatherDeviceConfig>, // Weather sources, key is device_number (>= 1)
    #[serde(default)]
    pub weather_aggregate: Option<WeatherAggregateConfig>, // Aggregate ObservingConditions device 0
    #[serde(default)]
    pub monitor_groups: HashMap<String, MonitorGroupConfig>, // Named groups with custom logic
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
            weather_aggregate: Some(WeatherAggregateConfig::default()),
            monitor_groups: HashMap::new(),
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

    /// Upgrade configs written before the aggregate weather device existed.
    /// The old device 0 becomes a source with the next free number, and the new
    /// aggregate device 0 takes over its name and safety thresholds so ASCOM
    /// clients and safety behaviour are unchanged. Returns true if anything changed.
    pub fn migrate(&mut self) -> bool {
        if self.weather_aggregate.is_some() {
            return false;
        }

        let mut aggregate = WeatherAggregateConfig::default();
        if let Some(mut device) = self.weather_devices.remove(&0) {
            aggregate.name = device.name.clone();
            aggregate.description = device.description.clone();
            aggregate.auto_connect = device.auto_connect;
            aggregate.safety_thresholds = device.safety_thresholds.take();

            device.device_number = self.next_weather_device_number();
            self.weather_devices.insert(device.device_number, device);
        }
        self.weather_aggregate = Some(aggregate);
        true
    }

    /// Lowest unused weather source number (0 is the aggregate device)
    pub fn next_weather_device_number(&self) -> u32 {
        (1..).find(|n| !self.weather_devices.contains_key(n)).unwrap_or(1)
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

        for (number, device) in &self.weather_devices {
            if *number == 0 {
                return Err(anyhow::anyhow!(
                    "Weather device '{}' uses device number 0, which is reserved for the aggregate device",
                    device.name
                ));
            }
            if device.device_number != *number {
                return Err(anyhow::anyhow!(
                    "Weather device '{}' is stored under number {} but has device_number {}",
                    device.name, number, device.device_number
                ));
            }
            let polled = match &device.source {
                WeatherDataSource::Ecowitt { host, poll_interval_seconds } => Some((host, *poll_interval_seconds)),
                WeatherDataSource::Alpaca { host, poll_interval_seconds, .. } => Some((host, *poll_interval_seconds)),
                _ => None,
            };
            if let Some((host, poll_interval_seconds)) = polled {
                if host.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}' has no host", device.name
                    ));
                }
                if poll_interval_seconds < 5 {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}' poll interval must be at least 5 seconds", device.name
                    ));
                }
            }
            if let WeatherDataSource::WeatherUnderground { station_id, api_key, poll_interval_seconds } = &device.source {
                if station_id.trim().is_empty() || api_key.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}' needs a Weather Underground station ID and API key", device.name
                    ));
                }
                if *poll_interval_seconds < 60 {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}' poll interval must be at least 60 seconds (Weather Underground allows 1500 calls/day)",
                        device.name
                    ));
                }
            }
            if let WeatherDataSource::Alpaca { properties, .. } = &device.source {
                if properties.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}' has no properties selected", device.name
                    ));
                }
                if let Some(p) = properties.iter().find(|p| p.ascom_name().is_none()) {
                    return Err(anyhow::anyhow!(
                        "Weather device '{}': {} is not an ASCOM ObservingConditions property",
                        device.name, p.label()
                    ));
                }
            }
        }

        if let Some(aggregate) = &self.weather_aggregate {
            if aggregate.stale_after_seconds == 0 {
                return Err(anyhow::anyhow!("Aggregate weather device stale_after_seconds must be at least 1"));
            }
            let referenced = aggregate.source_priority.iter()
                .chain(aggregate.property_sources.values().flatten());
            for number in referenced {
                if !self.weather_devices.contains_key(number) {
                    return Err(anyhow::anyhow!(
                        "Aggregate weather device references non-existent weather source {}", number
                    ));
                }
            }
        }

        // Validate that all monitor group members reference valid monitors
        for group in self.monitor_groups.values() {
            if group.members.is_empty() {
                return Err(anyhow::anyhow!(
                    "Monitor group '{}' has no members defined",
                    group.id
                ));
            }

            for member_id in &group.members {
                let mqtt_exists = self.mqtt_monitors.contains_key(member_id);
                let alpaca_exists = self.alpaca_monitors.contains_key(member_id);
                // Weather monitors use device_number as string (0 = aggregate)
                let weather_exists = member_id.parse::<u32>().ok()
                    .map(|num| {
                        if num == 0 { self.weather_aggregate.is_some() } else { self.weather_devices.contains_key(&num) }
                    })
                    .unwrap_or(false);
                if !mqtt_exists && !alpaca_exists && !weather_exists {
                    return Err(anyhow::anyhow!(
                        "Monitor group '{}' references non-existent monitor '{}'",
                        group.id, member_id
                    ));
                }
            }

            // Validate MinOf logic has a reasonable count
            if let GroupLogic::MinOf { count } = &group.logic {
                if *count == 0 {
                    return Err(anyhow::anyhow!(
                        "Monitor group '{}' has min_of count of 0, which would always be safe",
                        group.id
                    ));
                }
                if *count > group.members.len() as u32 {
                    return Err(anyhow::anyhow!(
                        "Monitor group '{}' has min_of count {} but only {} members",
                        group.id, count, group.members.len()
                    ));
                }
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
    pub include_in_safety: bool,
}

// Weather source configuration. Each source can also be served as its own
// ObservingConditions device (1, 2, ...) once more than one source is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherDeviceConfig {
    #[serde(default)]
    pub device_number: u32, // 0 when creating = assign the next free number
    pub name: String,
    pub description: String,
    pub source: WeatherDataSource,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool, // If true, device starts in connected state
    #[serde(default = "default_enabled")]
    pub expose_device: bool, // Serve as its own ASCOM device when 2+ sources are enabled
    #[serde(default)]
    pub safety_thresholds: Option<WeatherSafetyThresholds>,
}

/// The aggregate ObservingConditions device (always device 0). Each property is
/// served from the first source in its priority list that has fresh data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherAggregateConfig {
    #[serde(default = "default_aggregate_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool,
    /// Data older than this is skipped in favour of the next source in the list
    #[serde(default = "default_stale_after")]
    pub stale_after_seconds: u64,
    /// After falling back, a higher-priority source is only used again once it
    /// has reported without gaps for this long
    #[serde(default = "default_return_after")]
    pub return_after_seconds: u64,
    /// Default source order (device numbers). Empty = all enabled sources by device number.
    #[serde(default)]
    pub source_priority: Vec<u32>,
    /// Per-property source order, replacing source_priority for that property
    #[serde(default)]
    pub property_sources: HashMap<WeatherProperty, Vec<u32>>,
    #[serde(default)]
    pub safety_thresholds: Option<WeatherSafetyThresholds>,
}

impl Default for WeatherAggregateConfig {
    fn default() -> Self {
        Self {
            name: default_aggregate_name(),
            description: String::new(),
            auto_connect: true,
            stale_after_seconds: default_stale_after(),
            return_after_seconds: default_return_after(),
            source_priority: Vec::new(),
            property_sources: HashMap::new(),
            safety_thresholds: None,
        }
    }
}

impl WeatherAggregateConfig {
    /// Sources to try for a property, in priority order
    pub fn sources_for(&self, property: WeatherProperty, weather_devices: &HashMap<u32, WeatherDeviceConfig>) -> Vec<u32> {
        if let Some(sources) = self.property_sources.get(&property).filter(|s| !s.is_empty()) {
            return sources.clone();
        }
        if !self.source_priority.is_empty() {
            return self.source_priority.clone();
        }
        let mut all: Vec<u32> = weather_devices.keys().copied().collect();
        all.sort();
        all
    }

    /// Drop references to a deleted source
    pub fn remove_source(&mut self, device_number: u32) {
        self.source_priority.retain(|n| *n != device_number);
        for sources in self.property_sources.values_mut() {
            sources.retain(|n| *n != device_number);
        }
        self.property_sources.retain(|_, sources| !sources.is_empty());
    }
}

fn default_aggregate_name() -> String {
    "LLAMA Weather".to_string()
}

fn default_stale_after() -> u64 {
    300
}

fn default_return_after() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WeatherDataSource {
    Tempest {
        serial_number: Option<String>, // If None, accept any Tempest device
    },
    /// Ecowitt gateway polled over its local HTTP API
    Ecowitt {
        host: String, // Hostname or IP of the gateway
        #[serde(default = "default_ecowitt_poll_interval")]
        poll_interval_seconds: u64,
    },
    /// ASCOM Alpaca ObservingConditions device polled over HTTP
    Alpaca {
        host: String,
        #[serde(default = "default_alpaca_port")]
        port: u16,
        #[serde(default)]
        device_number: u32, // Device number on the remote Alpaca server
        #[serde(default = "default_ecowitt_poll_interval")]
        poll_interval_seconds: u64,
        #[serde(default)]
        properties: Vec<WeatherProperty>, // Properties to take from this device
    },
    /// Weather Underground personal weather station (PWS API)
    WeatherUnderground {
        station_id: String,
        api_key: String,
        #[serde(default = "default_wu_poll_interval")]
        poll_interval_seconds: u64, // PWS keys allow 1500 calls/day, so at least 60
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeatherSafetyThresholds {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i64, // <0 = ignore timeout
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
    #[serde(default)]
    pub sky_quality: Option<ThresholdConfig>,
    #[serde(default)]
    pub sky_temperature: Option<ThresholdConfig>,
    #[serde(default)]
    pub cloud_cover: Option<ThresholdConfig>,
}

impl WeatherSafetyThresholds {
    /// The thresholds that are configured, paired with the property they apply to
    pub fn configured(&self) -> Vec<(WeatherProperty, &ThresholdConfig)> {
        [
            (WeatherProperty::Temperature, &self.temperature),
            (WeatherProperty::Humidity, &self.humidity),
            (WeatherProperty::SkyBrightness, &self.sky_brightness),
            (WeatherProperty::WindSpeed, &self.wind_speed),
            (WeatherProperty::WindGust, &self.wind_gust),
            (WeatherProperty::RainRate, &self.rain_rate),
            (WeatherProperty::SkyQuality, &self.sky_quality),
            (WeatherProperty::SkyTemperature, &self.sky_temperature),
            (WeatherProperty::CloudCover, &self.cloud_cover),
        ]
        .into_iter()
        .filter_map(|(property, threshold)| threshold.as_ref().map(|t| (property, t)))
        .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub threshold: f64,
    pub operator: ComparisonOperator,
    pub safe_when_true: bool,
    #[serde(default)]
    pub timeout_seconds: Option<i64>, // Per-measurement timeout (overrides global if set, <0 = ignore timeout)
}

fn default_ecowitt_poll_interval() -> u64 {
    30
}

fn default_wu_poll_interval() -> u64 {
    120
}

fn default_alpaca_port() -> u16 {
    11111
}

fn default_auto_connect() -> bool {
    true
}

/// Logic for combining multiple sources into a single result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GroupLogic {
    /// OR logic - group is unsafe if ANY member is unsafe
    Any,
    /// AND logic - group is unsafe only if ALL members are unsafe
    All,
    /// N-out-of-M logic - group is unsafe if at least N members are unsafe
    #[serde(rename = "min_of")]
    MinOf { count: u32 },
}

impl Default for GroupLogic {
    fn default() -> Self {
        GroupLogic::Any
    }
}

impl GroupLogic {
    /// Evaluate the logic given a list of unsafe states (true = unsafe)
    pub fn evaluate_unsafe(&self, unsafe_states: &[bool]) -> bool {
        if unsafe_states.is_empty() {
            return false;
        }

        let unsafe_count = unsafe_states.iter().filter(|&&s| s).count() as u32;

        match self {
            GroupLogic::Any => unsafe_count > 0,
            GroupLogic::All => unsafe_count == unsafe_states.len() as u32,
            GroupLogic::MinOf { count } => unsafe_count >= *count,
        }
    }

    /// Get a human-readable description of the logic
    pub fn description(&self) -> String {
        match self {
            GroupLogic::Any => "unsafe if ANY member is unsafe".to_string(),
            GroupLogic::All => "unsafe only if ALL members are unsafe".to_string(),
            GroupLogic::MinOf { count } => format!("unsafe if at least {} members are unsafe", count),
        }
    }
}

/// Configuration for a monitor group that aggregates multiple monitors with custom logic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorGroupConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// List of monitor IDs to include in this group (MQTT, Alpaca, or Weather monitors)
    pub members: Vec<String>,
    /// How to combine member states to determine group safety
    #[serde(default)]
    pub logic: GroupLogic,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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
    /// Timeout for the aggregated switch (sources without data within this time are considered unsafe, <0 = ignore timeout)
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i64,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn old_style_config() -> AppConfig {
        serde_json::from_value(serde_json::json!({
            "weather_devices": {
                "0": {
                    "device_number": 0,
                    "name": "Tempest Weather Station",
                    "description": "WeatherFlow Tempest",
                    "source": {"type": "tempest", "serial_number": null},
                    "enabled": true,
                    "auto_connect": true,
                    "safety_thresholds": {
                        "enabled": true,
                        "timeout_seconds": 300,
                        "hold_time_seconds": 60,
                        "rain_rate": {"threshold": 0.0, "operator": "greaterthan", "safe_when_true": false}
                    }
                }
            },
            "monitor_groups": {},
            "server_port": 8080
        })).unwrap()
    }

    #[test]
    fn migration_moves_device_zero_to_a_source_behind_the_aggregate() {
        let mut config = old_style_config();
        assert!(config.migrate());

        let aggregate = config.weather_aggregate.as_ref().unwrap();
        assert_eq!(aggregate.name, "Tempest Weather Station");
        assert!(aggregate.safety_thresholds.as_ref().unwrap().rain_rate.is_some());

        let source = &config.weather_devices[&1];
        assert_eq!(source.device_number, 1);
        assert!(source.safety_thresholds.is_none());
        assert!(source.expose_device);
        assert!(!config.weather_devices.contains_key(&0));
        config.validate().unwrap();

        // Already migrated: nothing more to do
        assert!(!config.migrate());
    }

    #[test]
    fn example_config_is_valid_and_already_migrated() {
        let mut config: AppConfig = serde_json::from_str(include_str!("../../config.example.json")).unwrap();
        assert!(!config.migrate());
        config.validate().unwrap();
    }

    #[test]
    fn migration_of_config_without_weather_adds_default_aggregate() {
        let mut config: AppConfig = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(config.migrate());
        assert!(config.weather_devices.is_empty());
        assert_eq!(config.weather_aggregate.unwrap().name, "LLAMA Weather");
    }

    #[test]
    fn device_number_zero_is_rejected_after_migration() {
        let mut config = old_style_config();
        config.migrate();
        let mut device = config.weather_devices[&1].clone();
        device.device_number = 0;
        config.weather_devices.insert(0, device);
        assert!(config.validate().is_err());
    }

    #[test]
    fn aggregate_source_order_falls_back_to_all_sources() {
        let mut config = old_style_config();
        config.migrate();
        let mut second = config.weather_devices[&1].clone();
        second.device_number = 2;
        config.weather_devices.insert(2, second);

        let aggregate = config.weather_aggregate.as_mut().unwrap();
        assert_eq!(aggregate.sources_for(WeatherProperty::WindSpeed, &config.weather_devices), vec![1, 2]);

        aggregate.source_priority = vec![2, 1];
        aggregate.property_sources.insert(WeatherProperty::Pressure, vec![1]);
        assert_eq!(aggregate.sources_for(WeatherProperty::WindSpeed, &config.weather_devices), vec![2, 1]);
        assert_eq!(aggregate.sources_for(WeatherProperty::Pressure, &config.weather_devices), vec![1]);

        aggregate.remove_source(1);
        assert_eq!(aggregate.source_priority, vec![2]);
        assert!(aggregate.property_sources.is_empty());
    }
}
