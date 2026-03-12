mod alpaca;
mod config;
mod logging;
mod monitors;
mod switches;
mod web;
mod weather;

// #TODO: After adding a new safety sensor, the system holds it's current state - which we want in order to inadvertently trigger an `unsafe` state while we wait for the first batch of data.  However if we add a new sensor that is offline or never returns data, we should respect the timeout period for that sensor.  Please check whether that is being done already.  If not, please add that functionality.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use tracing_subscriber;

use alpaca::discovery::DiscoveryService;
use alpaca::safety_monitor::{AppState, SafetyMonitor};
use alpaca::observing_conditions::ObservingConditionsAppState;
use alpaca::management::ManagementState;
use alpaca::{SwitchDevice, SwitchAppState};
use config::load_config;
use logging::DebugLogger;
use monitors::MonitorState;
use switches::SwitchManager;
use weather::{TempestListener, WeatherMonitorManager, ObservingConditionsDevice, WeatherDataAccessor};
use web::handlers::WebState;
use std::collections::HashMap;

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
        "Loaded configuration: {} MQTT servers, {} MQTT monitors, {} Alpaca monitors, {} weather devices, {} switches",
        config.mqtt_servers.len(),
        config.mqtt_monitors.len(),
        config.alpaca_monitors.len(),
        config.weather_devices.len(),
        config.switches.len()
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

    // Initialize Tempest UDP listener
    let tempest_listener = Arc::new(TempestListener::new(config.tempest_udp_port));
    let tempest_data = tempest_listener.get_data();

    // Start Tempest listener task
    let listener_clone = tempest_listener.clone();
    tokio::spawn(async move {
        if let Err(e) = listener_clone.start().await {
            error!("Tempest listener error: {}", e);
        }
    });
    info!("🌤️  Tempest UDP listener started on port {}", config.tempest_udp_port);

    // Create ObservingConditions devices from configuration
    let mut oc_devices = HashMap::new();
    for (device_number, device_config) in &config.weather_devices {
        if device_config.enabled {
            let device = Arc::new(ObservingConditionsDevice::new(device_config.clone()));
            oc_devices.insert(*device_number, device);
            info!("Created ObservingConditions device {}: {}", device_number, device_config.name);
        }
    }
    let oc_devices_shared = Arc::new(RwLock::new(oc_devices));

    // Create weather data accessor
    let weather_data_accessor = Arc::new(WeatherDataAccessor::new(tempest_data.clone()));

    // Initialize weather monitor manager for safety threshold monitoring
    let mut weather_monitor_manager = WeatherMonitorManager::new();
    weather_monitor_manager.initialize(&config.weather_devices, tempest_data.clone()).await;
    let shared_weather_monitors = Arc::new(RwLock::new(weather_monitor_manager));
    info!("Weather safety monitors initialized");

    let location = config.location.clone();
    let logging_enabled = config.logging_enabled;
    let server_interface = config.server_interface.clone();

    // Create debug logger
    let debug_logger = match DebugLogger::new(logging_enabled) {
        Ok(logger) => {
            info!("Debug logger initialized (enabled: {})", logging_enabled);
            Arc::new(logger)
        }
        Err(e) => {
            error!("Failed to initialize debug logger: {}", e);
            return Err(e);
        }
    };

    // Create safety monitor
    let safety_monitor = Arc::new(SafetyMonitor::new(device_name, location));

    // Create application state for ASCOM Alpaca endpoints
    let alpaca_state = Arc::new(AppState {
        safety_monitor: safety_monitor.clone(),
        monitor_state: shared_monitor_state.clone(),
        weather_monitors: shared_weather_monitors.clone(),
    });

    // Create ObservingConditions application state
    let oc_app_state = Arc::new(ObservingConditionsAppState {
        devices: oc_devices_shared.clone(),
        weather_data: weather_data_accessor.clone(),
    });

    // Create Switch device and manager (if enabled)
    let (switch_device, switch_manager) = if config.switch_device.enabled {
        let device = Arc::new(SwitchDevice::new(&config.switch_device));
        info!("🔘 Switch device created: {}", device.device_name);

        // Initialize switch manager with configured switches
        let mut manager = SwitchManager::new(shared_monitor_state.clone());
        manager.initialize(&config);
        info!("🔘 Switch manager initialized with {} switches", manager.get_switch_count());

        let shared_manager = Arc::new(RwLock::new(manager));
        (Some(device), Some(shared_manager))
    } else {
        (None, None)
    };

    // Create Switch application state (if switch device is enabled)
    let switch_app_state = match (&switch_device, &switch_manager) {
        (Some(device), Some(manager)) => Some(Arc::new(SwitchAppState {
            switch_device: device.clone(),
            switch_manager: manager.clone(),
        })),
        _ => None,
    };

    // Create management state for management API endpoints
    let management_state = Arc::new(ManagementState {
        safety_monitor: safety_monitor.clone(),
        monitor_state: shared_monitor_state.clone(),
        oc_devices: oc_devices_shared.clone(),
        switch_device: switch_device.clone(),
    });

    // Create web state for configuration management
    let web_state = Arc::new(WebState {
        config: Arc::new(RwLock::new(config)),
        monitor_state: shared_monitor_state.clone(),
        switch_manager: switch_manager.clone(),
        debug_logger: debug_logger.clone(),
        safety_monitor: safety_monitor.clone(),
        oc_devices: oc_devices_shared.clone(),
        weather_monitors: shared_weather_monitors.clone(),
        tempest_data: tempest_data.clone(),
    });

    // Start periodic status logging task
    let periodic_monitor_state = shared_monitor_state.clone();
    let periodic_logger = debug_logger.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;

            let monitor_state = periodic_monitor_state.read().await;
            let statuses = monitor_state.get_all_statuses();
            let is_safe = monitor_state.is_safe();
            drop(monitor_state);

            // Log state changes and periodic status
            if let Err(e) = periodic_logger.check_overall_state_change(is_safe, &statuses) {
                warn!("Failed to log state change: {}", e);
            }
            if let Err(e) = periodic_logger.log_periodic_status(is_safe, &statuses) {
                warn!("Failed to log periodic status: {}", e);
            }
        }
    });

    // Build router
    let mut app = axum::Router::new()
        .merge(alpaca::create_management_router(management_state))
        .merge(alpaca::create_router(alpaca_state))
        .merge(alpaca::create_observingconditions_router(oc_app_state));

    // Add Switch router if enabled
    if let Some(switch_state) = switch_app_state {
        app = app.merge(alpaca::create_switch_router(switch_state));
    }

    let app = app.merge(web::create_router(web_state));

    // Start ASCOM Alpaca discovery service
    let discovery = Arc::new(DiscoveryService::new(
        server_port,
        safety_monitor.device_name.clone(),
    ));
    discovery.start();
    info!("🔍 ASCOM Alpaca discovery service started on UDP port 32227");

    // Start server
    let addr = format!("{}:{}", server_interface, server_port);
    info!("🚀 Server listening on {}", addr);
    info!("📊 Web UI available at http://localhost:{}", server_port);
    info!("🔌 ASCOM Alpaca SafetyMonitor API: http://localhost:{}/api/v1/safetymonitor/0/", server_port);
    info!("🌤️  ASCOM Alpaca ObservingConditions API: http://localhost:{}/api/v1/observingconditions/{{device}}/", server_port);
    if switch_device.is_some() {
        info!("🔘 ASCOM Alpaca Switch API: http://localhost:{}/api/v1/switch/0/", server_port);
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
