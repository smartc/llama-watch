use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::models::WeatherDataSource;

/// Weather quantities LLAMA understands. All values are stored in metric units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherProperty {
    Temperature,     // °C
    Humidity,        // %
    DewPoint,        // °C
    Pressure,        // hPa, station (not sea-level) pressure
    WindSpeed,       // m/s
    WindGust,        // m/s
    WindDirection,   // degrees
    RainRate,        // mm/hour
    SkyBrightness,   // lux (ASCOM definition)
    SkyQuality,      // mag/arcsec²
    SkyTemperature,  // °C
    CloudCover,      // %
    SolarRadiation,  // W/m² (not an ASCOM property)
    UvIndex,         // index (not an ASCOM property)
}

impl WeatherProperty {
    pub const ALL: [WeatherProperty; 14] = [
        WeatherProperty::Temperature,
        WeatherProperty::Humidity,
        WeatherProperty::DewPoint,
        WeatherProperty::Pressure,
        WeatherProperty::WindSpeed,
        WeatherProperty::WindGust,
        WeatherProperty::WindDirection,
        WeatherProperty::RainRate,
        WeatherProperty::SkyBrightness,
        WeatherProperty::SkyQuality,
        WeatherProperty::SkyTemperature,
        WeatherProperty::CloudCover,
        WeatherProperty::SolarRadiation,
        WeatherProperty::UvIndex,
    ];

    /// ASCOM ObservingConditions property name, or None if ASCOM has no such property
    pub fn ascom_name(self) -> Option<&'static str> {
        match self {
            WeatherProperty::Temperature => Some("Temperature"),
            WeatherProperty::Humidity => Some("Humidity"),
            WeatherProperty::DewPoint => Some("DewPoint"),
            WeatherProperty::Pressure => Some("Pressure"),
            WeatherProperty::WindSpeed => Some("WindSpeed"),
            WeatherProperty::WindGust => Some("WindGust"),
            WeatherProperty::WindDirection => Some("WindDirection"),
            WeatherProperty::RainRate => Some("RainRate"),
            WeatherProperty::SkyBrightness => Some("SkyBrightness"),
            WeatherProperty::SkyQuality => Some("SkyQuality"),
            WeatherProperty::SkyTemperature => Some("SkyTemperature"),
            WeatherProperty::CloudCover => Some("CloudCover"),
            WeatherProperty::SolarRadiation | WeatherProperty::UvIndex => None,
        }
    }

    pub fn from_ascom_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter()
            .find(|p| p.ascom_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }

    /// Display label, also used as the measurement key in weather status
    pub fn label(self) -> &'static str {
        match self {
            WeatherProperty::Temperature => "Temperature",
            WeatherProperty::Humidity => "Humidity",
            WeatherProperty::DewPoint => "Dew Point",
            WeatherProperty::Pressure => "Pressure",
            WeatherProperty::WindSpeed => "Wind Speed",
            WeatherProperty::WindGust => "Wind Gust",
            WeatherProperty::WindDirection => "Wind Direction",
            WeatherProperty::RainRate => "Rain Rate",
            WeatherProperty::SkyBrightness => "Sky Brightness",
            WeatherProperty::SkyQuality => "Sky Quality",
            WeatherProperty::SkyTemperature => "Sky Temperature",
            WeatherProperty::CloudCover => "Cloud Cover",
            WeatherProperty::SolarRadiation => "Solar Radiation",
            WeatherProperty::UvIndex => "UV Index",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            WeatherProperty::Temperature | WeatherProperty::DewPoint | WeatherProperty::SkyTemperature => "°C",
            WeatherProperty::Humidity | WeatherProperty::CloudCover => "%",
            WeatherProperty::SkyQuality => "mag/arcsec²",
            WeatherProperty::Pressure => "hPa",
            WeatherProperty::WindSpeed | WeatherProperty::WindGust => "m/s",
            WeatherProperty::WindDirection => "°",
            WeatherProperty::RainRate => "mm/hr",
            WeatherProperty::SkyBrightness => "lux",
            WeatherProperty::SolarRadiation => "W/m²",
            WeatherProperty::UvIndex => "",
        }
    }

    /// Label with unit, e.g. "Temperature (°C)"
    pub fn display_name(self) -> String {
        match self.unit() {
            "" => self.label().to_string(),
            unit => format!("{} ({})", self.label(), unit),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            WeatherProperty::Temperature => "Ambient air temperature",
            WeatherProperty::Humidity => "Relative humidity",
            WeatherProperty::DewPoint => "Dew point",
            WeatherProperty::Pressure => "Atmospheric pressure at the station",
            WeatherProperty::WindSpeed => "Average wind speed",
            WeatherProperty::WindGust => "Wind gust",
            WeatherProperty::WindDirection => "Wind direction",
            WeatherProperty::RainRate => "Rain rate",
            WeatherProperty::SkyBrightness => "Sky brightness (illuminance)",
            WeatherProperty::SkyQuality => "Sky quality",
            WeatherProperty::SkyTemperature => "Sky temperature",
            WeatherProperty::CloudCover => "Cloud cover",
            WeatherProperty::SolarRadiation => "Solar radiation",
            WeatherProperty::UvIndex => "UV index",
        }
    }
}

