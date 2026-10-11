use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock as SyncRwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::models::{AppConfig, WeatherDeviceConfig};
use super::observation::{supported_properties, Reading, Readings, SharedWeatherStore, WeatherProperty, WeatherStore};

/// What an ObservingConditions device serves
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherDeviceKind {
    /// Device 0: each property taken from the best available source
    Aggregate,
    /// A single weather source, by device number
    Source(u32),
}

impl WeatherDeviceKind {
    pub fn device_number(self) -> u32 {
        match self {
            WeatherDeviceKind::Aggregate => 0,
            WeatherDeviceKind::Source(n) => n,
        }
    }
}

/// ASCOM ObservingConditions device
pub struct ObservingConditionsDevice {
    pub device_number: u32,
    pub kind: WeatherDeviceKind,
    pub connected: SyncRwLock<bool>,
    pub server_transaction_id: AtomicU32,
    pub name: String,
    pub description: String,
    pub driver_info: String,
    pub driver_version: String,
}

impl ObservingConditionsDevice {
    pub fn new(kind: WeatherDeviceKind, name: String, description: String, connected: bool) -> Self {
        Self {
            device_number: kind.device_number(),
            kind,
            connected: SyncRwLock::new(connected),
            server_transaction_id: AtomicU32::new(0),
            name,
            description,
            driver_info: format!("LLAMA Weather Station Driver v{}", env!("CARGO_PKG_VERSION")),
            driver_version: env!("CARGO_PKG_VERSION").to_string(),
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
}

/// Weather data accessor for ObservingConditions devices and weather monitors
pub struct WeatherDataAccessor {
    pub store: SharedWeatherStore,
    pub config: Arc<RwLock<AppConfig>>,
}

impl WeatherDataAccessor {
    pub fn new(store: SharedWeatherStore, config: Arc<RwLock<AppConfig>>) -> Self {
        Self { store, config }
    }

    /// Current readings for a device
    pub async fn readings(&self, kind: WeatherDeviceKind) -> Readings {
        let config = self.config.read().await;
        self.readings_with(&config, kind).await
    }

    /// Current readings for a device, for callers already holding the config lock
    pub async fn readings_with(&self, config: &AppConfig, kind: WeatherDeviceKind) -> Readings {
        let store = self.store.read().await;
        resolve_readings(&store, config, kind, Utc::now())
    }

    /// Whether a device implements an ASCOM ObservingConditions property
    pub async fn is_property_supported(&self, kind: WeatherDeviceKind, property: WeatherProperty) -> bool {
        let config = self.config.read().await;
        property.ascom_name().is_some() && device_supported_properties(&config, kind).contains(&property)
    }

    /// ASCOM sensor description for a property
    pub async fn sensor_description(&self, kind: WeatherDeviceKind, property: WeatherProperty) -> String {
        let config = self.config.read().await;
        let names: Vec<&str> = providing_sources(&config, kind, property).iter()
            .map(|(_, device)| device.name.as_str())
            .collect();
        match names.as_slice() {
            [] => property.description().to_string(),
            [only] => format!("{} from {}", property.description(), only),
            [first, rest @ ..] => format!(
                "{} from {}, falling back to {}", property.description(), first, rest.join(", then ")
            ),
        }
    }
}

/// Enabled sources that can provide a property for a device, in priority order
fn providing_sources<'a>(
    config: &'a AppConfig,
    kind: WeatherDeviceKind,
    property: WeatherProperty,
) -> Vec<(u32, &'a WeatherDeviceConfig)> {
    let order = match kind {
        WeatherDeviceKind::Source(n) => vec![n],
        WeatherDeviceKind::Aggregate => match &config.weather_aggregate {
            Some(aggregate) => aggregate.sources_for(property, &config.weather_devices),
            None => Vec::new(),
        },
    };
    order.into_iter()
        .filter_map(|n| config.weather_devices.get(&n).map(|d| (n, d)))
        .filter(|(_, d)| d.enabled && supported_properties(&d.source).contains(&property))
        .collect()
}

/// Properties a device can serve. Fixed by configuration (not by what data has
/// arrived) so ASCOM clients see a stable set of implemented properties.
pub fn device_supported_properties(config: &AppConfig, kind: WeatherDeviceKind) -> Vec<WeatherProperty> {
    WeatherProperty::ALL.into_iter()
        .filter(|&p| !providing_sources(config, kind, p).is_empty())
        .collect()
}

/// Resolve the readings a device currently serves
pub fn resolve_readings(store: &WeatherStore, config: &AppConfig, kind: WeatherDeviceKind, now: DateTime<Utc>) -> Readings {
    let stale_after = config.weather_aggregate.as_ref()
        .map(|a| Duration::seconds(a.stale_after_seconds as i64))
        .unwrap_or_else(|| Duration::seconds(120));
    let mut cache: HashMap<u32, Readings> = HashMap::new();
    let mut readings = Readings::new();

    for property in WeatherProperty::ALL {
        let candidates: Vec<Reading> = providing_sources(config, kind, property).into_iter()
            .filter_map(|(n, device)| {
                cache.entry(n)
                    .or_insert_with(|| source_readings(store, device))
                    .get(&property)
                    .cloned()
            })
            .collect();

        // First fresh reading in priority order; if all are stale, the most
        // recent one so weather monitor timeouts still apply to it
        let chosen = candidates.iter()
            .find(|r| now - r.timestamp <= stale_after)
            .or_else(|| candidates.iter().max_by_key(|r| r.timestamp));
        if let Some(reading) = chosen {
            readings.insert(property, reading.clone());
        }
    }
    readings
}

/// Readings from a single source's latest observation
fn source_readings(store: &WeatherStore, device: &WeatherDeviceConfig) -> Readings {
    let Some(obs) = store.observation_for(&device.source) else {
        return Readings::new();
    };
    supported_properties(&device.source).into_iter()
        .filter_map(|property| {
            obs.get(property).map(|value| (property, Reading {
                value,
                timestamp: obs.measured_at(property),
                source_device: device.device_number,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::{WeatherAggregateConfig, WeatherDataSource};
    use crate::weather::observation::WeatherObservation;
    use std::collections::BTreeMap;

    fn source(number: u32, source: WeatherDataSource) -> WeatherDeviceConfig {
        WeatherDeviceConfig {
            device_number: number,
            name: format!("Source {}", number),
            description: String::new(),
            source,
            enabled: true,
            auto_connect: true,
            expose_device: true,
            safety_thresholds: None,
        }
    }

    fn tempest() -> WeatherDataSource {
        WeatherDataSource::Tempest { serial_number: Some("ST-1".into()) }
    }

    fn ecowitt() -> WeatherDataSource {
        WeatherDataSource::Ecowitt { host: "gw".into(), poll_interval_seconds: 30 }
    }

    fn setup(now: DateTime<Utc>, tempest_age: i64, ecowitt_age: i64) -> (WeatherStore, AppConfig) {
        let mut store = WeatherStore::default();
        store.update(WeatherStore::tempest_key("ST-1"), WeatherObservation::new(
            now - Duration::seconds(tempest_age),
            BTreeMap::from([(WeatherProperty::WindSpeed, 1.0), (WeatherProperty::SkyBrightness, 0.5)]),
        ));
        store.update(WeatherStore::ecowitt_key("gw"), WeatherObservation::new(
            now - Duration::seconds(ecowitt_age),
            BTreeMap::from([(WeatherProperty::WindSpeed, 2.0), (WeatherProperty::Pressure, 890.0)]),
        ));

        let mut config = AppConfig::new();
        config.weather_devices.insert(1, source(1, tempest()));
        config.weather_devices.insert(2, source(2, ecowitt()));
        config.weather_aggregate = Some(WeatherAggregateConfig::default());
        (store, config)
    }

    #[test]
    fn aggregate_uses_first_fresh_source_in_priority_order() {
        let now = Utc::now();
        let (store, mut config) = setup(now, 10, 10);

        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 1);
        // Properties only one source has come from that source
        assert_eq!(readings[&WeatherProperty::Pressure].source_device, 2);
        assert_eq!(readings[&WeatherProperty::SkyBrightness].source_device, 1);

        config.weather_aggregate.as_mut().unwrap()
            .property_sources.insert(WeatherProperty::WindSpeed, vec![2, 1]);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].value, 2.0);
    }

    #[test]
    fn aggregate_falls_back_when_primary_is_stale() {
        let now = Utc::now();
        let (store, config) = setup(now, 600, 10);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 2);
    }

    #[test]
    fn aggregate_serves_newest_stale_reading_when_all_are_stale() {
        let now = Utc::now();
        let (store, config) = setup(now, 900, 600);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        let wind = &readings[&WeatherProperty::WindSpeed];
        assert_eq!(wind.source_device, 2);
        assert_eq!((now - wind.timestamp).num_seconds(), 600);
    }

    #[test]
    fn aggregate_ignores_disabled_and_unlisted_sources() {
        let now = Utc::now();
        let (store, mut config) = setup(now, 10, 10);
        config.weather_devices.get_mut(&1).unwrap().enabled = false;
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 2);
        assert!(!readings.contains_key(&WeatherProperty::SkyBrightness));
        assert!(!device_supported_properties(&config, WeatherDeviceKind::Aggregate)
            .contains(&WeatherProperty::SkyBrightness));

        config.weather_devices.get_mut(&1).unwrap().enabled = true;
        config.weather_aggregate.as_mut().unwrap().source_priority = vec![2];
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert!(!readings.contains_key(&WeatherProperty::SkyBrightness));
    }

    #[test]
    fn alpaca_source_uses_per_property_measurement_time() {
        let now = Utc::now();
        let (mut store, mut config) = setup(now, 10, 10);
        let alpaca = WeatherDataSource::Alpaca {
            host: "skycon".into(),
            port: 11111,
            device_number: 0,
            poll_interval_seconds: 30,
            properties: vec![WeatherProperty::SkyQuality, WeatherProperty::Temperature],
        };
        config.weather_devices.insert(3, source(3, alpaca));
        config.weather_aggregate.as_mut().unwrap()
            .property_sources.insert(WeatherProperty::Temperature, vec![3]);

        // SkyQuality measured 400s ago although the poll itself just happened
        store.update(WeatherStore::alpaca_key("skycon", 11111, 0), WeatherObservation::new(
            now,
            BTreeMap::from([(WeatherProperty::SkyQuality, 20.5), (WeatherProperty::Temperature, 3.0)]),
        ).with_measured_at(BTreeMap::from([(WeatherProperty::SkyQuality, now - Duration::seconds(400))])));

        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!((now - readings[&WeatherProperty::SkyQuality].timestamp).num_seconds(), 400);
        assert_eq!(readings[&WeatherProperty::Temperature].source_device, 3);
        assert!(device_supported_properties(&config, WeatherDeviceKind::Aggregate)
            .contains(&WeatherProperty::SkyQuality));
        // Tempest and Ecowitt don't provide sky quality
        assert!(!device_supported_properties(&config, WeatherDeviceKind::Source(1))
            .contains(&WeatherProperty::SkyQuality));
    }

    #[test]
    fn source_device_serves_only_its_own_data() {
        let now = Utc::now();
        let (store, config) = setup(now, 10, 10);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Source(2), now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].value, 2.0);
        assert!(!readings.contains_key(&WeatherProperty::SkyBrightness));
        assert!(!device_supported_properties(&config, WeatherDeviceKind::Source(2))
            .contains(&WeatherProperty::SkyBrightness));
    }
}
