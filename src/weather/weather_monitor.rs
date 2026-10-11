use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::config::models::{ThresholdConfig, WeatherDeviceConfig};
use super::observing_conditions::WeatherDataAccessor;

#[derive(Debug, Clone)]
pub struct WeatherMonitorStatus {
    pub device_number: u32,
    pub name: String,
    pub is_safe: bool,
    pub last_update: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub enabled: bool,
    pub measurements: HashMap<String, MeasurementStatus>,
    // Hold time management
    pub hold_time_seconds: u64,
    pub pending_is_safe: Option<bool>,
    pub pending_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MeasurementStatus {
    pub name: String,
    pub value: f64,
    pub threshold: f64,
    pub is_safe: bool,
    pub timeout_seconds: Option<i64>,
    pub last_update: Option<DateTime<Utc>>,
}

/// Individual weather monitor
pub struct WeatherMonitor {
    config: WeatherDeviceConfig,
    weather_data: Arc<WeatherDataAccessor>,
    status: Arc<RwLock<WeatherMonitorStatus>>,
}

impl WeatherMonitor {
    pub fn new(config: WeatherDeviceConfig, weather_data: Arc<WeatherDataAccessor>) -> Self {
        let status = WeatherMonitorStatus {
            device_number: config.device_number,
            name: config.name.clone(),
            is_safe: true, // Start optimistic
            last_update: None,
            error: None,
            enabled: config.enabled,
            measurements: HashMap::new(),
            hold_time_seconds: config.safety_thresholds.as_ref()
                .map(|t| t.hold_time_seconds)
                .unwrap_or(0),
            pending_is_safe: None,
            pending_since: None,
        };

        Self {
            config,
            weather_data,
            status: Arc::new(RwLock::new(status)),
        }
    }

    pub fn get_status(&self) -> Arc<RwLock<WeatherMonitorStatus>> {
        self.status.clone()
    }

    /// Start monitoring task
    pub async fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.check_safety().await {
                    let mut status = self.status.write().await;
                    status.error = Some(format!("Monitor error: {}", e));
                    warn!("Weather monitor {} error: {}", self.config.name, e);
                }

                // Check every 5 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        })
    }

    async fn check_safety(&self) -> anyhow::Result<()> {
        // Skip if safety thresholds not configured or disabled
        let thresholds = match &self.config.safety_thresholds {
            Some(t) if t.enabled => t,
            _ => return Ok(()), // No thresholds configured
        };

        let readings = self.weather_data.readings(&self.config).await;
        let latest_update = match readings.values().map(|r| r.timestamp).max() {
            Some(ts) => ts,
            None => {
                // No data yet
                let mut status = self.status.write().await;
                status.error = Some("No weather data received yet".to_string());
                return Ok(());
            }
        };

        // Check timeout (if enabled)
        // timeout_seconds > 0: timeout enabled
        // timeout_seconds == 0: timeout disabled
        // timeout_seconds < 0: timeout ignored completely
        let now = Utc::now();
        let time_since_update = (now - latest_update).num_seconds();
        if thresholds.timeout_seconds > 0 && time_since_update > thresholds.timeout_seconds {
            let mut status = self.status.write().await;
            status.is_safe = false;
            status.error = Some(format!("Weather data timeout ({} seconds old)", time_since_update));
            status.pending_is_safe = None;
            status.pending_since = None;
            return Ok(());
        }
        // If timeout_seconds <= 0, we skip the timeout check

        // Check all configured thresholds
        let mut measurements = HashMap::new();
        let mut all_safe = true;

        for (property, threshold) in thresholds.configured() {
            if let Some(reading) = readings.get(&property) {
                let is_safe = check_threshold(reading.value, threshold);
                all_safe = all_safe && is_safe;
                measurements.insert(property.label().to_string(), MeasurementStatus {
                    name: property.label().to_string(),
                    value: reading.value,
                    threshold: threshold.threshold,
                    is_safe,
                    timeout_seconds: threshold.timeout_seconds,
                    last_update: Some(reading.timestamp),
                });
            }
        }

        // Update status with hold time logic
        let mut status = self.status.write().await;
        status.last_update = Some(latest_update);
        status.error = None;
        status.measurements = measurements;

        // Handle hold time state machine
        if thresholds.hold_time_seconds == 0 {
            // No hold time, immediate state change
            status.is_safe = all_safe;
            status.pending_is_safe = None;
            status.pending_since = None;
        } else {
            // Check if state is changing
            if all_safe != status.is_safe {
                // State wants to change
                if status.pending_is_safe == Some(all_safe) {
                    // Already pending this state
                    if let Some(pending_since) = status.pending_since {
                        let elapsed = (now - pending_since).num_seconds() as u64;
                        if elapsed >= thresholds.hold_time_seconds {
                            // Hold time elapsed, commit the change
                            info!("Weather monitor {} state changing to {} after {} second hold",
                                self.config.name, if all_safe { "SAFE" } else { "UNSAFE" }, elapsed);
                            status.is_safe = all_safe;
                            status.pending_is_safe = None;
                            status.pending_since = None;
                        } else {
                            debug!("Weather monitor {} pending {} ({}/{} seconds)",
                                self.config.name, if all_safe { "SAFE" } else { "UNSAFE" },
                                elapsed, thresholds.hold_time_seconds);
                        }
                    }
                } else {
                    // New pending state
                    info!("Weather monitor {} wants to change to {}, starting hold timer",
                        self.config.name, if all_safe { "SAFE" } else { "UNSAFE" });
                    status.pending_is_safe = Some(all_safe);
                    status.pending_since = Some(now);
                }
            } else {
                // State matches current, clear pending
                if status.pending_is_safe.is_some() {
                    debug!("Weather monitor {} returned to current state, clearing pending",
                        self.config.name);
                    status.pending_is_safe = None;
                    status.pending_since = None;
                }
            }
        }

        Ok(())
    }
}

