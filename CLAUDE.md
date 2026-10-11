# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**LLAMA Safety Monitor** (Lightweight Logical ASCOM MQTT Alpaca Safety Monitor) is a Rust-based ASCOM Alpaca server that implements two device types:

1. **SafetyMonitor** - Aggregates multiple data sources to determine observatory safety
2. **ObservingConditions** - Provides weather data from Tempest stations and Ecowitt gateways, with an aggregate device (0) that combines them

The system monitors MQTT feeds, ASCOM Alpaca endpoints, UDP weather broadcasts and polled weather gateways to provide real-time safety status for astronomical observatory operations.

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
- Spawns background tasks (Tempest listener, periodic logging, discovery service); `WeatherService` spawns Ecowitt pollers

**`src/config/`** - Configuration management
- `models.rs` - All configuration data structures
- Defines `ComparisonOperator` enum for threshold comparisons
- Configuration validation logic

**`src/monitors/`** - Safety monitor implementations
- `mod.rs` - `MonitorState` aggregates all monitor types and determines overall safety
- `mqtt_monitor.rs` - `MqttMonitorManager` manages MQTT topic subscriptions and evaluates JSON path conditions
- `alpaca_monitor.rs` - `AlpacaMonitorManager` polls ASCOM Alpaca device properties

**`src/weather/`** - Weather device implementations
- `observation.rs` - `WeatherProperty`, source-independent `WeatherObservation` (metric units), `WeatherStore` (latest observation per station)
- `tempest.rs` - `TempestListener` receives UDP broadcasts from Tempest weather stations
- `ecowitt.rs` - `EcowittPollers` poll Ecowitt gateways' local `/get_livedata_info` API and convert units to metric
- `observing_conditions.rs` - `ObservingConditionsDevice`, `WeatherDataAccessor`, and aggregate resolution (`resolve_readings`)
- `weather_monitor.rs` - `WeatherMonitorManager` evaluates weather safety thresholds
- `service.rs` - `WeatherService` rebuilds devices, monitors and pollers from config (`apply_config`)

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
TempestListener (UDP) ─┐
EcowittPollers (HTTP) ─┴─> Arc<RwLock<WeatherStore>>  // latest WeatherObservation per station
                             └─ WeatherDataAccessor (+ shared AppConfig) resolves Readings per device
                                  ├─ ObservingConditions API handlers
                                  └─ WeatherMonitorManager (evaluates safety thresholds)

// WeatherService owns the config-dependent parts and rebuilds them on change
WeatherService
  ├─ devices: Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>
  ├─ monitors: Arc<RwLock<WeatherMonitorManager>>
  └─ Ecowitt pollers
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

### ObservingConditions Devices

**Device numbering:**
- Weather sources are configured in `weather_devices` with device numbers 1, 2, ... (0 is reserved)
- **Device 0 is the aggregate** (`weather_aggregate`): each property is served from the first source in its
  priority list whose data is newer than `stale_after_seconds`; if all are stale, the newest is served so safety
  timeouts still apply. `property_sources` overrides `source_priority` per property; an empty `source_priority`
  means all enabled sources in device-number order
- Device 0 exists once at least one source is enabled. With two or more enabled sources, each source with
  `expose_device: true` is also served as its own device
- Supported properties are fixed per source type (not by what data has arrived), so clients see a stable set

**Supported properties:**
- Temperature, Humidity, DewPoint, Pressure
- WindSpeed, WindGust, WindDirection
- RainRate, SkyBrightness (Tempest only)
- Solar radiation and UV are tracked and shown in the web UI but have no ASCOM property

**Data sources:**
- Tempest: UDP broadcasts on port 50222 (configurable)
- Ecowitt: `GET http://<host>/get_livedata_info` polled every `poll_interval_seconds` (GW1100/GW2000/GW3000).
  Values carry units from the gateway's display settings and are converted to metric; unknown units or `--` are dropped.
  Station pressure comes from `wh25.abs`. Test fixture: `src/weather/testdata/ecowitt_gw3000_livedata.json`