/// A single snapshot from one weather station, in metric units
#[derive(Debug, Clone)]
pub struct WeatherObservation {
    pub timestamp: DateTime<Utc>,
    values: BTreeMap<WeatherProperty, f64>,
    /// When individual values were measured, if the station reports it and it
    /// differs from the observation timestamp
    measured_at: BTreeMap<WeatherProperty, DateTime<Utc>>,
}

impl WeatherObservation {
    /// Build an observation, deriving dew point from temperature and humidity
    /// when the station doesn't report it directly
    pub fn new(timestamp: DateTime<Utc>, mut values: BTreeMap<WeatherProperty, f64>) -> Self {
        if !values.contains_key(&WeatherProperty::DewPoint) {
            if let (Some(t), Some(h)) = (
                values.get(&WeatherProperty::Temperature),
                values.get(&WeatherProperty::Humidity),
            ) {
                if *h > 0.0 {
                    values.insert(WeatherProperty::DewPoint, calculate_dew_point(*t, *h));
                }
            }
        }
        Self { timestamp, values, measured_at: BTreeMap::new() }
    }

    /// Record that a value was measured at a different time than the observation
    pub fn with_measured_at(mut self, measured_at: BTreeMap<WeatherProperty, DateTime<Utc>>) -> Self {
        self.measured_at = measured_at;
        self
    }

    pub fn get(&self, property: WeatherProperty) -> Option<f64> {
        self.values.get(&property).copied()
    }

    /// Keep values the previous observation had but this one lacks (a read that
    /// failed this time, or a sensor the station briefly didn't report), with
    /// their original measurement time, so they age normally instead of
    /// disappearing until the next poll
    fn carry_forward(&mut self, previous: &WeatherObservation) {
        for (&property, &value) in &previous.values {
            if !self.values.contains_key(&property) {
                self.values.insert(property, value);
                self.measured_at.insert(property, previous.measured_at(property));
            }
        }
    }

    /// When a value was measured (the observation timestamp unless the station reported otherwise)
    pub fn measured_at(&self, property: WeatherProperty) -> DateTime<Utc> {
        self.measured_at.get(&property).copied().unwrap_or(self.timestamp)
    }
}

/// Dew point from temperature (°C) and relative humidity (%) using the Magnus formula
pub fn calculate_dew_point(temperature: f64, humidity: f64) -> f64 {
    let a = 17.27;
    let b = 237.7;
    let alpha = ((a * temperature) / (b + temperature)) + (humidity / 100.0).ln();
    (b * alpha) / (a - alpha)
}

/// A value served by a weather device, with where and when it came from
#[derive(Debug, Clone)]
pub struct Reading {
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    /// Device number of the source the value was taken from
    pub source_device: u32,
}

pub type Readings = BTreeMap<WeatherProperty, Reading>;

/// Gap between measurements that breaks a source's "reporting steadily" streak,
/// until configuration says otherwise (aggregate stale_after_seconds)
pub const DEFAULT_GAP_LIMIT_SECONDS: i64 = 300;

/// How long a source has been reporting a value without gaps
#[derive(Debug, Clone, Copy)]
struct Streak {
    since: DateTime<Utc>,
    last: DateTime<Utc>,
}

/// Latest observation from every weather station heard from, keyed by station
#[derive(Debug)]
pub struct WeatherStore {
    observations: HashMap<String, WeatherObservation>,
    streaks: HashMap<String, BTreeMap<WeatherProperty, Streak>>,
    started: DateTime<Utc>,
    gap_limit: Duration,
}

impl Default for WeatherStore {
    fn default() -> Self {
        Self {
            observations: HashMap::new(),
            streaks: HashMap::new(),
            started: Utc::now(),
            gap_limit: Duration::seconds(DEFAULT_GAP_LIMIT_SECONDS),
        }
    }
}

