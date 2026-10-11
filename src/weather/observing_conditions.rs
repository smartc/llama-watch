use parking_lot::RwLock as SyncRwLock;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::config::models::{WeatherDataSource, WeatherDeviceConfig};
use super::observation::{supported_properties, Reading, Readings, SharedWeatherStore, WeatherProperty};

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
    pub store: SharedWeatherStore,
}

impl WeatherDataAccessor {
    pub fn new(store: SharedWeatherStore) -> Self {
        Self { store }
    }

    /// Current readings for a weather device. Only properties the device's
    /// source supports and has reported are included.
    pub async fn readings(&self, config: &WeatherDeviceConfig) -> Readings {
        let store = self.store.read().await;
        let Some(obs) = store.observation_for(&config.source) else {
            return Readings::new();
        };
        supported_properties(&config.source).iter()
            .filter_map(|&property| {
                obs.get(property).map(|value| (property, Reading {
                    value,
                    timestamp: obs.timestamp,
                    source_device: config.device_number,
                }))
            })
            .collect()
    }
}

/// Whether a device implements an ASCOM ObservingConditions property
pub fn is_property_supported(config: &WeatherDeviceConfig, property: WeatherProperty) -> bool {
    property.ascom_name().is_some() && supported_properties(&config.source).contains(&property)
}

/// ASCOM sensor description for a property
pub fn get_sensor_description(config: &WeatherDeviceConfig, property: WeatherProperty) -> String {
    format!("{} from {}", property.description(), source_type_label(&config.source))
}

pub fn source_type_label(source: &WeatherDataSource) -> &'static str {
    match source {
        WeatherDataSource::Tempest { .. } => "Tempest weather station",
        WeatherDataSource::Ecowitt { .. } => "Ecowitt gateway",
        WeatherDataSource::WeatherUnderground { .. } => "Weather Underground",
    }
}
