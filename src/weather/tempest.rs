use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use super::observation::{SharedWeatherStore, WeatherObservation, WeatherProperty, WeatherStore};

/// Tempest observation data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempestObservation {
    pub serial_number: String,
    pub timestamp: DateTime<Utc>,
    pub wind_lull: f64,        // m/s
    pub wind_avg: f64,         // m/s
    pub wind_gust: f64,        // m/s
    pub wind_direction: f64,   // degrees
    pub pressure: f64,         // MB
    pub temperature: f64,      // C
    pub humidity: f64,         // %
    pub illuminance: f64,      // lux
    pub uv: f64,               // index
    pub solar_radiation: f64,  // W/m^2
    pub rain_accumulation: f64, // mm fallen during the report interval (not a running total)
    pub precipitation_type: u8, // 0=none, 1=rain, 2=hail, 3=rain+hail
    pub battery: f64,          // volts
    pub report_interval_minutes: f64,
}

/// UDP message types from Tempest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TempestMessage {
    #[serde(rename = "obs_st")]
    Observation {
        serial_number: String,
        hub_sn: String,
        obs: Vec<Vec<serde_json::Value>>,
        firmware_revision: Option<u32>,
    },
    #[serde(rename = "rapid_wind")]
    RapidWind {
        serial_number: String,
        hub_sn: String,
        ob: Vec<serde_json::Value>,
    },
    #[serde(rename = "evt_precip")]
    RainStart {
        serial_number: String,
        hub_sn: String,
        evt: Vec<serde_json::Value>,
    },
    #[serde(rename = "evt_strike")]
    LightningStrike {
        serial_number: String,
        hub_sn: String,
        evt: Vec<serde_json::Value>,
    },
    #[serde(rename = "device_status")]
    DeviceStatus {
        serial_number: String,
        hub_sn: String,
        timestamp: u64,
    },
}

impl TempestObservation {
    /// Parse observation from Tempest obs array (22 fields as per UDP spec v143+)
    pub fn from_obs_array(serial_number: String, obs: &[serde_json::Value]) -> Result<Self> {
        if obs.len() < 18 {
            anyhow::bail!("Observation array too short: {} fields", obs.len());
        }

        let timestamp = obs[0]
            .as_i64()
            .context("Invalid timestamp")?;
        let timestamp = DateTime::from_timestamp(timestamp, 0)
            .context("Invalid timestamp value")?;

        Ok(TempestObservation {
            serial_number,
            timestamp,
            wind_lull: obs[1].as_f64().unwrap_or(0.0),
            wind_avg: obs[2].as_f64().unwrap_or(0.0),
            wind_gust: obs[3].as_f64().unwrap_or(0.0),
            wind_direction: obs[4].as_f64().unwrap_or(0.0),
            pressure: obs[6].as_f64().unwrap_or(0.0),
            temperature: obs[7].as_f64().unwrap_or(0.0),
            humidity: obs[8].as_f64().unwrap_or(0.0),
            illuminance: obs[9].as_f64().unwrap_or(0.0),
            uv: obs[10].as_f64().unwrap_or(0.0),
            solar_radiation: obs[11].as_f64().unwrap_or(0.0),
            rain_accumulation: obs[12].as_f64().unwrap_or(0.0),
            precipitation_type: obs[13].as_u64().unwrap_or(0) as u8,
            battery: obs[16].as_f64().unwrap_or(0.0),
            report_interval_minutes: obs[17].as_f64().filter(|m| *m > 0.0).unwrap_or(1.0),
        })
    }

    /// Rain rate in mm/hour. obs[12] is the rain that fell during the report
    /// interval (normally one minute), so scale it up to an hourly rate.
    pub fn rain_rate(&self) -> f64 {
        self.rain_accumulation.max(0.0) * 60.0 / self.report_interval_minutes
    }

