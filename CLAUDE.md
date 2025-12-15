# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**LLAMA Safety Monitor** (Lightweight Logical ASCOM MQTT Alpaca Safety Monitor) is a Rust-based ASCOM Alpaca server that implements two device types:

1. **SafetyMonitor** - Aggregates multiple data sources to determine observatory safety
2. **ObservingConditions** - Provides weather data from Tempest weather stations

The system monitors MQTT feeds, ASCOM Alpaca endpoints, and UDP weather broadcasts to provide real-time safety status for astronomical observatory operations.

## Common Development Commands

### Build & Run
```bash
# Build in development mode
cargo build

# Build optimized release
cargo build --release

# Run the application
cargo run --release

# Binary location after build
./target/release/llama-watch
```

### Testing
```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture
```

### Logging
The application uses the `tracing` crate with environment-based log filtering:

```bash
# Default logging (info level)
cargo run

# Debug logging
RUST_LOG=llama_watch=debug cargo run

# Trace all components
RUST_LOG=trace cargo run
```

### Configuration
- Configuration is stored in `config.json` (created on first run if missing)
- See `config.example.json` for a complete example with all available options
- The web UI at `http://localhost:8080` provides live configuration editing

## Architecture

### Core Components

The application is organized into distinct modules with clear separation of concerns:

**`src/main.rs`** - Application entry point
- Initializes all subsystems (monitors, weather, web server, ASCOM API)
- Sets up shared state using `Arc<RwLock<T>>` for thread-safe state management
- Spawns background tasks (Tempest listener, periodic logging, discovery service)

**`src/config/`** - Configuration management
- `models.rs` - All configuration data structures
- Defines `ComparisonOperator` enum for threshold comparisons
- Configuration validation logic

**`src/monitors/`** - Safety monitor implementations
- `mod.rs` - `MonitorState` aggregates all monitor types and determines overall safety
- `mqtt_monitor.rs` - `MqttMonitorManager` manages MQTT topic subscriptions and evaluates JSON path conditions
- `alpaca_monitor.rs` - `AlpacaMonitorManager` polls ASCOM Alpaca device properties

**`src/weather/`** - Weather device implementations
- `tempest.rs` - `TempestListener` receives UDP broadcasts from Tempest weather stations
- `observing_conditions.rs` - `ObservingConditionsDevice` implements ASCOM ObservingConditions interface
- `weather_monitor.rs` - `WeatherMonitorManager` evaluates weather safety thresholds

**`src/alpaca/`** - ASCOM Alpaca API implementation
- `safety_monitor.rs` - SafetyMonitor device API endpoints
- `observing_conditions.rs` - ObservingConditions device API endpoints
- `management.rs` - Management API (device discovery and enumeration)
- `discovery.rs` - UDP discovery service (responds to ASCOM discovery broadcasts)
- `models.rs` - ASCOM Alpaca data types and response structures

**`src/web/`** - Web interface and configuration API
- `handlers.rs` - REST API endpoints for configuration management
- Serves static files from `static/` directory

**`src/logging.rs`** - Debug logging to files
- `DebugLogger` writes timestamped logs to `logs/` directory when enabled

### State Management Architecture

The application uses a sophisticated state sharing pattern:

```rust
// Core monitor state - determines overall safety
Arc<RwLock<MonitorState>>
  ├─ MqttMonitorManager (manages N MQTT monitors)
  └─ AlpacaMonitorManager (manages N Alpaca monitors)

// Weather data pipeline
Arc<RwLock<HashMap<String, TempestObservation>>>  // Raw Tempest data
  ├─ WeatherDataAccessor (provides data to ObservingConditions devices)
  └─ WeatherMonitorManager (evaluates safety thresholds)

// ASCOM device instances
Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>
```

**State is shared across:**
- ASCOM Alpaca API handlers (read safety status, weather data)
- Web API handlers (read/write configuration, reload monitors)
- Background tasks (update monitor values, log state changes)

**Threading model:**
- Tokio async runtime with `#[tokio::main]`
- `Arc<RwLock<T>>` for async-safe shared state (from `tokio::sync`)
- `Arc<parking_lot::RwLock<T>>` for synchronous shared state (ASCOM device connected flag)
- Each MQTT monitor spawns its own async task
- Tempest listener runs in a background task
- Periodic logging runs every 30 seconds in a background task

### Safety Determination Logic

The `MonitorState::is_safe()` method implements complex logic to handle initialization and state transitions:

1. **No monitors configured** → UNSAFE (cannot determine safety)
2. **Monitors initializing (no data yet):**
   - If ANY monitor with data is UNSAFE → UNSAFE immediately
   - If all monitors with data are SAFE but some initializing → Hold last stable state (default UNSAFE)
   - Prevents false SAFE during partial initialization
3. **All monitors have data:**
   - SAFE only if ALL monitors are safe AND no errors
   - ONE unsafe monitor → entire system UNSAFE

**Hold time feature:** Monitors can specify `hold_time_seconds` to prevent state flapping. The pending state must persist for the hold time before the transition is committed.

**Timeout handling:** If a monitor doesn't receive data within `timeout_seconds`, it's marked UNSAFE with an error.

### Monitor Reload Behavior

When configuration is reloaded (`/reload` endpoint):

1. Capture current stable state (if all monitors have data)
2. Save monitor statuses before shutdown
3. Shutdown all MQTT connections and Alpaca polling tasks
4. Reinitialize monitors with new configuration
5. Restore previous `is_safe` states to prevent false UNSAFE during reinitialization
6. Clear pending states (hold times reset)

This prevents brief UNSAFE blips when adding/removing monitors.

## ASCOM Alpaca Implementation Details

