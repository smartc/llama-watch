//! Weather Underground personal weather station, polled from the PWS current
//! observations API (`api.weather.com/v2/pws/observations/current`). PWS API
//! keys are limited to 1500 calls/day, so the poll interval is at least 60s.
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

/// Parse a PWS current observations response (requested with units=m).
/// Pressure is not used: WU reports it reduced to sea level, while ASCOM
/// Pressure is the pressure at the station.
pub fn parse_current(data: &Value) -> Result<WeatherObservation> {
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
    let values: BTreeMap<_, _> = fields.into_iter()
        .filter_map(|(property, value)| value.map(|v| (property, v)))
        .collect();

    Ok(WeatherObservation::new(timestamp, values))
}

async fn fetch(client: &reqwest::Client, station_id: &str, api_key: &str) -> Result<WeatherObservation> {
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
    parse_current(&data)
}

/// Polls one Weather Underground station and writes its observations to the weather store
pub async fn poll_station(station_id: String, api_key: String, interval_seconds: u64, store: SharedWeatherStore) {
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
        match fetch(&client, &station_id, &api_key).await {
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
        let obs = parse_current(&data).unwrap();

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
    fn missing_values_are_omitted() {
        let data = json!({"observations": [{
            "obsTimeUtc": "2026-10-11T15:57:39Z",
            "humidity": null,
            "winddir": 90,
            "metric": {"temp": 5.0, "windSpeed": null}
        }]});
        let obs = parse_current(&data).unwrap();
        assert!(approx(obs.get(WeatherProperty::Temperature), 5.0));
        assert_eq!(obs.get(WeatherProperty::Humidity), None);
        assert_eq!(obs.get(WeatherProperty::WindSpeed), None);
        assert!(approx(obs.get(WeatherProperty::WindDirection), 90.0));
    }

    #[test]
    fn rejects_empty_response() {
        assert!(parse_current(&json!({"observations": []})).is_err());
        assert!(parse_current(&json!({})).is_err());
    }
}
