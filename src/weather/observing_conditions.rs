use chrono::{DateTime, Duration, Utc};
use parking_lot::{Mutex as SyncMutex, RwLock as SyncRwLock};
use tracing::{debug, info, warn};
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
    /// Source each aggregate property was last served from, for logging changes
    aggregate_sources: SyncMutex<HashMap<WeatherProperty, u32>>,
}

impl WeatherDataAccessor {
    pub fn new(store: SharedWeatherStore, config: Arc<RwLock<AppConfig>>) -> Self {
        Self { store, config, aggregate_sources: SyncMutex::new(HashMap::new()) }
    }

    /// Current readings for a device
    pub async fn readings(&self, kind: WeatherDeviceKind) -> Readings {
        let config = self.config.read().await;
        self.readings_with(&config, kind).await
    }

    /// Current readings for a device, for callers already holding the config lock
    pub async fn readings_with(&self, config: &AppConfig, kind: WeatherDeviceKind) -> Readings {
        let store = self.store.read().await;
        let now = Utc::now();
        let readings = resolve_readings(&store, config, kind, now);
        if kind == WeatherDeviceKind::Aggregate {
            self.log_source_changes(&store, config, &readings, now);
        }
        readings
    }

    /// Log when an aggregate property starts coming from a different source
    fn log_source_changes(&self, store: &WeatherStore, config: &AppConfig, readings: &Readings, now: DateTime<Utc>) {
        let name = |n: u32| config.weather_devices.get(&n)
            .map(|d| format!("{} (#{})", d.name, n))
            .unwrap_or_else(|| format!("#{}", n));
        let mut previous = self.aggregate_sources.lock();

        for property in WeatherProperty::ALL {
            let current = readings.get(&property).map(|r| r.source_device);
            let before = previous.get(&property).copied();
            if current == before {
                continue;
            }
            let label = property.label();
            if store.starting_up(now) {
                // Sources report for the first time at different moments after startup
                if let Some(to) = current {
                    debug!("Aggregate {}: using {}", label, name(to));
                }
            } else {
                match (before, current) {
                    (None, Some(to)) => info!("Aggregate {}: now using {}", label, name(to)),
                    (Some(from), None) => warn!("Aggregate {}: no source has data (was {})", label, name(from)),
                    (Some(from), Some(to)) => {
                        let order: Vec<u32> = providing_sources(config, WeatherDeviceKind::Aggregate, property)
                            .into_iter().map(|(n, _)| n).collect();
                        let rank = |n: u32| order.iter().position(|&o| o == n).unwrap_or(usize::MAX);
                        if rank(to) < rank(from) {
                            info!("Aggregate {}: back to {} (was {})", label, name(to), name(from));
                        } else {
                            let reason = config.weather_devices.get(&from)
                                .and_then(|d| source_readings(store, d).get(&property).map(|r| r.timestamp))
                                .map(|ts| format!("its data is {}s old", (now - ts).num_seconds()))
                                .unwrap_or_else(|| "it has no data".to_string());
                            info!("Aggregate {}: switched from {} to {}; {}", label, name(from), name(to), reason);
                        }
                    }
                    (None, None) => {}
                }
            }
            match current {
                Some(n) => previous.insert(property, n),
                None => previous.remove(&property),
            };
        }
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

/// Resolve the readings a device currently serves. For each property, in
/// priority order, use the first source that is fresh and has been reporting
/// without gaps for return_after_seconds. A source that recovers after
/// dropping out therefore only takes over again once it has proven steady,
/// instead of the aggregate bouncing between sources. If no fresh source is
/// steady yet, the first fresh one is used; if all are stale, the most recent
/// reading is served so weather monitor timeouts still apply to it.
pub fn resolve_readings(store: &WeatherStore, config: &AppConfig, kind: WeatherDeviceKind, now: DateTime<Utc>) -> Readings {
    let aggregate = config.weather_aggregate.clone().unwrap_or_default();
    let stale_after = Duration::seconds(aggregate.stale_after_seconds as i64);
    let return_after = Duration::seconds(aggregate.return_after_seconds as i64);
    let mut cache: HashMap<u32, Readings> = HashMap::new();
    let mut readings = Readings::new();

    for property in WeatherProperty::ALL {
        let candidates: Vec<(Reading, &WeatherDeviceConfig)> = providing_sources(config, kind, property).into_iter()
            .filter_map(|(n, device)| {
                cache.entry(n)
                    .or_insert_with(|| source_readings(store, device))
                    .get(&property)
                    .map(|r| (r.clone(), device))
            })
            .collect();

        let fresh = |r: &Reading| now - r.timestamp <= stale_after;
        let chosen = candidates.iter()
            .find(|(r, d)| fresh(r) && store.is_steady(&d.source, property, now, return_after))
            .or_else(|| candidates.iter().find(|(r, _)| fresh(r)))
            .or_else(|| candidates.iter().max_by_key(|(r, _)| r.timestamp));
        if let Some((reading, _)) = chosen {
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

    fn wind(store: &mut WeatherStore, key: String, at: DateTime<Utc>, value: f64) {
        store.update(key, WeatherObservation::new(at, BTreeMap::from([(WeatherProperty::WindSpeed, value)])));
    }

    #[test]
    fn recovered_source_is_used_again_only_after_reporting_steadily() {
        let now = Utc::now();
        let (_, config) = setup(now, 10, 10);
        let mut store = WeatherStore::started_at(now - Duration::seconds(3600));
        let tempest = WeatherStore::tempest_key("ST-1");
        let ecowitt = WeatherStore::ecowitt_key("gw");

        // Ecowitt reports throughout; Tempest drops out for half an hour and is back 120s ago
        for t in (0..=3600).step_by(30) {
            wind(&mut store, ecowitt.clone(), now - Duration::seconds(3600 - t), 2.0);
        }
        for t in (0..=1800).step_by(60) {
            wind(&mut store, tempest.clone(), now - Duration::seconds(3600 - t), 1.0);
        }
        wind(&mut store, tempest.clone(), now - Duration::seconds(120), 1.0);
        wind(&mut store, tempest.clone(), now - Duration::seconds(60), 1.0);
        wind(&mut store, tempest.clone(), now, 1.0);

        // Tempest is fresh and first in priority, but has only been back for 120s
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 2);

        // Once it has reported steadily for 300s, it takes over again
        let later = now + Duration::seconds(200);
        for t in (60..=200).step_by(60) {
            wind(&mut store, tempest.clone(), now + Duration::seconds(t), 1.0);
            wind(&mut store, ecowitt.clone(), now + Duration::seconds(t), 2.0);
        }
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, later);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 1);
    }

    #[test]
    fn a_single_missed_read_does_not_switch_source_but_an_outage_does() {
        let now = Utc::now();
        let (_, config) = setup(now, 10, 10);
        let mut store = WeatherStore::started_at(now - Duration::seconds(3600));
        let tempest = WeatherStore::tempest_key("ST-1");
        let ecowitt = WeatherStore::ecowitt_key("gw");
        for t in (0..=3600).step_by(60) {
            wind(&mut store, tempest.clone(), now - Duration::seconds(3600 - t), 1.0);
            wind(&mut store, ecowitt.clone(), now - Duration::seconds(3600 - t), 2.0);
        }
        // Latest Tempest packet lacks wind speed (e.g. a failed read)
        store.update(tempest.clone(), WeatherObservation::new(now, BTreeMap::from([(WeatherProperty::Pressure, 890.0)])));

        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 1);
        assert_eq!(readings[&WeatherProperty::WindSpeed].value, 1.0);

        // If Tempest keeps omitting it, the carried value goes stale and the aggregate falls back
        let later = now + Duration::seconds(400);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, later);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 2);
    }

    #[test]
    fn primary_is_used_when_no_source_is_steady_yet() {
        let now = Utc::now();
        let (_, config) = setup(now, 10, 10);
        let mut store = WeatherStore::started_at(now - Duration::seconds(3600));
        // Both sources only just appeared, long after startup
        wind(&mut store, WeatherStore::tempest_key("ST-1"), now - Duration::seconds(30), 1.0);
        wind(&mut store, WeatherStore::ecowitt_key("gw"), now - Duration::seconds(10), 2.0);
        let readings = resolve_readings(&store, &config, WeatherDeviceKind::Aggregate, now);
        assert_eq!(readings[&WeatherProperty::WindSpeed].source_device, 1);
    }

    #[test]
    fn repeated_measurement_time_does_not_count_as_steady() {
        let now = Utc::now();
        let mut store = WeatherStore::started_at(now - Duration::seconds(3600));
        let source = ecowitt();
        let key = WeatherStore::ecowitt_key("gw");
        // A stuck sensor keeps reporting the same measurement time
        for _ in 0..20 {
            wind(&mut store, key.clone(), now - Duration::seconds(400), 2.0);
        }
        wind(&mut store, key.clone(), now, 2.0);
        assert!(!store.is_steady(&source, WeatherProperty::WindSpeed, now, Duration::seconds(300)));
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
