//! Ecowitt gateway (GW1100/GW2000/GW3000 and similar) polled over the local
//! HTTP API. `GET http://<gateway>/get_livedata_info` returns JSON whose values
//! are strings carrying units that follow the gateway's display settings, so
//! every value is converted to metric based on the unit it arrives with.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::observation::{SharedWeatherStore, WeatherObservation, WeatherProperty, WeatherStore};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MIN_POLL_INTERVAL_SECONDS: u64 = 5;

/// Lux per W/m² used by Ecowitt when reporting light as illuminance
const LUX_PER_WATT_M2: f64 = 126.7;

pub fn livedata_url(host: &str) -> String {
    let base = if host.contains("://") { host.to_string() } else { format!("http://{}", host) };
    format!("{}/get_livedata_info", base.trim_end_matches('/'))
}

/// Parse a `get_livedata_info` response into an observation
pub fn parse_livedata(data: &Value, timestamp: DateTime<Utc>) -> Result<WeatherObservation> {
    let common = data.get("common_list")
        .and_then(Value::as_array)
        .context("Response has no common_list")?;

    let mut values = BTreeMap::new();

    for item in common {
        let Some(id) = item.get("id").and_then(Value::as_str) else { continue };
        let property = match id.to_ascii_lowercase().as_str() {
            "0x02" => WeatherProperty::Temperature,
            "0x07" => WeatherProperty::Humidity,
            "0x03" => WeatherProperty::DewPoint,
            "0x0b" => WeatherProperty::WindSpeed,
            "0x0c" => WeatherProperty::WindGust,
            "0x0a" => WeatherProperty::WindDirection,
            "0x15" => WeatherProperty::SolarRadiation,
            "0x17" => WeatherProperty::UvIndex,
            _ => continue,
        };
        if let Some(value) = measurement(item, "val").and_then(|(v, unit)| to_metric(property, v, &unit)) {
            values.insert(property, value);
        }
    }

    // Prefer the tipping-bucket gauge, fall back to a piezo gauge (e.g. WS90)
    let rain_rate = ["rain", "piezoRain"].iter()
        .filter_map(|section| data.get(section).and_then(Value::as_array))
        .flat_map(|items| items.iter())
        .filter(|item| item.get("id").and_then(Value::as_str).is_some_and(|id| id.eq_ignore_ascii_case("0x0e")))
        .find_map(|item| measurement(item, "val").and_then(|(v, unit)| to_metric(WeatherProperty::RainRate, v, &unit)));
    if let Some(rate) = rain_rate {
        values.insert(WeatherProperty::RainRate, rate);
    }

    // Station (absolute) pressure from the gateway's built-in sensor
    let pressure = data.get("wh25")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|wh25| measurement(wh25, "abs"))
        .and_then(|(v, unit)| to_metric(WeatherProperty::Pressure, v, &unit));
    if let Some(pressure) = pressure {
        values.insert(WeatherProperty::Pressure, pressure);
    }

    Ok(WeatherObservation::new(timestamp, values))
}

/// Read a numeric value and its unit from an item. The unit is either in a
/// separate "unit" field or follows the number inside the value string
/// ("13.32 km/h", "94%"). Missing readings ("--") return None.
fn measurement(item: &Value, key: &str) -> Option<(f64, String)> {
    let raw = match item.get(key)? {
        Value::Number(n) => return n.as_f64().map(|v| (v, unit_field(item))),
        Value::String(s) => s.trim(),
        _ => return None,
    };
    let split = raw.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(raw.len());
    let value: f64 = raw[..split].parse().ok()?;
    let inline_unit = raw[split..].trim();
    let unit = if inline_unit.is_empty() { unit_field(item) } else { inline_unit.to_string() };
    Some((value, unit))
}

fn unit_field(item: &Value) -> String {
    item.get("unit").and_then(Value::as_str).unwrap_or("").trim().to_string()
}

/// Convert a value to the metric unit LLAMA stores. Unknown units return None
/// rather than guessing.
fn to_metric(property: WeatherProperty, value: f64, unit: &str) -> Option<f64> {
    let unit = unit.trim_start_matches('°').to_ascii_lowercase();
    let converted = match property {
        WeatherProperty::Temperature | WeatherProperty::DewPoint => match unit.as_str() {
            "c" | "℃" => value,
            "f" | "℉" => (value - 32.0) * 5.0 / 9.0,
            _ => return unknown_unit(property, &unit),
        },
        WeatherProperty::WindSpeed | WeatherProperty::WindGust => match unit.as_str() {
            "m/s" => value,
            "km/h" => value / 3.6,
            "mph" => value * 0.44704,
            "knots" | "knot" | "kn" => value * 0.514444,
            "ft/s" => value * 0.3048,
            _ => return unknown_unit(property, &unit),
        },
        WeatherProperty::Pressure => match unit.as_str() {
            "hpa" | "mbar" | "mb" => value,
            "kpa" => value * 10.0,
            "inhg" => value * 33.8639,
            "mmhg" => value * 1.33322,
            _ => return unknown_unit(property, &unit),
        },
        WeatherProperty::RainRate => match unit.as_str() {
            "mm/hr" | "mm/h" => value,
            "in/hr" | "in/h" => value * 25.4,
            _ => return unknown_unit(property, &unit),
        },
        WeatherProperty::SolarRadiation => match unit.as_str() {
            "w/m2" | "w/m²" => value,
            "klux" => value * 1000.0 / LUX_PER_WATT_M2,
            "kfc" => value * 10.7639 * 1000.0 / LUX_PER_WATT_M2,
            _ => return unknown_unit(property, &unit),
        },
        WeatherProperty::Humidity | WeatherProperty::WindDirection | WeatherProperty::UvIndex => value,
        WeatherProperty::SkyBrightness
        | WeatherProperty::SkyQuality
        | WeatherProperty::SkyTemperature
        | WeatherProperty::CloudCover => return None,
    };
    Some(converted)
}