pub type SharedWeatherStore = Arc<RwLock<WeatherStore>>;

impl WeatherStore {
    pub fn tempest_key(serial_number: &str) -> String {
        format!("tempest:{}", serial_number)
    }

    pub fn ecowitt_key(host: &str) -> String {
        format!("ecowitt:{}", host.trim().to_ascii_lowercase())
    }

    pub fn weather_underground_key(station_id: &str) -> String {
        format!("wu:{}", station_id.trim().to_ascii_uppercase())
    }

    pub fn alpaca_key(host: &str, port: u16, device_number: u32) -> String {
        format!("alpaca:{}:{}:{}", host.trim().to_ascii_lowercase(), port, device_number)
    }

    #[cfg(test)]
    pub fn started_at(started: DateTime<Utc>) -> Self {
        Self { started, ..Self::default() }
    }

    /// Whether LLAMA started recently enough that sources may still be making
    /// their first reports
    pub fn starting_up(&self, now: DateTime<Utc>) -> bool {
        now - self.started < self.gap_limit
    }

    /// Gap between measurements that counts as the source having dropped out
    pub fn set_gap_limit(&mut self, gap_limit: Duration) {
        self.gap_limit = gap_limit;
    }

    pub fn update(&mut self, key: String, mut observation: WeatherObservation) {
        if let Some(previous) = self.observations.get(&key) {
            observation.carry_forward(previous);
        }
        let streaks = self.streaks.entry(key.clone()).or_default();
        for property in WeatherProperty::ALL {
            if observation.get(property).is_none() {
                continue;
            }
            let measured = observation.measured_at(property);
            match streaks.get_mut(&property) {
                // Same measurement reported again (e.g. a stuck sensor): no new data
                Some(streak) if measured <= streak.last => {}
                Some(streak) => {
                    if measured - streak.last > self.gap_limit {
                        streak.since = measured;
                    }
                    streak.last = measured;
                }
                None => {
                    streaks.insert(property, Streak { since: measured, last: measured });
                }
            }
        }
        self.observations.insert(key, observation);
    }

    /// Store key of the station a configured data source currently reads from
    pub fn key_for(&self, source: &WeatherDataSource) -> Option<String> {
        match source {
            WeatherDataSource::Tempest { serial_number: Some(sn) } => Some(Self::tempest_key(sn)),
            WeatherDataSource::Tempest { serial_number: None } => {
                // Any Tempest on the network; prefer the most recently heard one
                self.observations.iter()
                    .filter(|(key, _)| key.starts_with("tempest:"))
                    .max_by_key(|(_, obs)| obs.timestamp)
                    .map(|(key, _)| key.clone())
            }
            WeatherDataSource::Ecowitt { host, .. } => Some(Self::ecowitt_key(host)),
            WeatherDataSource::Alpaca { host, port, device_number, .. } => {
                Some(Self::alpaca_key(host, *port, *device_number))
            }
            WeatherDataSource::WeatherUnderground { station_id, .. } => {
                Some(Self::weather_underground_key(station_id))
            }
        }
    }

    /// Latest observation for a configured data source
    pub fn observation_for(&self, source: &WeatherDataSource) -> Option<&WeatherObservation> {
        self.key_for(source).and_then(|key| self.observations.get(&key))
    }

    /// Whether a source has reported a value without gaps for at least `period`,
    /// or without gaps since LLAMA started
    pub fn is_steady(&self, source: &WeatherDataSource, property: WeatherProperty, now: DateTime<Utc>, period: Duration) -> bool {
        let Some(streak) = self.key_for(source)
            .and_then(|key| self.streaks.get(&key))
            .and_then(|streaks| streaks.get(&property))
        else {
            return false;
        };
        now - streak.since >= period || streak.since - self.started <= self.gap_limit
    }
}

