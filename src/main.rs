mod alpaca;
mod config;
mod monitors;
mod web;

use anyhow::Result;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber;

use alpaca::safety_monitor::{AppState, SafetyMonitor};
use config::load_config;
use monitors::MonitorState;
use web::handlers::WebState;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llama_watch=info,tower_http=debug".into()),
        )
        .init();

    info!("🦙 LLAMA Safety Monitor starting...");

    // Load configuration
    let config = load_config().await?;
    info!(
        "Loaded configuration: {} MQTT servers, {} MQTT monitors, {} Alpaca monitors",
        config.mqtt_servers.len(),
        config.mqtt_monitors.len(),
        config.alpaca_monitors.len()
    );

    let server_port = if config.server_port > 0 {
        config.server_port
    } else {
        8080
    };

    let device_name = if config.device_name.is_empty() {
        "LLAMA Safety Monitor".to_string()
    } else {
        config.device_name.clone()
    };

    // Initialize monitor state
    let mut monitor_state = MonitorState::new();
    if let Err(e) = monitor_state.initialize(&config).await {
        error!("Failed to initialize monitors: {}", e);
        return Err(e);
    }
    info!("Monitors initialized successfully");

    let shared_monitor_state = Arc::new(RwLock::new(monitor_state));

    // Create safety monitor
    let safety_monitor = Arc::new(SafetyMonitor::new(device_name));

    // Create application state for ASCOM Alpaca endpoints
    let alpaca_state = Arc::new(AppState {
        safety_monitor: safety_monitor.clone(),
        monitor_state: shared_monitor_state.clone(),
    });

    // Create web state for configuration management
    let web_state = Arc::new(WebState {
        config: Arc::new(RwLock::new(config)),
        monitor_state: shared_monitor_state.clone(),
    });

    // Build router
    let app = axum::Router::new()
        .merge(alpaca::create_router(alpaca_state))
        .merge(web::create_router(web_state));

    // Start server
    let addr = format!("0.0.0.0:{}", server_port);
    info!("🚀 Server listening on {}", addr);
    info!("📊 Web UI available at http://localhost:{}", server_port);
    info!("🔌 ASCOM Alpaca API available at http://localhost:{}/api/v1/safetymonitor/0/", server_port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
