use anyhow::{Context, Result};
use jsonpath_rust::JsonPathFinder;
use parking_lot::RwLock;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::models::{MqttMonitorConfig, MqttServerConfig, MonitorStatus};

pub struct MqttMonitor {
    config: MqttMonitorConfig,
    server_config: MqttServerConfig,
    status: Arc<RwLock<MonitorStatus>>,
}

impl MqttMonitor {
    pub fn new(config: MqttMonitorConfig, server_config: MqttServerConfig) -> Self {
        let status = Arc::new(RwLock::new(MonitorStatus {
            id: config.id.clone(),
            name: config.name.clone(),
            monitor_type: "MQTT".to_string(),
            is_safe: false,
            current_value: None,
            threshold: config.threshold,
            last_update: None,
            error: None,
            raw_payload: None,
            enabled: config.enabled,
            hold_time_seconds: config.hold_time_seconds,
            pending_is_safe: None,
            pending_since: None,
            include_in_safety: config.include_in_safety,
        }));

        Self {
            config,
            server_config,
            status,
        }
    }

    pub fn get_status(&self) -> MonitorStatus {
        let mut status = self.status.read().clone();
        let now = chrono::Utc::now();

        // Update config-driven fields
        status.enabled = self.config.enabled;
        status.hold_time_seconds = self.config.hold_time_seconds;
        status.include_in_safety = self.config.include_in_safety;

        // Check for pending safety state transitions and commit if hold time elapsed
        if let (Some(pending_safe), Some(pending_since)) = (status.pending_is_safe, status.pending_since) {
            let elapsed = now.signed_duration_since(pending_since);
            if elapsed.num_seconds() as u64 >= self.config.hold_time_seconds {
                // Hold time has elapsed, commit the pending state
                let mut s = self.status.write();
                s.is_safe = pending_safe;
                s.pending_is_safe = None;
                s.pending_since = None;
                status.is_safe = pending_safe;
                status.pending_is_safe = None;
                status.pending_since = None;
            }
        }

        // Check for safety timeout (if enabled)
        // timeout_seconds > 0: timeout enabled
        // timeout_seconds == 0: timeout disabled
        // timeout_seconds < 0: timeout ignored completely (no "no data yet" error either)
        if self.config.timeout_seconds > 0 {
            if let Some(last_update) = status.last_update {
                let elapsed = now.signed_duration_since(last_update);
                if elapsed.num_seconds() > self.config.timeout_seconds {
                    status.is_safe = false;
                    status.error = Some(format!(
                        "Timeout: No data received for {} seconds (timeout: {}s)",
                        elapsed.num_seconds(),
                        self.config.timeout_seconds
                    ));
                }
            } else {
                // No data ever received
                status.is_safe = false;
                status.error = Some("No data received yet".to_string());
            }
        } else if self.config.timeout_seconds == 0 && status.last_update.is_none() {
            // timeout_seconds == 0 but no data yet - still report waiting for data
            // (but don't mark as unsafe)
            status.error = Some("Waiting for initial data".to_string());
        }
        // If timeout_seconds < 0, we skip all timeout/waiting checks

        status
    }

    pub async fn start(&self) -> Result<JoinHandle<()>> {
        let config = self.config.clone();
        let server_config = self.server_config.clone();
        let status = self.status.clone();

        let handle = tokio::spawn(async move {
            loop {
                if let Err(e) = Self::run_monitor(
                    config.clone(),
                    server_config.clone(),
                    status.clone(),
                )
                .await
                {
                    error!("MQTT monitor '{}' error: {}", config.name, e);
                    {
                        let mut s = status.write();
                        s.error = Some(e.to_string());
                        s.is_safe = false;
                    }
                }

                // Wait before reconnecting
                tokio::time::sleep(Duration::from_secs(5)).await;
                info!("Reconnecting MQTT monitor '{}'...", config.name);
            }
        });

        Ok(handle)
    }

    async fn run_monitor(
        config: MqttMonitorConfig,
        server_config: MqttServerConfig,
        status: Arc<RwLock<MonitorStatus>>,
    ) -> Result<()> {
        // Configure MQTT options
        let mut mqtt_options = MqttOptions::new(
            format!("llama-watch-{}", config.id),
            &server_config.host,
            server_config.port,
        );

        mqtt_options.set_keep_alive(Duration::from_secs(30));

        if let (Some(username), Some(password)) =
            (&server_config.username, &server_config.password)
        {
            mqtt_options.set_credentials(username, password);
        }

        // Create client with larger queue to handle bursts of messages
        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 100);

        // Subscribe to topic with QoS 1 for reliable delivery
        client
            .subscribe(&config.topic, QoS::AtLeastOnce)
            .await
            .context("Failed to subscribe to topic")?;

        info!(
            "MQTT monitor '{}' subscribed to topic '{}' on {}:{}",
            config.name, config.topic, server_config.host, server_config.port
        );