    /// Convert to the source-independent observation format
    pub fn to_observation(&self) -> WeatherObservation {
        let values = BTreeMap::from([
            (WeatherProperty::Temperature, self.temperature),
            (WeatherProperty::Humidity, self.humidity),
            (WeatherProperty::Pressure, self.pressure),
            (WeatherProperty::WindSpeed, self.wind_avg),
            (WeatherProperty::WindGust, self.wind_gust),
            (WeatherProperty::WindDirection, self.wind_direction),
            (WeatherProperty::RainRate, self.rain_rate()),
            (WeatherProperty::SkyBrightness, self.illuminance),
            (WeatherProperty::SolarRadiation, self.solar_radiation),
            (WeatherProperty::UvIndex, self.uv),
        ]);
        WeatherObservation::new(self.timestamp, values)
    }
}

/// Tempest UDP listener
pub struct TempestListener {
    port: u16,
    store: SharedWeatherStore,
}

impl TempestListener {
    pub fn new(port: u16, store: SharedWeatherStore) -> Self {
        Self { port, store }
    }

    /// Start listening for UDP broadcasts
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", self.port).parse()?;
        let socket = UdpSocket::bind(bind_addr).await
            .context(format!("Failed to bind UDP socket to {}", bind_addr))?;

        info!("Tempest UDP listener started on port {}", self.port);

        let mut buf = vec![0u8; 2048]; // Tempest messages are typically < 1KB

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    let data = &buf[..len];
                    if let Err(e) = self.process_message(data, addr).await {
                        debug!("Failed to process Tempest message from {}: {}", addr, e);
                    }
                }
                Err(e) => {
                    error!("UDP receive error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn process_message(&self, data: &[u8], _addr: SocketAddr) -> Result<()> {
        let msg: TempestMessage = serde_json::from_slice(data)
            .context("Failed to parse JSON")?;

        match msg {
            TempestMessage::Observation { serial_number, obs, .. } => {
                if let Some(obs_data) = obs.first() {
                    match TempestObservation::from_obs_array(serial_number.clone(), obs_data) {
                        Ok(observation) => {
                            debug!("Received Tempest observation from {}: temp={:.1}°C, humidity={:.0}%, wind={:.1}m/s",
                                serial_number, observation.temperature, observation.humidity, observation.wind_avg);

                            self.store.write().await.update(
                                WeatherStore::tempest_key(&serial_number),
                                observation.to_observation(),
                            );
                        }
                        Err(e) => {
                            warn!("Failed to parse Tempest observation: {}", e);
                        }
                    }
                }
            }
            TempestMessage::RapidWind { serial_number, hub_sn: _, ob } => {
                // Rapid wind updates - could be used for faster wind monitoring
                debug!("Received rapid wind from {}: {:?}", serial_number, ob);
            }
            TempestMessage::RainStart { serial_number, .. } => {
                info!("Rain started at {}", serial_number);
            }
            TempestMessage::LightningStrike { serial_number, .. } => {
                info!("Lightning detected at {}", serial_number);
            }
            TempestMessage::DeviceStatus { .. } => {
                // Device status - could be used for health monitoring
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obs_array(rain_mm: f64, interval_min: u64) -> Vec<serde_json::Value> {
        vec![
            json!(1700000000), json!(0.1), json!(1.2), json!(2.3), json!(180), json!(3),
            json!(1013.2), json!(12.5), json!(80), json!(0), json!(0.0), json!(0),
            json!(rain_mm), json!(1), json!(0), json!(0), json!(2.6), json!(interval_min),
        ]
    }

    #[test]
    fn rain_rate_scales_per_interval_amount_to_hourly() {
        let obs = TempestObservation::from_obs_array("ST-1".into(), &obs_array(0.1, 1)).unwrap();
        assert!((obs.rain_rate() - 6.0).abs() < 1e-9);

        let obs = TempestObservation::from_obs_array("ST-1".into(), &obs_array(0.5, 5)).unwrap();
        assert!((obs.rain_rate() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn steady_rain_is_not_reported_as_dry() {
        // Two identical one-minute amounts used to produce a rate of zero
        let a = TempestObservation::from_obs_array("ST-1".into(), &obs_array(0.2, 1)).unwrap();
        let b = TempestObservation::from_obs_array("ST-1".into(), &obs_array(0.2, 1)).unwrap();
        assert!(a.rain_rate() > 0.0 && b.rain_rate() > 0.0);
    }
}
