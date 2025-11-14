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

        let status = Arc::new(RwLock::new(MonitorStatus {
            id: config.id.clone(),
            name: config.name.clone(),
            monitor_type: "ASCOM Alpaca".to_string(),
            is_safe: false,
            current_value: None,
            threshold: config.threshold,
            last_update: None,
            error: None,
        }));

        Self {
            config,
            client,
            status,
        }
    }

    pub fn get_status(&self) -> MonitorStatus {
        self.status.read().clone()
    }

    pub async fn start(&self, poll_interval: Duration) -> Result<JoinHandle<()>> {
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        let handle = tokio::spawn(async move {
            loop {
                match Self::poll_endpoint(&config, &client).await {
                    Ok((value, is_safe)) => {
                        let mut s = status.write();
                        s.current_value = Some(value);
                        s.is_safe = is_safe;
                        s.last_update = Some(chrono::Utc::now());
                        s.error = None;

                        info!(
                            "Alpaca monitor '{}': value={}, safe={}",
                            config.name, value, is_safe
                        );
                    }
                    Err(e) => {
                        error!("Alpaca monitor '{}' error: {}", config.name, e);
                        let mut s = status.write();
                        s.error = Some(e.to_string());
                        s.is_safe = false;
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
}
