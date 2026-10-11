use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::config::models::AppConfig;
use super::ecowitt::EcowittPollers;
use super::observing_conditions::{ObservingConditionsDevice, WeatherDataAccessor, WeatherDeviceKind};
use super::weather_monitor::WeatherMonitorManager;

pub type ObservingConditionsDevices = Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>;

/// Owns everything weather-related that depends on configuration: the ASCOM
/// ObservingConditions devices, the weather safety monitors and the polled
/// sources. `apply_config` brings them in line with the current config.
pub struct WeatherService {
    pub data: Arc<WeatherDataAccessor>,
    pub devices: ObservingConditionsDevices,
    pub monitors: Arc<RwLock<WeatherMonitorManager>>,
    pollers: Mutex<EcowittPollers>,
}

impl WeatherService {
    pub fn new(data: Arc<WeatherDataAccessor>) -> Self {
        Self {
            data,
            devices: Arc::new(RwLock::new(HashMap::new())),
            monitors: Arc::new(RwLock::new(WeatherMonitorManager::new())),
            pollers: Mutex::new(EcowittPollers::default()),
        }
    }

    /// Rebuild devices, monitors and pollers from the current configuration.
    /// Devices that still exist keep their connected state.
    pub async fn apply_config(&self) {
        let config = self.data.config.read().await.clone();

        let mut devices = self.devices.write().await;
        let mut updated = HashMap::new();
        for device in exposed_devices(&config) {
            let connected = devices.get(&device.kind.device_number())
                .filter(|existing| existing.kind == device.kind)
                .map(|existing| existing.is_connected())
                .unwrap_or(device.auto_connect);
            let number = device.kind.device_number();
            updated.insert(number, Arc::new(ObservingConditionsDevice::new(
                device.kind, device.name, device.description, connected,
            )));
        }
        let mut numbers: Vec<_> = updated.keys().copied().collect();
        numbers.sort();
        info!("ObservingConditions devices: {:?}", numbers);
        *devices = updated;
        drop(devices);

        self.monitors.write().await.rebuild(&config, self.data.clone()).await;
        self.pollers.lock().await.sync(&config.weather_devices, &self.data.store);
    }
}

pub struct ExposedDevice {
    pub kind: WeatherDeviceKind,
    pub name: String,
    pub description: String,
    pub auto_connect: bool,
}

/// ObservingConditions devices to serve. Device 0 (the aggregate) exists as
/// soon as one source is enabled; individual sources are also served as their
/// own devices once two or more are enabled, unless hidden with expose_device.
pub fn exposed_devices(config: &AppConfig) -> Vec<ExposedDevice> {
    let mut enabled: Vec<_> = config.weather_devices.values().filter(|d| d.enabled).collect();
    enabled.sort_by_key(|d| d.device_number);

    let mut exposed = Vec::new();
    if enabled.is_empty() {
        return exposed;
    }

    let aggregate = config.weather_aggregate.clone().unwrap_or_default();
    exposed.push(ExposedDevice {
        kind: WeatherDeviceKind::Aggregate,
        name: aggregate.name,
        description: aggregate.description,
        auto_connect: aggregate.auto_connect,
    });

    if enabled.len() >= 2 {
        for device in enabled.into_iter().filter(|d| d.expose_device) {
            exposed.push(ExposedDevice {
                kind: WeatherDeviceKind::Source(device.device_number),
                name: device.name.clone(),
                description: device.description.clone(),
                auto_connect: device.auto_connect,
            });
        }
    }
    exposed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::{WeatherAggregateConfig, WeatherDataSource, WeatherDeviceConfig};

    fn config_with_sources(count: u32) -> AppConfig {
        let mut config = AppConfig::new();
        config.weather_aggregate = Some(WeatherAggregateConfig::default());
        for n in 1..=count {
            config.weather_devices.insert(n, WeatherDeviceConfig {
                device_number: n,
                name: format!("Source {}", n),
                description: String::new(),
                source: WeatherDataSource::Tempest { serial_number: Some(format!("ST-{}", n)) },
                enabled: true,
                auto_connect: true,
                expose_device: true,
                safety_thresholds: None,
            });
        }
        config
    }

    fn numbers(config: &AppConfig) -> Vec<u32> {
        exposed_devices(config).iter().map(|d| d.kind.device_number()).collect()
    }

    #[test]
    fn no_sources_means_no_devices() {
        assert!(numbers(&config_with_sources(0)).is_empty());
    }

    #[test]
    fn single_source_is_served_only_as_the_aggregate() {
        assert_eq!(numbers(&config_with_sources(1)), vec![0]);
    }

    #[test]
    fn multiple_sources_are_also_served_individually() {
        assert_eq!(numbers(&config_with_sources(3)), vec![0, 1, 2, 3]);
    }

    #[test]
    fn hidden_and_disabled_sources_are_not_served() {
        let mut config = config_with_sources(3);
        config.weather_devices.get_mut(&2).unwrap().expose_device = false;
        assert_eq!(numbers(&config), vec![0, 1, 3]);

        // Disabling one of two sources leaves a single source, served as the aggregate only
        let mut config = config_with_sources(2);
        config.weather_devices.get_mut(&2).unwrap().enabled = false;
        assert_eq!(numbers(&config), vec![0]);
    }
}