- Future: Weather Underground API (structure exists, not implemented)

**Property mapping:**
- Tempest `illuminance` (lux) → `SkyBrightness` (ASCOM defines SkyBrightness in lux; SkyQuality is mag/arcsec² and is not implemented)
- Tempest `rain_accumulation` (rain during the report interval) → `RainRate` (scaled to mm/hour)

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
      "DeviceName": "LLAMA Weather",
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

The aggregate device and individual weather sources can optionally enable safety monitoring
(usually on the aggregate, so a stale source falls back instead of tripping UNSAFE):

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
A configured threshold with no data for its property, or data older than its timeout, is UNSAFE immediately
(bypassing hold time). New monitors start UNSAFE; on config changes monitors keep their committed state.

### Config Migration

`AppConfig::migrate()` runs on load and on `POST /api/config`. Configs without `weather_aggregate` are upgraded:
the old weather device 0 becomes a source with the next free number and the aggregate takes over its name,
description, auto-connect and safety thresholds. The original file is saved as `config.json.pre-weather-aggregate.bak`.

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
- `GET|POST /api/weather/devices`, `GET|PUT|DELETE /api/weather/devices/:id`, `POST .../:id/toggle` - Weather sources (POST with `device_number: 0` assigns the next free number)
- `GET|PUT /api/weather/aggregate` - Aggregate device 0 configuration
- `GET /api/weather/status` - Readings (with source per measurement) and safety status for device 0 and each enabled source
- Weather changes apply immediately via `WeatherService::apply_config()` (no restart)

## Important Implementation Notes

### Error Handling
- ASCOM Alpaca errors use specific error codes (e.g., `0x400` for invalid device number)
- Monitor errors are captured in `MonitorStatus.error` field
- Monitors with errors are always considered UNSAFE

### Weather Data Freshness
- Tempest broadcasts are received asynchronously via UDP and stamped with the observation time
- Ecowitt observations are stamped with the time of the successful poll (the payload has no timestamp)
- `WeatherStore` keys observations as `tempest:<serial>` / `ecowitt:<host>`; a Tempest source without a serial uses the most recently heard Tempest
- `timeSinceLastUpdate` is per property, from the reading actually served (for device 0, the chosen source)
- Never hold the config `RwLock` while calling `WeatherDataAccessor::readings()` (it locks config itself);
  use `readings_with(&config, ...)` instead, since tokio's RwLock is write-preferring and nested reads can deadlock

### Auto-Connect Feature
- Weather sources and the aggregate support `auto_connect: true` in configuration
- When true, the device starts connected when it is created; existing devices keep their connected state across config changes
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
1. Add a variant to `WeatherProperty` in `weather/observation.rs` (and to `ALL`, labels, units, ASCOM name)
2. Populate it in each source's conversion (`TempestObservation::to_observation`, `ecowitt::parse_livedata`)
3. Add it to `supported_properties()` for the source types that provide it
4. Add an API endpoint in `alpaca/observing_conditions.rs` (`property_getter!`) if it is an ASCOM property
5. For safety thresholds: add a field to `WeatherSafetyThresholds` and to `WeatherSafetyThresholds::configured()`
6. Add it to `WEATHER_PROPERTIES` in `static/js/weather.js`

### Adding a New Weather Source Type
1. Add a variant to `WeatherDataSource` in `config/models.rs`
2. Write observations into `WeatherStore` under a unique key and resolve it in `WeatherStore::observation_for`
3. Declare its properties in `supported_properties()`
4. If it is polled, start/stop it from `WeatherService::apply_config`

### Testing ASCOM Compliance
- Use ASCOM Remote or ASCOM Conform tool for validation
- Discovery service must respond on UDP port 32227
- Device must implement all required ASCOM common properties
- Connected/disconnected state must be properly managed
