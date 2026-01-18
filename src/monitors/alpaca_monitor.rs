use anyhow::{Context, Result};
use parking_lot::RwLock;
use reqwest::Client;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::{
    alpaca::models::AlpacaResponse,
    config::models::{AlpacaMonitorConfig, MonitorStatus},
};

pub struct AlpacaMonitor {
    config: AlpacaMonitorConfig,
    client: Client,
    status: Arc<RwLock<MonitorStatus>>,
}

impl AlpacaMonitor {
    pub fn new(config: AlpacaMonitorConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        // Calculate effective switch timing (fallback to safety timing if not specified)
        let switch_timeout = config.switch_timeout_seconds.unwrap_or(config.timeout_seconds);
        let switch_hold_time = config.switch_hold_time_seconds.unwrap_or(config.hold_time_seconds);

        let status = Arc::new(RwLock::new(MonitorStatus {
            id: config.id.clone(),
            name: config.name.clone(),
            monitor_type: "ASCOM Alpaca".to_string(),
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
            // New switch-related fields
            include_in_safety: config.include_in_safety,
            include_in_switch: config.include_in_switch,
            switch_name: config.switch_name.clone(),
            switch_is_safe: false,
            switch_timeout_seconds: switch_timeout,
            switch_hold_time_seconds: switch_hold_time,
            switch_pending_is_safe: None,
            switch_pending_since: None,
            switch_error: None,
        }));

        Self {
            config,
            client,
            status,
        }
    }

    pub fn get_status(&self) -> MonitorStatus {
        let mut status = self.status.read().clone();
        let now = chrono::Utc::now();

        // Calculate effective switch timing
        let switch_timeout = self.config.switch_timeout_seconds.unwrap_or(self.config.timeout_seconds);
        let switch_hold_time = self.config.switch_hold_time_seconds.unwrap_or(self.config.hold_time_seconds);

        // Update config-driven fields
        status.enabled = self.config.enabled;
        status.hold_time_seconds = self.config.hold_time_seconds;
        status.include_in_safety = self.config.include_in_safety;
        status.include_in_switch = self.config.include_in_switch;
        status.switch_name = self.config.switch_name.clone();
        status.switch_timeout_seconds = switch_timeout;
        status.switch_hold_time_seconds = switch_hold_time;

        // === SAFETY STATE PROCESSING ===
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

        // Check for safety timeout (if enabled - 0 means disabled)
        if self.config.timeout_seconds > 0 {
            if let Some(last_update) = status.last_update {
                let elapsed = now.signed_duration_since(last_update);
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

        // === SWITCH STATE PROCESSING ===
        // Check for pending switch state transitions and commit if switch hold time elapsed
        if let (Some(pending_safe), Some(pending_since)) = (status.switch_pending_is_safe, status.switch_pending_since) {
            let elapsed = now.signed_duration_since(pending_since);
            if elapsed.num_seconds() as u64 >= switch_hold_time {
                // Switch hold time has elapsed, commit the pending state
                let mut s = self.status.write();
                s.switch_is_safe = pending_safe;
                s.switch_pending_is_safe = None;
                s.switch_pending_since = None;
                status.switch_is_safe = pending_safe;
                status.switch_pending_is_safe = None;
                status.switch_pending_since = None;
            }
        }

        // Check for switch timeout (if enabled - 0 means disabled)
        if switch_timeout > 0 {
            if let Some(last_update) = status.last_update {
                let elapsed = now.signed_duration_since(last_update);
                if elapsed.num_seconds() as u64 > switch_timeout {
                    status.switch_is_safe = false;
                    status.switch_error = Some(format!(
                        "Timeout: No data received for {} seconds (timeout: {}s)",
                        elapsed.num_seconds(),
                        switch_timeout
                    ));
                }
            } else {
                // No data ever received
                status.switch_is_safe = false;
                status.switch_error = Some("No data received yet".to_string());
            }
        }

        status
    }

    pub async fn start(&self, poll_interval: Duration) -> Result<JoinHandle<()>> {
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        let handle = tokio::spawn(async move {
            loop {
                match Self::poll_endpoint(&config, &client).await {
                    Ok((value, calculated_is_safe)) => {
                        let now = chrono::Utc::now();
                        let mut s = status.write();
                        let is_initial_reading = s.last_update.is_none();

                        // Calculate effective switch timing
                        let switch_hold_time = config.switch_hold_time_seconds.unwrap_or(config.hold_time_seconds);

                        s.current_value = Some(value);
                        s.last_update = Some(now);
                        s.error = None;
                        s.switch_error = None;

                        // === SAFETY STATE LOGIC ===
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
                                                "Alpaca monitor '{}': safety state change committed after hold time - safe={}",
                                                config.name, calculated_is_safe
                                            );
                                        }
                                    }
                                } else {
                                    // New pending transition - reset timer
                                    s.pending_is_safe = Some(calculated_is_safe);
                                    s.pending_since = Some(now);
                                    info!(
                                        "Alpaca monitor '{}': pending safety state change {} -> {} (hold time: {}s)",
                                        config.name, s.is_safe, calculated_is_safe, config.hold_time_seconds
                                    );
                                }
                            }
                        }

                        // === SWITCH STATE LOGIC (independent timing) ===
                        if switch_hold_time == 0 || is_initial_reading {
                            // Immediate change (no hold time OR first reading)
                            s.switch_is_safe = calculated_is_safe;
                            s.switch_pending_is_safe = None;
                            s.switch_pending_since = None;
                        } else {
                            // Switch hold time is configured and this is not the first reading
                            if calculated_is_safe == s.switch_is_safe {
                                // Calculated state matches current switch state - clear any pending
                                s.switch_pending_is_safe = None;
                                s.switch_pending_since = None;
                            } else {
                                // Calculated state differs from current switch state
                                if s.switch_pending_is_safe == Some(calculated_is_safe) {
                                    // Already pending this transition - check if switch hold time elapsed
                                    if let Some(pending_since) = s.switch_pending_since {
                                        let elapsed = now.signed_duration_since(pending_since);
                                        if elapsed.num_seconds() as u64 >= switch_hold_time {
                                            // Switch hold time elapsed - commit the change
                                            s.switch_is_safe = calculated_is_safe;
                                            s.switch_pending_is_safe = None;
                                            s.switch_pending_since = None;
                                            info!(
                                                "Alpaca monitor '{}': switch state change committed after hold time - safe={}",
                                                config.name, calculated_is_safe
                                            );
                                        }
                                    }
                                } else {
                                    // New pending switch transition - reset timer
                                    s.switch_pending_is_safe = Some(calculated_is_safe);
                                    s.switch_pending_since = Some(now);
                                    info!(
                                        "Alpaca monitor '{}': pending switch state change {} -> {} (hold time: {}s)",
                                        config.name, s.switch_is_safe, calculated_is_safe, switch_hold_time
                                    );
                                }
                            }
                        }

                        info!(
                            "Alpaca monitor '{}': value={}, safety_safe={}, switch_safe={}, safety_pending={:?}, switch_pending={:?}",
                            config.name, value, s.is_safe, s.switch_is_safe, s.pending_is_safe, s.switch_pending_is_safe
                        );
                    }
                    Err(e) => {
                        error!("Alpaca monitor '{}' error: {}", config.name, e);
                        let mut s = status.write();
                        s.error = Some(e.to_string());
                        s.switch_error = Some(e.to_string());
                        s.is_safe = false;
                        s.switch_is_safe = false;
                    }
                }

