use chrono::{DateTime, Utc};
use parking_lot::RwLock as SyncRwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::models::WeatherDeviceConfig;
use super::tempest::{TempestData, TempestObservation};

/// ASCOM ObservingConditions device
pub struct ObservingConditionsDevice {
    pub device_number: u32,
    pub connected: SyncRwLock<bool>,
    pub server_transaction_id: AtomicU32,
    pub name: String,
    pub description: String,
    pub driver_info: String,
    pub driver_version: String,
    pub config: SyncRwLock<WeatherDeviceConfig>,
}

impl ObservingConditionsDevice {
    pub fn new(config: WeatherDeviceConfig) -> Self {
        let auto_connect = config.auto_connect;
        Self {
            device_number: config.device_number,
            connected: SyncRwLock::new(auto_connect), // Start connected if auto_connect is true
            server_transaction_id: AtomicU32::new(0),
            name: config.name.clone(),
            description: config.description.clone(),
            driver_info: format!("LLAMA Weather Station Driver v{}", env!("CARGO_PKG_VERSION")),
            driver_version: env!("CARGO_PKG_VERSION").to_string(),
            config: SyncRwLock::new(config),
        }
    }

    pub fn next_transaction_id(&self) -> u32 {
        self.server_transaction_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.read()
    }

    pub fn set_connected(&self, connected: bool) {
        *self.connected.write() = connected;
    }

    pub fn update_config(&self, config: WeatherDeviceConfig) {
        *self.config.write() = config;
    }

    pub fn get_config(&self) -> WeatherDeviceConfig {
        self.config.read().clone()
    }
}

/// Weather data accessor for ObservingConditions devices
pub struct WeatherDataAccessor {
    pub tempest_data: Arc<RwLock<TempestData>>,
}

impl WeatherDataAccessor {
    pub fn new(tempest_data: Arc<RwLock<TempestData>>) -> Self {
        Self { tempest_data }
    }

    /// Get observation for a specific device
    pub async fn get_observation(&self, config: &WeatherDeviceConfig) -> Option<TempestObservation> {
        use crate::config::models::WeatherDataSource;

        match &config.source {
            WeatherDataSource::Tempest { serial_number } => {
                let data = self.tempest_data.read().await;
                if let Some(sn) = serial_number {
                    // Get specific device
                    data.observations.get(sn).cloned()
                } else {
                    // Get any Tempest device (first one)
                    data.observations.values().next().cloned()
                }
            }
            WeatherDataSource::WeatherUnderground { .. } => {
                // TODO: Implement Weather Underground support
                None
            }
        }
    }

    /// Get previous observation for rain rate calculation
    pub async fn get_previous_observation(&self, config: &WeatherDeviceConfig) -> Option<TempestObservation> {
        use crate::config::models::WeatherDataSource;

        match &config.source {
            WeatherDataSource::Tempest { serial_number } => {
                let data = self.tempest_data.read().await;
                if let Some(sn) = serial_number {
                    data.previous_observations.get(sn).cloned()
                } else {
                    data.previous_observations.values().next().cloned()
                }
            }
            WeatherDataSource::WeatherUnderground { .. } => {
                None
            }
        }
    }

    /// Get rain rate for this device
    pub async fn get_rain_rate(&self, config: &WeatherDeviceConfig) -> Option<f64> {
        let current = self.get_observation(config).await?;
        let previous = self.get_previous_observation(config).await;
        Some(current.calculate_rain_rate(previous.as_ref()))
    }

    /// Get time since last update for a specific property
    pub async fn time_since_last_update(&self, config: &WeatherDeviceConfig, _property: &str) -> Option<f64> {
        let obs = self.get_observation(config).await?;
        let now = Utc::now();
        let duration = now.signed_duration_since(obs.timestamp);
        Some(duration.num_seconds() as f64)
    }
}

/// Property names for ASCOM ObservingConditions
pub struct ObservingConditionsProperties;