/// Properties a data source can provide. This is fixed by configuration so
/// that ASCOM clients see a stable set of implemented properties.
pub fn supported_properties(source: &WeatherDataSource) -> Vec<WeatherProperty> {
    match source {
        // Tempest has no sky quality, sky temperature or cloud sensors
        WeatherDataSource::Tempest { .. } => WeatherProperty::ALL.into_iter()
            .filter(|p| !matches!(p, WeatherProperty::SkyQuality | WeatherProperty::SkyTemperature | WeatherProperty::CloudCover))
            .collect(),
        // No light sensor usable at night, so no SkyBrightness
        WeatherDataSource::Ecowitt { .. } => vec![
            WeatherProperty::Temperature,
            WeatherProperty::Humidity,
            WeatherProperty::DewPoint,
            WeatherProperty::Pressure,
            WeatherProperty::WindSpeed,
            WeatherProperty::WindGust,
            WeatherProperty::WindDirection,
            WeatherProperty::RainRate,
            WeatherProperty::SolarRadiation,
            WeatherProperty::UvIndex,
        ],
        // Whatever was selected when the source was configured
        WeatherDataSource::Alpaca { properties, .. } => properties.clone(),
        // WU only has sea-level pressure; it is converted to station pressure
        // when the station's altitude is configured
        WeatherDataSource::WeatherUnderground { pressure_altitude_m, .. } => [
            WeatherProperty::Temperature,
            WeatherProperty::Humidity,
            WeatherProperty::DewPoint,
            WeatherProperty::WindSpeed,
            WeatherProperty::WindGust,
            WeatherProperty::WindDirection,
            WeatherProperty::RainRate,
            WeatherProperty::SolarRadiation,
            WeatherProperty::UvIndex,
        ].into_iter()
            .chain(pressure_altitude_m.map(|_| WeatherProperty::Pressure))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dew_point_is_derived_when_not_reported() {
        let values = BTreeMap::from([
            (WeatherProperty::Temperature, 20.0),
            (WeatherProperty::Humidity, 50.0),
        ]);
        let obs = WeatherObservation::new(Utc::now(), values);
        let dp = obs.get(WeatherProperty::DewPoint).unwrap();
        assert!((dp - 9.26).abs() < 0.05, "dew point {}", dp);
    }

    #[test]
    fn reported_dew_point_is_kept() {
        let values = BTreeMap::from([
            (WeatherProperty::Temperature, 20.0),
            (WeatherProperty::Humidity, 50.0),
            (WeatherProperty::DewPoint, 1.0),
        ]);
        let obs = WeatherObservation::new(Utc::now(), values);
        assert_eq!(obs.get(WeatherProperty::DewPoint), Some(1.0));
    }

    #[test]
    fn values_missing_from_an_update_are_kept_with_their_original_time() {
        let t0 = Utc::now() - Duration::seconds(60);
        let t1 = t0 + Duration::seconds(30);
        let mut store = WeatherStore::default();
        let key = WeatherStore::ecowitt_key("gw");
        let source = WeatherDataSource::Ecowitt { host: "gw".into(), poll_interval_seconds: 30 };

        store.update(key.clone(), WeatherObservation::new(t0, BTreeMap::from([
            (WeatherProperty::Temperature, 5.0),
            (WeatherProperty::CloudCover, 40.0),
        ])));
        // Next poll: the cloud cover read failed
        store.update(key.clone(), WeatherObservation::new(t1, BTreeMap::from([
            (WeatherProperty::Temperature, 6.0),
        ])));

        let obs = store.observation_for(&source).unwrap();
        assert_eq!(obs.get(WeatherProperty::Temperature), Some(6.0));
        assert_eq!(obs.measured_at(WeatherProperty::Temperature), t1);
        assert_eq!(obs.get(WeatherProperty::CloudCover), Some(40.0));
        assert_eq!(obs.measured_at(WeatherProperty::CloudCover), t0);

        // The carried value doesn't count as new data for the steady-reporting streak
        let streak = store.streaks[&key][&WeatherProperty::CloudCover];
        assert_eq!(streak.last, t0);
    }

    #[test]
    fn weather_underground_provides_pressure_only_with_altitude() {
        let source = |pressure_altitude_m| WeatherDataSource::WeatherUnderground {
            station_id: "ISTATION1".into(),
            api_key: "key".into(),
            poll_interval_seconds: 120,
            pressure_altitude_m,
        };
        assert!(!supported_properties(&source(None)).contains(&WeatherProperty::Pressure));
        assert!(supported_properties(&source(Some(1100.0))).contains(&WeatherProperty::Pressure));
    }

    #[test]
    fn ascom_names_round_trip_case_insensitively() {
        assert_eq!(WeatherProperty::from_ascom_name("skybrightness"), Some(WeatherProperty::SkyBrightness));
        assert_eq!(WeatherProperty::from_ascom_name("DewPoint"), Some(WeatherProperty::DewPoint));
        assert_eq!(WeatherProperty::from_ascom_name("SolarRadiation"), None);
        assert_eq!(WeatherProperty::from_ascom_name("SkyQuality"), Some(WeatherProperty::SkyQuality));
        assert_eq!(WeatherProperty::from_ascom_name("StarFWHM"), None);
    }
}
