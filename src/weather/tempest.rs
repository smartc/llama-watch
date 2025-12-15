use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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
    pub rain_accumulation: f64, // mm
    pub precipitation_type: u8, // 0=none, 1=rain, 2=hail, 3=rain+hail
    pub battery: f64,          // volts
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
        })
    }

    /// Calculate rain rate in mm/hour based on accumulation over time
    pub fn calculate_rain_rate(&self, previous_obs: Option<&TempestObservation>) -> f64 {
        if let Some(prev) = previous_obs {
            let duration_hours = (self.timestamp - prev.timestamp).num_seconds() as f64 / 3600.0;
            if duration_hours > 0.0 && duration_hours < 1.0 {
                // Only calculate if observations are within 1 hour
                let accumulation = self.rain_accumulation - prev.rain_accumulation;
                if accumulation >= 0.0 {
                    return accumulation / duration_hours;
                }
            }
        }
        0.0 // No previous observation or too far apart
    }

    /// Calculate dew point from temperature and humidity
    /// Using Magnus formula
    pub fn calculate_dew_point(&self) -> f64 {
        let a = 17.27;
        let b = 237.7;
        let alpha = ((a * self.temperature) / (b + self.temperature)) + (self.humidity / 100.0).ln();
        (b * alpha) / (a - alpha)
    }

    /// Convert illuminance (lux) to sky brightness (mag/arcsec^2)
    /// This is an approximation - sky brightness in astronomical terms
    /// Lower lux = darker sky = higher magnitude (better)
    pub fn calculate_sky_brightness(&self) -> f64 {
        if self.illuminance <= 0.0 {
            return 22.0; // Very dark sky
        }
        // Approximate conversion: mag = 22 - 2.5 * log10(lux)
        // This puts bright daylight (~100000 lux) at ~-10 mag/arcsec^2
        // and very dark night (~0.001 lux) at ~22 mag/arcsec^2
        22.0 - 2.5 * self.illuminance.log10()
    }
}

/// Shared state for Tempest observations
#[derive(Debug, Clone, Default)]
pub struct TempestData {
    pub observations: HashMap<String, TempestObservation>,
    pub previous_observations: HashMap<String, TempestObservation>, // For rain rate calculation
}

/// Tempest UDP listener
pub struct TempestListener {
    port: u16,
    data: Arc<RwLock<TempestData>>,
}

impl TempestListener {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            data: Arc::new(RwLock::new(TempestData::default())),
        }
    }

    pub fn get_data(&self) -> Arc<RwLock<TempestData>> {
        self.data.clone()
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

                            let mut data = self.data.write().await;

                            // Save previous observation for rain rate calculation
                            let prev_obs = data.observations.get(&serial_number).cloned();
                            if let Some(prev) = prev_obs {
                                data.previous_observations.insert(serial_number.clone(), prev);
                            }

                            data.observations.insert(serial_number, observation);
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
