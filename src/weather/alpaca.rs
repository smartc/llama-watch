//! ASCOM Alpaca ObservingConditions device used as a weather source. Each
//! configured property is read with `GET /api/v1/observingconditions/{n}/{property}`,
//! and its age with `timesincelastupdate`, so a sensor that stops updating shows
//! as stale even while the device keeps answering. The remote device's
//! Connected state is never changed.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::observation::{SharedWeatherStore, WeatherObservation, WeatherProperty, WeatherStore};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_POLL_INTERVAL_SECONDS: u64 = 5;
const ERROR_NOT_IMPLEMENTED: i64 = 0x400;
/// Alpaca ClientID identifying LLAMA to the remote device
const CLIENT_ID: &str = "4242";

/// Result of one Alpaca call
#[derive(Debug, PartialEq)]
enum AlpacaValue {
    Value(Value),
    NotImplemented,
}

fn parse_response(body: &Value) -> Result<AlpacaValue> {
    let error = body.get("ErrorNumber").and_then(Value::as_i64).unwrap_or(0);
    match error {
        0 => Ok(AlpacaValue::Value(body.get("Value").cloned().context("Response has no Value")?)),
        ERROR_NOT_IMPLEMENTED => Ok(AlpacaValue::NotImplemented),
        code => {
            let message = body.get("ErrorMessage").and_then(Value::as_str).unwrap_or("");
            bail!("Alpaca error {:#x}: {}", code, message)
        }
    }
}

fn as_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

struct AlpacaClient {
    client: reqwest::Client,
    base: String,
}

impl AlpacaClient {
    fn new(host: &str, port: u16, device_number: u32) -> Result<Self> {
        let host = host.trim().trim_end_matches('/');
        let host = host.strip_prefix("http://").unwrap_or(host);
        Ok(Self {
            client: reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            base: format!("http://{}:{}/api/v1/observingconditions/{}", host, port, device_number),
        })
    }

    async fn get(&self, endpoint: &str, sensor: Option<&str>) -> Result<AlpacaValue> {
        let mut query = vec![("ClientID", CLIENT_ID), ("ClientTransactionID", "1")];
        if let Some(sensor) = sensor {
            query.push(("SensorName", sensor));
        }
        let body: Value = self.client.get(format!("{}/{}", self.base, endpoint))
            .query(&query)
            .send().await?
            .error_for_status()?
            .json().await?;
        parse_response(&body)
    }

    /// A property's value and how many seconds ago it was measured (if the device reports it)
    async fn read_property(&self, property: WeatherProperty) -> Result<Option<(f64, Option<f64>)>> {
        let name = property.ascom_name().context("Not an ASCOM property")?;
        let value = match self.get(&name.to_ascii_lowercase(), None).await? {
            AlpacaValue::Value(v) => as_number(&v).with_context(|| format!("{} is not a number", name))?,
            AlpacaValue::NotImplemented => return Ok(None),
        };
        let age = match self.get("timesincelastupdate", Some(name)).await {
            Ok(AlpacaValue::Value(v)) => as_number(&v).filter(|a| *a >= 0.0),
            _ => None,
        };
        Ok(Some((value, age)))
    }
}

/// Read the configured properties into an observation. Properties that error
/// are left out; if none can be read the poll fails.
async fn read_observation(client: &AlpacaClient, properties: &[WeatherProperty]) -> Result<WeatherObservation> {
    let now = Utc::now();
    let mut values = BTreeMap::new();
    let mut measured_at = BTreeMap::new();
    let mut last_error = None;

    for &property in properties {
        match client.read_property(property).await {
            Ok(Some((value, age))) => {
                values.insert(property, value);
                if let Some(age) = age {
                    measured_at.insert(property, now - chrono::Duration::milliseconds((age * 1000.0) as i64));
                }
            }
            Ok(None) => debug!("{} reports {} as not implemented", client.base, property.label()),
            Err(e) => {
                debug!("Failed to read {} from {}: {:#}", property.label(), client.base, e);
                last_error = Some(e);
            }
        }
    }

    if values.is_empty() {
        if let Some(e) = last_error {
            return Err(e);
        }
        bail!("Device returned none of the configured properties");
    }
    Ok(WeatherObservation::new(now, values).with_measured_at(measured_at))
}

