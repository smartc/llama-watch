use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::config::models::{AppConfig, ThresholdConfig, WeatherSafetyThresholds};
use super::observation::Readings;
use super::observing_conditions::{WeatherDataAccessor, WeatherDeviceKind};

#[derive(Debug, Clone)]
pub struct WeatherMonitorStatus {
    pub is_safe: bool,
    pub last_update: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub enabled: bool,
    pub measurements: HashMap<String, MeasurementStatus>,
    // Hold time management
    pub pending_is_safe: Option<bool>,
    pub pending_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MeasurementStatus {
    pub threshold: f64,
    pub is_safe: bool,
    pub timeout_seconds: Option<i64>,
}

/// Result of checking readings against safety thresholds
#[derive(Debug)]
pub struct Evaluation {
    /// All thresholds are within their safe range
    pub all_safe: bool,
    pub measurements: HashMap<String, MeasurementStatus>,
    /// Missing or timed-out data. Makes the monitor UNSAFE immediately,
    /// bypassing the hold time.
    pub problem: Option<String>,
    pub latest_update: Option<DateTime<Utc>>,
}

/// Check readings against thresholds. A configured threshold whose property has
/// no data, or whose data is older than its timeout, counts as unsafe.
pub fn evaluate(thresholds: &WeatherSafetyThresholds, readings: &Readings, now: DateTime<Utc>) -> Evaluation {
    let latest_update = readings.values().map(|r| r.timestamp).max();
    if latest_update.is_none() {
        return Evaluation {
            all_safe: false,
            measurements: HashMap::new(),
            problem: Some("No weather data received yet".to_string()),
            latest_update,
        };
    }

    let mut measurements = HashMap::new();
    let mut all_safe = true;
    let mut problems = Vec::new();

    for (property, threshold) in thresholds.configured() {
        let Some(reading) = readings.get(&property) else {
            all_safe = false;
            problems.push(format!("No {} data", property.label()));
            continue;
        };

        // Per-measurement timeout overrides the global one
        // timeout > 0: enabled, == 0: disabled, < 0: ignored
        let timeout = threshold.timeout_seconds.unwrap_or(thresholds.timeout_seconds);
        let age = (now - reading.timestamp).num_seconds();
        let timed_out = timeout > 0 && age > timeout;
        if timed_out {
            problems.push(format!("{} data timeout ({} seconds old)", property.label(), age));
        }

        let is_safe = !timed_out && check_threshold(reading.value, threshold);
        all_safe = all_safe && is_safe;
        measurements.insert(property.label().to_string(), MeasurementStatus {
            threshold: threshold.threshold,
            is_safe,
            timeout_seconds: threshold.timeout_seconds,
        });
    }

    Evaluation {
        all_safe,
        measurements,
        problem: (!problems.is_empty()).then(|| problems.join("; ")),
        latest_update,
    }
}

/// Individual weather monitor
pub struct WeatherMonitor {
    kind: WeatherDeviceKind,
    name: String,
    thresholds: WeatherSafetyThresholds,
    weather_data: Arc<WeatherDataAccessor>,
    status: Arc<RwLock<WeatherMonitorStatus>>,
}

impl WeatherMonitor {
    pub fn new(
        kind: WeatherDeviceKind,
        name: String,
        thresholds: WeatherSafetyThresholds,
        weather_data: Arc<WeatherDataAccessor>,
        initial_is_safe: bool,
    ) -> Self {
        let status = WeatherMonitorStatus {
            is_safe: initial_is_safe,
            last_update: None,
            error: None,
            enabled: true,
            measurements: HashMap::new(),
            pending_is_safe: None,
            pending_since: None,
        };

        Self {
            kind,
            name,
            thresholds,
            weather_data,
            status: Arc::new(RwLock::new(status)),
        }
    }

    pub fn get_status(&self) -> Arc<RwLock<WeatherMonitorStatus>> {
        self.status.clone()
    }

    /// Start monitoring task
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.check_safety().await {
                    let mut status = self.status.write().await;
                    status.error = Some(format!("Monitor error: {}", e));
                    warn!("Weather monitor {} error: {}", self.name, e);
                }

                // Check every 5 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        })
    }