                tokio::time::sleep(poll_interval).await;
            }
        });

        Ok(handle)
    }

    async fn poll_endpoint(config: &AlpacaMonitorConfig, client: &Client) -> Result<(f64, bool)> {
        // Build URL for the ASCOM Alpaca endpoint
        let url = format!(
            "http://{}:{}/api/v1/{}/{}/{}",
            config.host,
            config.port,
            config.device_type.to_lowercase(),
            config.device_number,
            config.property.to_lowercase()
        );

        // Make HTTP request
        let response = client
            .get(&url)
            .query(&[("ClientID", "1"), ("ClientTransactionID", "1")])
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP error: {}",
                response.status()
            ));
        }

        // Parse response
        let text = response.text().await.context("Failed to read response")?;
        let alpaca_response: AlpacaResponse<Value> = serde_json::from_str(&text)
            .context("Failed to parse ASCOM Alpaca response")?;

        // Check for ASCOM errors
        if alpaca_response.error_number != 0 {
            return Err(anyhow::anyhow!(
                "ASCOM error {}: {}",
                alpaca_response.error_number,
                alpaca_response.error_message
            ));
        }

        // Extract numeric value
        let value = Self::value_to_f64(&alpaca_response.value)?;

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

pub struct AlpacaMonitorManager {
    monitors: HashMap<String, AlpacaMonitor>,
    handles: Vec<JoinHandle<()>>,
}

impl AlpacaMonitorManager {
    pub fn new() -> Self {
        Self {
            monitors: HashMap::new(),
            handles: Vec::new(),
        }
    }

    pub async fn add_monitor(&mut self, config: AlpacaMonitorConfig) -> Result<()> {
        let monitor = AlpacaMonitor::new(config.clone());
        let handle = monitor.start(Duration::from_secs(5)).await?;

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
            // Preserve switch state
            status.switch_is_safe = old_status.switch_is_safe;
            // Clear any pending states to avoid confusion after config change
            status.pending_is_safe = None;
            status.pending_since = None;
            status.switch_pending_is_safe = None;
            status.switch_pending_since = None;
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