/// Polls one Alpaca ObservingConditions device and writes its observations to the weather store
pub async fn poll_device(
    host: String,
    port: u16,
    device_number: u32,
    properties: Vec<WeatherProperty>,
    interval_seconds: u64,
    store: SharedWeatherStore,
) {
    let key = WeatherStore::alpaca_key(&host, port, device_number);
    let interval = Duration::from_secs(interval_seconds.max(MIN_POLL_INTERVAL_SECONDS));
    let client = match AlpacaClient::new(&host, port, device_number) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create HTTP client for Alpaca device {}: {}", key, e);
            return;
        }
    };

    info!("Polling Alpaca ObservingConditions {} every {}s", client.base, interval.as_secs());
    let mut failing = false;
    loop {
        match read_observation(&client, &properties).await {
            Ok(observation) => {
                if failing {
                    info!("Alpaca device {} is responding again", client.base);
                    failing = false;
                }
                debug!("Alpaca {}: {:?}", client.base, observation);
                store.write().await.update(key.clone(), observation);
            }
            Err(e) if !failing => {
                warn!("Failed to poll Alpaca device {}: {:#}", client.base, e);
                failing = true;
            }
            Err(e) => debug!("Alpaca device {} still failing: {:#}", client.base, e),
        }

        tokio::time::sleep(interval).await;
    }
}

/// A property found on a device by `detect`
#[derive(Debug, Serialize)]
pub struct DetectedProperty {
    pub property: WeatherProperty,
    pub ascom_name: &'static str,
    pub value: f64,
    pub age_seconds: Option<f64>,
    /// None when the device doesn't implement SensorDescription for it, which
    /// usually means the value is a placeholder rather than a real sensor
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DetectedDevice {
    pub name: Option<String>,
    pub description: Option<String>,
    pub properties: Vec<DetectedProperty>,
}

/// Find the ObservingConditions properties a device implements
pub async fn detect(host: &str, port: u16, device_number: u32) -> Result<DetectedDevice> {
    let client = AlpacaClient::new(host, port, device_number)?;
    let text = |v: AlpacaValue| match v {
        AlpacaValue::Value(Value::String(s)) => Some(s),
        _ => None,
    };

    // Fail early with a useful message if the device isn't there
    let name = text(client.get("name", None).await
        .with_context(|| format!("No ObservingConditions device at {}", client.base))?);
    let description = client.get("description", None).await.ok().and_then(text);

    let mut properties = Vec::new();
    for property in WeatherProperty::ALL {
        let Some(ascom_name) = property.ascom_name() else { continue };
        let Ok(Some((value, age_seconds))) = client.read_property(property).await else { continue };
        let description = client.get("sensordescription", Some(ascom_name)).await.ok().and_then(text);
        properties.push(DetectedProperty { property, ascom_name, value, age_seconds, description });
    }

    Ok(DetectedDevice { name, description, properties })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_values_and_errors() {
        let ok = json!({"Value": 7.7, "ClientTransactionID": 1, "ServerTransactionID": 9, "ErrorNumber": 0, "ErrorMessage": ""});
        assert_eq!(parse_response(&ok).unwrap(), AlpacaValue::Value(json!(7.7)));

        let not_implemented = json!({"Value": 0, "ErrorNumber": 1024, "ErrorMessage": "Property not implemented"});
        assert_eq!(parse_response(&not_implemented).unwrap(), AlpacaValue::NotImplemented);

        let not_connected = json!({"Value": 0, "ErrorNumber": 1031, "ErrorMessage": "Not connected"});
        assert!(parse_response(&not_connected).unwrap_err().to_string().contains("0x407"));
    }

    #[test]
    fn rejects_non_numeric_values() {
        assert_eq!(as_number(&json!(-4.3)), Some(-4.3));
        assert_eq!(as_number(&json!(100)), Some(100.0));
        assert_eq!(as_number(&json!("12")), None);
        assert_eq!(as_number(&json!(null)), None);
    }

    #[test]
    fn builds_device_url() {
        let client = AlpacaClient::new("http://skycon.local/", 11111, 0).unwrap();
        assert_eq!(client.base, "http://skycon.local:11111/api/v1/observingconditions/0");
    }
}
