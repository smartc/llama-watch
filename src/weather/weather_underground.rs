//! Weather Underground personal weather station, polled from the PWS current
//! observations API (`api.weather.com/v2/pws/observations/current`). PWS API
//! keys are limited to 1500 calls/day, so the poll interval is at least 60s.
//! WU only has sea-level pressure; see `station_pressure`.
//! The API key is part of the request URL, so request errors are logged
//! without their URL.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::observation::{SharedWeatherStore, WeatherObservation, WeatherProperty, WeatherStore};

const API_URL: &str = "https://api.weather.com/v2/pws/observations/current";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const MIN_POLL_INTERVAL_SECONDS: u64 = 60;

/// Convert sea-level pressure back to station pressure with the hypsometric
/// equation, using the mean temperature of the air column between the station
/// and sea level (station temperature plus the standard lapse rate). Stations
/// such as Ecowitt gateways reduce to sea level this way, so the round trip is
/// within about 1 hPa. Without a temperature, the standard atmosphere is assumed.
pub fn station_pressure(sea_level_hpa: f64, altitude_m: f64, temperature_c: Option<f64>) -> f64 {
    const G: f64 = 9.80665; // m/s²
    const R_DRY_AIR: f64 = 287.05; // J/(kg·K)
    const LAPSE_RATE: f64 = 0.0065; // K/m
    let station_k = temperature_c.map(|t| t + 273.15).unwrap_or(288.15 - LAPSE_RATE * altitude_m);
    let mean_k = station_k + LAPSE_RATE * altitude_m / 2.0;
    sea_level_hpa / (G * altitude_m / (R_DRY_AIR * mean_k)).exp()
}

/// Parse a PWS current observations response (requested with units=m).
/// WU only reports pressure reduced to sea level, while ASCOM Pressure is the
/// pressure at the station, so Pressure is only provided when the station's
/// altitude is known.
pub fn parse_current(data: &Value, pressure_altitude_m: Option<f64>) -> Result<WeatherObservation> {
    let obs = data.get("observations")
        .and_then(Value::as_array)
        .and_then(|o| o.first())
        .context("Response has no observations")?;
    let metric = obs.get("metric").context("Observation has no metric values")?;

    let timestamp = obs.get("obsTimeUtc")
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
        .or_else(|| obs.get("epoch").and_then(Value::as_i64).and_then(|e| DateTime::from_timestamp(e, 0)))
        .context("Observation has no time")?;

    if obs.get("qcStatus").and_then(Value::as_i64) == Some(-1) {
        debug!("Weather Underground observation failed quality control; using it anyway");
    }

    let number = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|v| v.is_finite());
    let fields = [
        (WeatherProperty::Temperature, number(metric.get("temp"))),
        (WeatherProperty::Humidity, number(obs.get("humidity"))),
        (WeatherProperty::DewPoint, number(metric.get("dewpt"))),
        // units=m reports wind in km/h
        (WeatherProperty::WindSpeed, number(metric.get("windSpeed")).map(|v| v / 3.6)),
        (WeatherProperty::WindGust, number(metric.get("windGust")).map(|v| v / 3.6)),
        (WeatherProperty::WindDirection, number(obs.get("winddir"))),
        (WeatherProperty::RainRate, number(metric.get("precipRate"))),
        (WeatherProperty::SolarRadiation, number(obs.get("solarRadiation"))),
        (WeatherProperty::UvIndex, number(obs.get("uv"))),
    ];
    let mut values: BTreeMap<_, _> = fields.into_iter()
        .filter_map(|(property, value)| value.map(|v| (property, v)))
        .collect();

    if let (Some(altitude), Some(sea_level)) = (pressure_altitude_m, number(metric.get("pressure"))) {
        let temperature = values.get(&WeatherProperty::Temperature).copied();
        values.insert(WeatherProperty::Pressure, station_pressure(sea_level, altitude, temperature));
    }

    Ok(WeatherObservation::new(timestamp, values))
}

async fn fetch(client: &reqwest::Client, station_id: &str, api_key: &str, pressure_altitude_m: Option<f64>) -> Result<WeatherObservation> {
    let response = client.get(API_URL)
        .query(&[
            ("stationId", station_id),
            ("format", "json"),
            ("units", "m"),
            ("numericPrecision", "decimal"),
            ("apiKey", api_key),
        ])
        .send().await
        .map_err(reqwest::Error::without_url)?;

    match response.status().as_u16() {
        200 => {}
        204 => anyhow::bail!("No recent observations for station {}", station_id),
        401 => anyhow::bail!("API key was rejected (HTTP 401)"),
        429 => anyhow::bail!("Rate limited by Weather Underground (HTTP 429); increase the poll interval"),
        code => anyhow::bail!("HTTP {}", code),
    }
    let data: Value = response.json().await.map_err(reqwest::Error::without_url)?;
    parse_current(&data, pressure_altitude_m)
}