        // Process events
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    if let Event::Incoming(Packet::Publish(publish)) = event {
                        let payload = String::from_utf8_lossy(&publish.payload).to_string();

                        match Self::process_message(&config, &payload) {
                            Ok((value, calculated_is_safe)) => {
                                let now = chrono::Utc::now();
                                let mut s = status.write();
                                let is_initial_reading = s.last_update.is_none();

                                s.current_value = Some(value);
                                s.last_update = Some(now);
                                s.error = None;
                                s.raw_payload = Some(payload.clone());

                                // Safety state logic with hold time
                                if config.hold_time_seconds == 0 || is_initial_reading {
                                    // Immediate change (no hold time OR first reading)
                                    s.is_safe = calculated_is_safe;
                                    s.pending_is_safe = None;
                                    s.pending_since = None;
                                } else {
                                    // Hold time is configured and this is not the first reading
                                    if calculated_is_safe == s.is_safe {
                                        // Calculated state matches current state - clear any pending
                                        s.pending_is_safe = None;
                                        s.pending_since = None;
                                    } else {
                                        // Calculated state differs from current state
                                        if s.pending_is_safe == Some(calculated_is_safe) {
                                            // Already pending this transition - check if hold time elapsed
                                            if let Some(pending_since) = s.pending_since {
                                                let elapsed = now.signed_duration_since(pending_since);
                                                if elapsed.num_seconds() as u64 >= config.hold_time_seconds {
                                                    // Hold time elapsed - commit the change
                                                    s.is_safe = calculated_is_safe;
                                                    s.pending_is_safe = None;
                                                    s.pending_since = None;
                                                    info!(
                                                        "MQTT monitor '{}': state change committed after hold time - safe={}",
                                                        config.name, calculated_is_safe
                                                    );
                                                }
                                            }
                                        } else {
                                            // New pending transition - reset timer
                                            s.pending_is_safe = Some(calculated_is_safe);
                                            s.pending_since = Some(now);
                                            info!(
                                                "MQTT monitor '{}': pending state change {} -> {} (hold time: {}s)",
                                                config.name, s.is_safe, calculated_is_safe, config.hold_time_seconds
                                            );
                                        }
                                    }
                                }

                                info!(
                                    "MQTT monitor '{}': value={}, safe={}, pending={:?}",
                                    config.name, value, s.is_safe, s.pending_is_safe
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "MQTT monitor '{}' failed to process message: {}",
                                    config.name, e
                                );
                                let mut s = status.write();
                                s.error = Some(e.to_string());
                                s.is_safe = false;
                                s.last_update = Some(chrono::Utc::now());
                                s.raw_payload = Some(payload.clone());
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("MQTT connection error: {}", e));
                }
            }
        }
    }

    fn process_message(config: &MqttMonitorConfig, payload: &str) -> Result<(f64, bool)> {
        let value = match &config.json_path {
            None => {
                // No JSON path - treat payload as raw numeric value
                payload.trim().parse::<f64>()
                    .context("Failed to parse raw payload as number")?
            }
            Some(path) if path.is_empty() => {
                // Empty path - treat as raw numeric value
                payload.trim().parse::<f64>()
                    .context("Failed to parse raw payload as number")?
            }
            Some(path) => {
                // Parse JSON and extract value using path
                let json: Value = serde_json::from_str(payload)
                    .context("Failed to parse JSON payload")?;
                Self::extract_value(&json, path)?
            }
        };

        // Compare with threshold
        let comparison_result = config.operator.compare(value, config.threshold);

        // Determine safety based on safe_when_true flag
        let is_safe = if config.safe_when_true {
            comparison_result
        } else {
            !comparison_result
        };

        Ok((value, is_safe))
    }

    fn extract_value(json: &Value, path: &str) -> Result<f64> {
        // First, try direct key access (handles keys with dots like "Volts.1")
        if let Value::Object(map) = json {
            if let Some(value) = map.get(path) {
                return Self::value_to_f64(value);
            }
        }

        // If direct access fails, try JSON path query
        // Normalize path - add $ prefix if not present
        let json_path = if path.starts_with('$') {
            path.to_string()
        } else {
            format!("$.{}", path)
        };

        // Use jsonpath-rust to extract value
        let finder = JsonPathFinder::from_str(json.to_string().as_str(), &json_path)
            .map_err(|e| anyhow::anyhow!("Failed to parse JSON path: {}", e))?;

        let result = finder.find();

        // Extract the value
        match result {
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Err(anyhow::anyhow!("JSON path '{}' returned no results", path));
                }
                Self::value_to_f64(&arr[0])
            }
            _ => Self::value_to_f64(&result),
        }
    }

    fn value_to_f64(value: &Value) -> Result<f64> {
        match value {
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("Failed to convert number to f64")),
            Value::String(s) => s
                .parse::<f64>()
                .context("Failed to parse string as f64"),
            _ => Err(anyhow::anyhow!("Value is not convertible to number: {:?}", value)),
        }
    }
}

pub struct MqttMonitorManager {
    monitors: HashMap<String, MqttMonitor>,
    handles: Vec<JoinHandle<()>>,
}

impl MqttMonitorManager {
    pub fn new() -> Self {
        Self {
            monitors: HashMap::new(),
            handles: Vec::new(),
        }
    }

    pub async fn add_monitor(
        &mut self,
        config: MqttMonitorConfig,
        server_config: MqttServerConfig,
    ) -> Result<()> {
        let monitor = MqttMonitor::new(config.clone(), server_config);
        let handle = monitor.start().await?;

        self.monitors.insert(config.id.clone(), monitor);
        self.handles.push(handle);

        Ok(())
    }

    pub fn get_statuses(&self) -> Vec<MonitorStatus> {
        self.monitors
            .values()
            .map(|m| m.get_status())
            .collect()
    }

    pub fn get_status(&self, id: &str) -> Option<MonitorStatus> {
        self.monitors.get(id).map(|m| m.get_status())
    }

    pub fn restore_state(&self, id: &str, old_status: &MonitorStatus) -> Option<()> {
        if let Some(monitor) = self.monitors.get(id) {
            let mut status = monitor.status.write();
            // Preserve the safety state and current value
            status.is_safe = old_status.is_safe;
            status.current_value = old_status.current_value;
            status.last_update = old_status.last_update;
            // Clear any pending states to avoid confusion after config change
            status.pending_is_safe = None;
            status.pending_since = None;
            Some(())
        } else {
            None
        }
    }

    pub async fn shutdown(&mut self) {
        // Abort all running monitor tasks
        for handle in self.handles.drain(..) {
            handle.abort();
        }
        self.monitors.clear();
    }
}