fn check_threshold(value: f64, threshold: &ThresholdConfig) -> bool {
    let comparison_result = threshold.operator.compare(value, threshold.threshold);
    if threshold.safe_when_true {
        comparison_result
    } else {
        !comparison_result
    }
}

/// Manager for all weather monitors
pub struct WeatherMonitorManager {
    monitors: HashMap<u32, Arc<WeatherMonitor>>,
    tasks: HashMap<u32, JoinHandle<()>>,
}

impl WeatherMonitorManager {
    pub fn new() -> Self {
        Self {
            monitors: HashMap::new(),
            tasks: HashMap::new(),
        }
    }

    pub async fn initialize(
        &mut self,
        configs: &HashMap<u32, WeatherDeviceConfig>,
        weather_data: Arc<WeatherDataAccessor>,
    ) {
        // Create monitors for devices with safety thresholds enabled
        for (device_number, config) in configs {
            if config.enabled {
                if let Some(thresholds) = &config.safety_thresholds {
                    if thresholds.enabled {
                        let monitor = Arc::new(WeatherMonitor::new(config.clone(), weather_data.clone()));
                        let handle = monitor.clone().start().await;
                        self.monitors.insert(*device_number, monitor);
                        self.tasks.insert(*device_number, handle);
                        info!("Started weather safety monitor for device {}: {}", device_number, config.name);
                    }
                }
            }
        }
    }

    pub async fn shutdown(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
        self.monitors.clear();
    }

    pub async fn remove_monitor(&mut self, device_number: u32) {
        if let Some(task) = self.tasks.remove(&device_number) {
            task.abort();
        }
        self.monitors.remove(&device_number);
        info!("Removed weather safety monitor for device {}", device_number);
    }

    pub fn get_monitor(&self, device_number: u32) -> Option<&Arc<WeatherMonitor>> {
        self.monitors.get(&device_number)
    }

    pub async fn get_all_statuses(&self) -> HashMap<u32, WeatherMonitorStatus> {
        let mut statuses = HashMap::new();
        for (device_number, monitor) in &self.monitors {
            let status = monitor.get_status().read().await.clone();
            statuses.insert(*device_number, status);
        }
        statuses
    }

    /// Check if all enabled weather monitors are safe
    pub async fn are_all_safe(&self) -> bool {
        for monitor in self.monitors.values() {
            let status_lock = monitor.get_status();
            let status = status_lock.read().await;
            if status.enabled && !status.is_safe {
                return false;
            }
        }
        true
    }

    /// Get overall safety status across all weather monitors
    pub async fn get_overall_status(&self) -> Option<(bool, Vec<String>)> {
        if self.monitors.is_empty() {
            return None;
        }

        let mut all_safe = true;
        let mut messages = Vec::new();

        for monitor in self.monitors.values() {
            let status_lock = monitor.get_status();
            let status = status_lock.read().await;
            if !status.enabled {
                continue;
            }

            if !status.is_safe {
                all_safe = false;
                if let Some(error) = &status.error {
                    messages.push(format!("{}: {}", status.name, error));
                } else {
                    // Report which measurements are unsafe
                    for measurement in status.measurements.values() {
                        if !measurement.is_safe {
                            messages.push(format!("{} - {}: {:.1} (threshold: {:.1})",
                                status.name, measurement.name, measurement.value, measurement.threshold));
                        }
                    }
                }
            }
        }

        Some((all_safe, messages))
    }
}