/// Polls one Weather Underground station and writes its observations to the weather store
pub async fn poll_station(
    station_id: String,
    api_key: String,
    interval_seconds: u64,
    pressure_altitude_m: Option<f64>,
    store: SharedWeatherStore,
) {
    let key = WeatherStore::weather_underground_key(&station_id);
    let interval = Duration::from_secs(interval_seconds.max(MIN_POLL_INTERVAL_SECONDS));
    let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create HTTP client for Weather Underground station {}: {}", station_id, e);
            return;
        }
    };

    info!("Polling Weather Underground station {} every {}s", station_id, interval.as_secs());
    let mut failing = false;
    loop {
        match fetch(&client, &station_id, &api_key, pressure_altitude_m).await {
            Ok(observation) => {
                if failing {
                    info!("Weather Underground station {} is responding again", station_id);
                    failing = false;
                }
                debug!("Weather Underground {}: {:?}", station_id, observation);
                store.write().await.update(key.clone(), observation);
            }
            Err(e) if !failing => {
                warn!("Failed to poll Weather Underground station {}: {:#}", station_id, e);
                failing = true;
            }
            Err(e) => debug!("Weather Underground station {} still failing: {:#}", station_id, e),
        }

        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn approx(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 0.01)
    }

    #[test]
    fn parses_pws_current_observation() {
        let data: Value = serde_json::from_str(include_str!("testdata/wu_pws_current.json")).unwrap();
        let obs = parse_current(&data, None).unwrap();

        assert_eq!(obs.timestamp.to_rfc3339(), "2026-10-11T15:57:39+00:00");
        assert!(approx(obs.get(WeatherProperty::Temperature), -1.4));
        assert!(approx(obs.get(WeatherProperty::Humidity), 96.0));
        assert!(approx(obs.get(WeatherProperty::DewPoint), -2.0));
        assert!(approx(obs.get(WeatherProperty::WindSpeed), 3.2 / 3.6));
        assert!(approx(obs.get(WeatherProperty::WindGust), 5.4 / 3.6));
        assert!(approx(obs.get(WeatherProperty::WindDirection), 342.0));
        assert!(approx(obs.get(WeatherProperty::RainRate), 0.0));
        assert!(approx(obs.get(WeatherProperty::SolarRadiation), 85.6));
        assert_eq!(obs.get(WeatherProperty::Pressure), None);
    }

    #[test]
    fn converts_sea_level_pressure_back_to_station_pressure() {
        // Simultaneous readings: GW3000 at 1100 m reported abs 893.1 / rel 1023.0 hPa
        // at -1.4 °C, and uploaded 1022.99 to Weather Underground
        let station = station_pressure(1022.99, 1100.0, Some(-1.4));
        assert!((station - 893.1).abs() < 1.0, "station pressure {}", station);

        // No altitude configured: no Pressure; altitude configured: converted value
        let data: Value = serde_json::from_str(include_str!("testdata/wu_pws_current.json")).unwrap();
        assert_eq!(parse_current(&data, None).unwrap().get(WeatherProperty::Pressure), None);
        let pressure = parse_current(&data, Some(1100.0)).unwrap().get(WeatherProperty::Pressure).unwrap();
        assert!((pressure - station_pressure(1023.1, 1100.0, Some(-1.4))).abs() < 1e-9);
        assert_eq!(station_pressure(1013.25, 0.0, Some(15.0)), 1013.25);
    }

    #[test]
    fn missing_values_are_omitted() {
        let data = json!({"observations": [{
            "obsTimeUtc": "2026-10-11T15:57:39Z",
            "humidity": null,
            "winddir": 90,
            "metric": {"temp": 5.0, "windSpeed": null}
        }]});
        let obs = parse_current(&data, None).unwrap();
        assert!(approx(obs.get(WeatherProperty::Temperature), 5.0));
        assert_eq!(obs.get(WeatherProperty::Humidity), None);
        assert_eq!(obs.get(WeatherProperty::WindSpeed), None);
        assert!(approx(obs.get(WeatherProperty::WindDirection), 90.0));
    }

    #[test]
    fn rejects_empty_response() {
        assert!(parse_current(&json!({"observations": []}), None).is_err());
        assert!(parse_current(&json!({}), None).is_err());
    }
}