### SafetyMonitor Device (Device Number 0)

**Key endpoints:**
- `GET /api/v1/safetymonitor/0/issafe` - Returns overall safety (aggregates all monitors)
- `PUT /api/v1/safetymonitor/0/connected` - Set device connected state
- Standard ASCOM common properties (name, description, driverinfo, etc.)

**Connected state behavior:**
- Device starts in disconnected state (per ASCOM standard)
- When disconnected, `issafe` returns `false` (not an error)
- Client must call `PUT /connected` with `Connected=true` to activate

**Transaction IDs:**
- Server maintains `AtomicU32` counter for server transaction IDs
- Each response includes both client and server transaction IDs

### ObservingConditions Device (Device Number 0+)

**Supported properties:**
- Temperature, Humidity, DewPoint, Pressure
- WindSpeed, WindGust, WindDirection
- RainRate, SkyBrightness

**Data sources:**
- Tempest: UDP broadcasts on port 50222 (configurable)
- Future: Weather Underground API (structure exists, not implemented)

**Property mapping:**
- Tempest `illuminance` (lux) → `SkyBrightness` (mag/arcsec² using conversion formula)
- Tempest `rain_accumulation` → `RainRate` (calculated from time-based changes)

### Management API

`GET /management/v1/configureddevices` returns all configured devices:
```json
{
  "Value": [
    {
      "DeviceName": "LLAMA Safety Monitor",
      "DeviceType": "SafetyMonitor",
      "DeviceNumber": 0,
      "UniqueID": "..."
    },
    {
      "DeviceName": "Tempest Weather Station",
      "DeviceType": "ObservingConditions",
      "DeviceNumber": 0,
      "UniqueID": "..."
    }
  ]
}
```

## Configuration Model

### Comparison Operators and Safety Logic

All monitors use the same threshold evaluation:

```rust
enum ComparisonOperator {
    GreaterThan,  // value > threshold
    LessThan,     // value < threshold
    Equal,        // value == threshold (with f64::EPSILON tolerance)
}
```

**`safe_when_true` flag inverts the interpretation:**
- `true` → Condition met = SAFE
- `false` → Condition met = UNSAFE

**Examples:**
```json
// UNSAFE when voltage drops below 12V
{
  "threshold": 12.0,
  "operator": "lessthan",
  "safe_when_true": false
}

// SAFE when temperature is above 0°C
{
  "threshold": 0.0,
  "operator": "greaterthan",
  "safe_when_true": true
}
```

### MQTT Monitor Configuration

JSON path extraction uses the `jsonpath-rust` crate:
- Supports dot notation: `Volts.1`, `nested.object.value`
- Optional `$` prefix: `$.Volts.1` (both work)
- If `json_path` is `None`, expects raw numeric MQTT payload

### Weather Device Safety Thresholds

Weather devices can optionally enable safety monitoring:

```json
{
  "safety_thresholds": {
    "enabled": true,
    "timeout_seconds": 300,  // Global default
    "hold_time_seconds": 60,  // Global default
    "temperature": {
      "threshold": -10.0,
      "operator": "lessthan",
      "safe_when_true": false,
      "timeout_seconds": 120  // Per-property override
    }
  }
}
```

When enabled, weather thresholds are evaluated by `WeatherMonitorManager` and contribute to overall safety status.

## Web Interface

Static files in `static/`:
- `index.html` - Main configuration UI
- `css/` - Stylesheets
- `js/` - JavaScript application

**Key API endpoints:**
- `GET /config` - Retrieve current configuration
- `POST /config` - Update configuration (does not reload monitors)
- `POST /reload` - Reload monitors from current config (call after saving config)
- `GET /status` - Get all monitor statuses
- `GET /logs` - List available log files (when logging enabled)

## Important Implementation Notes

### Error Handling
- ASCOM Alpaca errors use specific error codes (e.g., `0x400` for invalid device number)
- Monitor errors are captured in `MonitorStatus.error` field
- Monitors with errors are always considered UNSAFE

### Weather Data Freshness
- Tempest broadcasts are received asynchronously via UDP
- `TempestListener` maintains `Arc<RwLock<HashMap<String, TempestObservation>>>`
- Multiple devices can share the same Tempest data source (filtered by serial_number)
- `timeSinceLastUpdate` calculated from `observation.timestamp` vs current time

### Auto-Connect Feature
- Weather devices support `auto_connect: true` in configuration
- When true, device is automatically set to connected state on startup
- Useful for automated observatory systems

### Unique Device IDs
- Generated from MAC address + device name using SHA-256
- Ensures stable device identity for ASCOM clients
- Implemented in `alpaca/models.rs::generate_unique_id()`

## Common Patterns When Modifying Code

### Adding a New Monitor Type
1. Create new module in `src/monitors/`
2. Implement manager struct with `add_monitor()`, `get_statuses()`, `shutdown()` methods
3. Add manager instance to `MonitorState` struct
4. Update `MonitorState::initialize()` to initialize new monitors
5. Update `MonitorState::get_all_statuses()` to include new monitor statuses

### Adding a New Weather Property
1. Add field to `TempestObservation` struct in `weather/tempest.rs`
2. Parse field in `TempestMessage::Observation` handler
3. Add getter method to `ObservingConditionsDevice`
4. Add API endpoint in `alpaca/observing_conditions.rs`
5. Add threshold field to `WeatherSafetyThresholds` in `config/models.rs`
6. Update `WeatherMonitorManager::evaluate_safety()` to check new threshold

### Testing ASCOM Compliance
- Use ASCOM Remote or ASCOM Conform tool for validation
- Discovery service must respond on UDP port 32227
- Device must implement all required ASCOM common properties
- Connected/disconnected state must be properly managed