fn unknown_unit(property: WeatherProperty, unit: &str) -> Option<f64> {
    debug!("Ignoring Ecowitt {} with unrecognised unit '{}'", property.label(), unit);
    None
}

/// Polls one Ecowitt gateway and writes its observations to the weather store
pub async fn poll_gateway(host: String, interval_seconds: u64, store: SharedWeatherStore) {
    let url = livedata_url(&host);
    let key = WeatherStore::ecowitt_key(&host);
    let interval = Duration::from_secs(interval_seconds.max(MIN_POLL_INTERVAL_SECONDS));
    let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create HTTP client for Ecowitt gateway {}: {}", host, e);
            return;
        }
    };

    info!("Polling Ecowitt gateway {} every {}s", url, interval.as_secs());
    let mut failing = false;
    loop {
        let result = async {
            let data: Value = client.get(&url).send().await?
                .error_for_status()?
                .json().await?;
            parse_livedata(&data, Utc::now())
        }.await;

        match result {
            Ok(observation) => {
                if failing {
                    info!("Ecowitt gateway {} is responding again", host);
                    failing = false;
                }
                debug!("Ecowitt {}: {:?}", host, observation);
                store.write().await.update(key.clone(), observation);
            }
            Err(e) if !failing => {
                warn!("Failed to poll Ecowitt gateway {}: {:#}", host, e);
                failing = true;
            }
            Err(e) => debug!("Ecowitt gateway {} still failing: {:#}", host, e),
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
    fn parses_gw3000_livedata() {
        let data: Value = serde_json::from_str(include_str!("testdata/ecowitt_gw3000_livedata.json")).unwrap();
        let obs = parse_livedata(&data, Utc::now()).unwrap();

        assert!(approx(obs.get(WeatherProperty::Temperature), -1.4));
        assert!(approx(obs.get(WeatherProperty::Humidity), 94.0));
        assert!(approx(obs.get(WeatherProperty::DewPoint), -2.2));
        assert!(approx(obs.get(WeatherProperty::WindSpeed), 7.92 / 3.6));
        assert!(approx(obs.get(WeatherProperty::WindGust), 11.16 / 3.6));
        assert!(approx(obs.get(WeatherProperty::WindDirection), 328.0));
        assert!(approx(obs.get(WeatherProperty::RainRate), 0.0));
        assert!(approx(obs.get(WeatherProperty::Pressure), 892.4));
        assert!(approx(obs.get(WeatherProperty::SolarRadiation), 0.02));
        assert!(approx(obs.get(WeatherProperty::UvIndex), 0.0));
        assert_eq!(obs.get(WeatherProperty::SkyBrightness), None);
    }

    #[test]
    fn converts_imperial_units() {
        let data = json!({
            "common_list": [
                {"id": "0x02", "val": "50.0", "unit": "F"},
                {"id": "0x0B", "val": "10.0 mph"},
                {"id": "0x15", "val": "12.67 Klux"}
            ],
            "rain": [{"id": "0x0E", "val": "0.10 in/Hr"}],
            "wh25": [{"intemp": "68.0", "unit": "F", "abs": "29.92 inHg"}]
        });
        let obs = parse_livedata(&data, Utc::now()).unwrap();
        assert!(approx(obs.get(WeatherProperty::Temperature), 10.0));
        assert!(approx(obs.get(WeatherProperty::WindSpeed), 4.4704));
        assert!(approx(obs.get(WeatherProperty::RainRate), 2.54));
        assert!(approx(obs.get(WeatherProperty::Pressure), 1013.21));
        assert!(approx(obs.get(WeatherProperty::SolarRadiation), 100.0));
    }

    #[test]
    fn missing_and_unknown_values_are_omitted() {
        let data = json!({
            "common_list": [
                {"id": "0x02", "val": "--", "unit": "C"},
                {"id": "0x07", "val": "--%"},
                {"id": "0x0B", "val": "3 Bft"}
            ],
            "piezoRain": [{"id": "0x0E", "val": "1.2 mm/Hr"}]
        });
        let obs = parse_livedata(&data, Utc::now()).unwrap();
        assert_eq!(obs.get(WeatherProperty::Temperature), None);
        assert_eq!(obs.get(WeatherProperty::Humidity), None);
        assert_eq!(obs.get(WeatherProperty::DewPoint), None);
        assert_eq!(obs.get(WeatherProperty::WindSpeed), None);
        assert!(approx(obs.get(WeatherProperty::RainRate), 1.2));
    }

    #[test]
    fn rejects_unexpected_response() {
        assert!(parse_livedata(&json!({"error": "nope"}), Utc::now()).is_err());
    }

    #[test]
    fn builds_url_from_host() {
        assert_eq!(livedata_url("ecowitt.local"), "http://ecowitt.local/get_livedata_info");
        assert_eq!(livedata_url("http://10.0.0.5/"), "http://10.0.0.5/get_livedata_info");
    }
}
