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
        }));

        Self {
            config,
            server_config,
            status,
        }
    }

    pub fn get_status(&self) -> MonitorStatus {
        let mut status = self.status.read().clone();

        // Update enabled state from config
        status.enabled = self.config.enabled;

        // Check for timeout (if enabled - 0 means disabled)
        if self.config.timeout_seconds > 0 {
            if let Some(last_update) = status.last_update {
                let elapsed = chrono::Utc::now().signed_duration_since(last_update);
                if elapsed.num_seconds() as u64 > self.config.timeout_seconds {
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
        }

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

        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);

        // Subscribe to topic
        client
            .subscribe(&config.topic, QoS::AtMostOnce)
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
                            Ok((value, is_safe)) => {
                                let mut s = status.write();
                                s.current_value = Some(value);
                                s.is_safe = is_safe;
                                s.last_update = Some(chrono::Utc::now());
                                s.error = None;
                                s.raw_payload = Some(payload.clone());

                                info!(
                                    "MQTT monitor '{}': value={}, safe={}",
                                    config.name, value, is_safe
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

    pub async fn shutdown(&mut self) {
        // Abort all running monitor tasks
        for handle in self.handles.drain(..) {
            handle.abort();
        }
        self.monitors.clear();
    }
}