#[allow(dead_code)]
impl ObservingConditionsProperties {
    pub const CLOUD_COVER: &'static str = "CloudCover";
    pub const DEW_POINT: &'static str = "DewPoint";
    pub const HUMIDITY: &'static str = "Humidity";
    pub const PRESSURE: &'static str = "Pressure";
    pub const RAIN_RATE: &'static str = "RainRate";
    pub const SKY_BRIGHTNESS: &'static str = "SkyBrightness";
    pub const SKY_QUALITY: &'static str = "SkyQuality";
    pub const SKY_TEMPERATURE: &'static str = "SkyTemperature";
    pub const STAR_FWHM: &'static str = "StarFWHM";
    pub const TEMPERATURE: &'static str = "Temperature";
    pub const WIND_DIRECTION: &'static str = "WindDirection";
    pub const WIND_GUST: &'static str = "WindGust";
    pub const WIND_SPEED: &'static str = "WindSpeed";

    pub fn all() -> Vec<&'static str> {
        vec![
            Self::CLOUD_COVER,
            Self::DEW_POINT,
            Self::HUMIDITY,
            Self::PRESSURE,
            Self::RAIN_RATE,
            Self::SKY_BRIGHTNESS,
            Self::SKY_QUALITY,
            Self::SKY_TEMPERATURE,
            Self::STAR_FWHM,
            Self::TEMPERATURE,
            Self::WIND_DIRECTION,
            Self::WIND_GUST,
            Self::WIND_SPEED,
        ]
    }

    pub fn tempest_supported() -> Vec<&'static str> {
        vec![
            Self::DEW_POINT,        // Calculated from temp + humidity
            Self::HUMIDITY,         // Direct
            Self::PRESSURE,         // Direct
            Self::RAIN_RATE,        // Calculated from accumulation
            Self::SKY_BRIGHTNESS,   // Converted from illuminance
            Self::TEMPERATURE,      // Direct
            Self::WIND_DIRECTION,   // Direct
            Self::WIND_GUST,        // Direct
            Self::WIND_SPEED,       // Direct (avg)
        ]
    }
}

/// Sensor descriptions for supported properties
pub fn get_sensor_description(property: &str) -> String {
    match property {
        ObservingConditionsProperties::DEW_POINT =>
            "Dew point calculated from temperature and humidity using Magnus formula".to_string(),
        ObservingConditionsProperties::HUMIDITY =>
            "Relative humidity from Tempest weather station".to_string(),
        ObservingConditionsProperties::PRESSURE =>
            "Atmospheric pressure from Tempest weather station".to_string(),
        ObservingConditionsProperties::RAIN_RATE =>
            "Rain rate calculated from precipitation accumulation".to_string(),
        ObservingConditionsProperties::SKY_BRIGHTNESS =>
            "Sky brightness derived from illuminance sensor".to_string(),
        ObservingConditionsProperties::TEMPERATURE =>
            "Ambient air temperature from Tempest weather station".to_string(),
        ObservingConditionsProperties::WIND_DIRECTION =>
            "Wind direction from Tempest weather station".to_string(),
        ObservingConditionsProperties::WIND_GUST =>
            "Maximum wind gust from Tempest weather station".to_string(),
        ObservingConditionsProperties::WIND_SPEED =>
            "Average wind speed from Tempest weather station".to_string(),
        _ => format!("Property '{}' not supported", property),
    }
}

/// Get property value from observation
pub fn get_property_value(obs: &TempestObservation, property: &str, previous_obs: Option<&TempestObservation>) -> Option<f64> {
    match property {
        ObservingConditionsProperties::DEW_POINT => Some(obs.calculate_dew_point()),
        ObservingConditionsProperties::HUMIDITY => Some(obs.humidity),
        ObservingConditionsProperties::PRESSURE => Some(obs.pressure),
        ObservingConditionsProperties::RAIN_RATE => Some(obs.calculate_rain_rate(previous_obs)),
        ObservingConditionsProperties::SKY_BRIGHTNESS => Some(obs.calculate_sky_brightness()),
        ObservingConditionsProperties::TEMPERATURE => Some(obs.temperature),
        ObservingConditionsProperties::WIND_DIRECTION => Some(obs.wind_direction),
        ObservingConditionsProperties::WIND_GUST => Some(obs.wind_gust),
        ObservingConditionsProperties::WIND_SPEED => Some(obs.wind_avg),
        _ => None, // Property not implemented
    }
}

/// Check if property is supported by Tempest
pub fn is_property_supported(property: &str) -> bool {
    ObservingConditionsProperties::tempest_supported().contains(&property)
}