    async fn check_safety(&self) -> anyhow::Result<()> {
        let readings = self.weather_data.readings(self.kind).await;
        let now = Utc::now();
        let evaluation = evaluate(&self.thresholds, &readings, now);

        let mut status = self.status.write().await;
        status.measurements = evaluation.measurements;
        if evaluation.latest_update.is_some() {
            status.last_update = evaluation.latest_update;
        }

        if let Some(problem) = evaluation.problem {
            if status.is_safe {
                warn!("Weather monitor {} UNSAFE: {}", self.name, problem);
            }
            status.is_safe = false;
            status.error = Some(problem);
            status.pending_is_safe = None;
            status.pending_since = None;
            return Ok(());
        }
        status.error = None;
        let all_safe = evaluation.all_safe;

        // Handle hold time state machine
        if self.thresholds.hold_time_seconds == 0 {
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
                        if elapsed >= self.thresholds.hold_time_seconds {
                            // Hold time elapsed, commit the change
                            info!("Weather monitor {} state changing to {} after {} second hold",
                                self.name, if all_safe { "SAFE" } else { "UNSAFE" }, elapsed);
                            status.is_safe = all_safe;
                            status.pending_is_safe = None;
                            status.pending_since = None;
                        } else {
                            debug!("Weather monitor {} pending {} ({}/{} seconds)",
                                self.name, if all_safe { "SAFE" } else { "UNSAFE" },
                                elapsed, self.thresholds.hold_time_seconds);
                        }
                    }
                } else {
                    // New pending state
                    info!("Weather monitor {} wants to change to {}, starting hold timer",
                        self.name, if all_safe { "SAFE" } else { "UNSAFE" });
                    status.pending_is_safe = Some(all_safe);
                    status.pending_since = Some(now);
                }
            } else {
                // State matches current, clear pending
                if status.pending_is_safe.is_some() {
                    debug!("Weather monitor {} returned to current state, clearing pending",
                        self.name);
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

    /// Replace all monitors with those the configuration asks for. A monitor
    /// that existed before keeps its committed safe/unsafe state; new monitors
    /// start UNSAFE until their data has been SAFE for the hold time.
    pub async fn rebuild(&mut self, config: &AppConfig, weather_data: Arc<WeatherDataAccessor>) {
        let previous: HashMap<u32, bool> = self.get_all_statuses().await.into_iter()
            .map(|(n, status)| (n, status.is_safe))
            .collect();
        self.shutdown().await;

        for (kind, name, thresholds) in monitored_devices(config) {
            let number = kind.device_number();
            let initial_is_safe = previous.get(&number).copied().unwrap_or(false);
            let monitor = Arc::new(WeatherMonitor::new(
                kind, name.clone(), thresholds, weather_data.clone(), initial_is_safe,
            ));
            self.tasks.insert(number, monitor.clone().start());
            self.monitors.insert(number, monitor);
            info!("Started weather safety monitor for device {}: {}", number, name);
        }
    }

    pub async fn shutdown(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
        self.monitors.clear();
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
}

/// Devices with safety thresholds enabled: the aggregate (when it has an
/// enabled source) and any enabled source with its own thresholds
fn monitored_devices(config: &AppConfig) -> Vec<(WeatherDeviceKind, String, WeatherSafetyThresholds)> {
    let mut monitored = Vec::new();
    let any_source_enabled = config.weather_devices.values().any(|d| d.enabled);

    if let Some(aggregate) = &config.weather_aggregate {
        if let Some(thresholds) = aggregate.safety_thresholds.as_ref().filter(|t| t.enabled) {
            if any_source_enabled {
                monitored.push((WeatherDeviceKind::Aggregate, aggregate.name.clone(), thresholds.clone()));
            }
        }
    }

    for (number, device) in &config.weather_devices {
        if let Some(thresholds) = device.safety_thresholds.as_ref().filter(|t| t.enabled && device.enabled) {
            monitored.push((WeatherDeviceKind::Source(*number), device.name.clone(), thresholds.clone()));
        }
    }
    monitored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::ComparisonOperator;
    use crate::weather::observation::{Reading, WeatherProperty};
    use chrono::Duration;

    fn rain_threshold(timeout: Option<i64>) -> WeatherSafetyThresholds {
        WeatherSafetyThresholds {
            enabled: true,
            timeout_seconds: 300,
            hold_time_seconds: 60,
            rain_rate: Some(ThresholdConfig {
                threshold: 0.0,
                operator: ComparisonOperator::GreaterThan,
                safe_when_true: false,
                timeout_seconds: timeout,
            }),
            ..Default::default()
        }
    }

    fn readings(property: WeatherProperty, value: f64, age: i64, now: DateTime<Utc>) -> Readings {
        Readings::from([(property, Reading { value, timestamp: now - Duration::seconds(age), source_device: 1 })])
    }

    #[test]
    fn evaluates_thresholds() {
        let now = Utc::now();
        let dry = evaluate(&rain_threshold(None), &readings(WeatherProperty::RainRate, 0.0, 5, now), now);
        assert!(dry.all_safe && dry.problem.is_none());

        let wet = evaluate(&rain_threshold(None), &readings(WeatherProperty::RainRate, 1.2, 5, now), now);
        assert!(!wet.all_safe && wet.problem.is_none());
    }

    #[test]
    fn missing_threshold_property_is_unsafe() {
        let now = Utc::now();
        let eval = evaluate(&rain_threshold(None), &readings(WeatherProperty::WindSpeed, 1.0, 5, now), now);
        assert!(!eval.all_safe);
        assert_eq!(eval.problem.as_deref(), Some("No Rain Rate data"));
    }

    #[test]
    fn no_data_at_all_is_unsafe() {
        let eval = evaluate(&rain_threshold(None), &Readings::new(), Utc::now());
        assert!(!eval.all_safe);
        assert!(eval.problem.is_some());
    }

    #[test]
    fn per_measurement_timeout_overrides_global() {
        let now = Utc::now();
        let data = readings(WeatherProperty::RainRate, 0.0, 200, now);
        assert!(evaluate(&rain_threshold(None), &data, now).problem.is_none());

        let eval = evaluate(&rain_threshold(Some(120)), &data, now);
        assert!(!eval.all_safe);
        assert!(eval.problem.unwrap().contains("timeout"));

        // Negative timeout ignores data age
        let old = readings(WeatherProperty::RainRate, 0.0, 10_000, now);
        assert!(evaluate(&rain_threshold(Some(-1)), &old, now).all_safe);
    }
}
