# 🦙 LLAMA Safety Monitor

**L**ightweight **L**ogical **A**SCOM **M**QTT **A**lpaca Safety Monitor

A Rust-based ASCOM Alpaca Safety Monitor that monitors multiple MQTT feeds and ASCOM Alpaca endpoints to determine observatory safety conditions.

## Features

- **ASCOM Alpaca Safety Monitor Interface**: Full implementation of the ASCOM Alpaca SafetyMonitor device specification
- **MQTT Monitoring**: Monitor any number of MQTT topics with JSON path extraction
- **ASCOM Alpaca Endpoint Monitoring**: Monitor other ASCOM Alpaca devices and properties
- **Flexible Threshold Comparisons**: Support for `>`, `<`, and `==` operators
- **JSON Path Extraction**: Extract specific values from nested JSON objects in MQTT messages
- **Web-based Configuration**: Easy-to-use web interface for managing monitors
- **Real-time Status Display**: Live monitoring dashboard showing all configured monitors
- **Persistent Configuration**: Configuration stored in JSON format

## Use Cases

Monitor various observatory conditions:

- Solar panel voltage levels
- Weather station data
- Roof/dome position sensors
- Power system status
- Environmental sensors (temperature, humidity, wind, etc.)
- Safety monitors from other devices

## Installation

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- MQTT broker (if using MQTT monitors)
- ASCOM Alpaca devices (if using Alpaca monitors)

### Building from Source

```bash
# Clone the repository
git clone <repository-url>
cd llama-watch

# Build the project
cargo build --release

# The binary will be in target/release/llama-watch
```

## Quick Start

1. **Run the server**:

```bash
cargo run --release
```

or

```bash
./target/release/llama-watch
```

2. **Open the web interface**:

Navigate to `http://localhost:8080` in your web browser.

3. **Configure your monitors**:

   - Add MQTT servers (if using MQTT)
   - Add MQTT monitors with JSON paths and thresholds
   - Add ASCOM Alpaca monitors
   - Save your configuration

4. **Connect ASCOM clients**:

Use the ASCOM Alpaca endpoint at:
```
http://localhost:8080/api/v1/safetymonitor/0/issafe
```

## Configuration

### MQTT Monitors

#### Example: Solar Panel Voltage Monitor

Given this MQTT message at topic `observatory/power/solar1`:

```json
{
  "Amps": 67.96,
  "Volts.0": -0.23,
  "Volts.1": 12.84,
  "Power": 872.6,
  "Temperature": 20.36,
  "Pressure": 880.9
}
```

Configure the monitor:

- **Topic**: `observatory/power/solar1`
- **JSON Path**: `Volts.1` (or `$.Volts.1`)
- **Threshold**: `12.00`
- **Operator**: `Less Than (<)`
- **Safe when TRUE**: Unchecked

This monitor will report **UNSAFE** when `Volts.1 < 12.00`.

#### JSON Path Syntax

The JSON path supports dot notation for nested objects:

- `Volts.1` → Extracts `12.84` from the example above
- `Temperature` → Extracts `20.36`
- `$.nested.object.value` → Use `$` prefix for explicit root

### ASCOM Alpaca Monitors

#### Example: Safety Monitor

Monitor another ASCOM Alpaca safety monitor:

- **Host**: `192.168.1.100`
- **Port**: `11111`
- **Device Type**: `safetymonitor`
- **Device Number**: `0`
- **Property**: `issafe`
- **Threshold**: `1.0`
- **Operator**: `Equal (=)`
- **Safe when TRUE**: Checked

This monitor will report **SAFE** when the remote safety monitor's `issafe` property equals `1.0` (true).

### Comparison Logic

The `Safe when TRUE` checkbox controls the safety logic:

- **Checked**: Safe when comparison is TRUE
  - Example: `value > 12.0` → Safe when voltage is above 12V
- **Unchecked**: Safe when comparison is FALSE (or unsafe when TRUE)
  - Example: `value < 12.0` → Unsafe when voltage drops below 12V

### Overall Safety Status

The LLAMA Safety Monitor reports **SAFE** only when:

1. **All** configured monitors report safe
2. **No** monitors have errors
3. All monitors have received at least one update

If any monitor reports unsafe or has an error, the overall status is **UNSAFE**.

## ASCOM Alpaca API

### Endpoints

The following ASCOM Alpaca SafetyMonitor endpoints are implemented:

- `GET /api/v1/safetymonitor/0/issafe` - Returns overall safety status
- `GET /api/v1/safetymonitor/0/connected` - Returns connection status
- `PUT /api/v1/safetymonitor/0/connected` - Set connection status
- `GET /api/v1/safetymonitor/0/name` - Returns device name
- `GET /api/v1/safetymonitor/0/description` - Returns device description
- `GET /api/v1/safetymonitor/0/driverinfo` - Returns driver information
- `GET /api/v1/safetymonitor/0/driverversion` - Returns driver version
- `GET /api/v1/safetymonitor/0/interfaceversion` - Returns interface version
- `GET /api/v1/safetymonitor/0/supportedactions` - Returns supported actions

### Example Usage

```bash
# Check if safe
curl "http://localhost:8080/api/v1/safetymonitor/0/issafe?ClientID=1&ClientTransactionID=1"

# Response:
{
  "Value": true,
  "ClientTransactionID": 1,
  "ServerTransactionID": 1,
  "ErrorNumber": 0,
  "ErrorMessage": ""
}
```

## Configuration File

Configuration is stored in `config.json` in the working directory:

```json
{
  "mqtt_servers": {
    "mqtt-main": {
      "id": "mqtt-main",
      "host": "mqtt.example.com",
      "port": 1883,
      "username": null,
      "password": null
    }
  },
  "mqtt_monitors": {
    "solar-voltage": {
      "id": "solar-voltage",
      "name": "Solar Panel Voltage",
      "server_id": "mqtt-main",
      "topic": "observatory/power/solar1",
      "json_path": "Volts.1",
      "threshold": 12.0,
      "operator": "lessthan",
      "safe_when_true": false
    }
  },
  "alpaca_monitors": {},
  "server_port": 8080,
  "device_name": "LLAMA Safety Monitor"
}
```

## Environment Variables

- `RUST_LOG`: Set logging level (default: `llama_watch=info,tower_http=debug`)

```bash
RUST_LOG=llama_watch=debug cargo run
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    LLAMA Safety Monitor                  │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌────────────────┐         ┌────────────────┐         │
│  │  MQTT Monitor  │         │ Alpaca Monitor │         │
│  │    Manager     │         │    Manager     │         │
│  └────────────────┘         └────────────────┘         │
│         │                           │                   │
│         ├─ MQTT Monitor 1          ├─ Alpaca Monitor 1 │
│         ├─ MQTT Monitor 2          └─ Alpaca Monitor 2 │
│         └─ MQTT Monitor N                               │
│                                                          │
│  ┌──────────────────────────────────────────┐          │
│  │         Monitor State Aggregator          │          │
│  │  (Combines all monitors → is_safe)       │          │
│  └──────────────────────────────────────────┘          │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐                    │
│  │ ASCOM Alpaca │  │   Web API    │                    │
│  │ SafetyMonitor│  │ (Config UI)  │                    │
│  │   Interface  │  │              │                    │
│  └──────────────┘  └──────────────┘                    │
└─────────────────────────────────────────────────────────┘
```

## Development

### Project Structure

```
llama-watch/
├── Cargo.toml
├── src/
│   ├── main.rs              # Application entry point
│   ├── alpaca/              # ASCOM Alpaca implementation
│   │   ├── mod.rs
│   │   ├── models.rs        # Alpaca data models
│   │   └── safety_monitor.rs # SafetyMonitor endpoints
│   ├── config/              # Configuration management
│   │   ├── mod.rs
│   │   └── models.rs        # Config data models
│   ├── monitors/            # Monitor implementations
│   │   ├── mod.rs
│   │   ├── mqtt_monitor.rs  # MQTT monitoring
│   │   └── alpaca_monitor.rs # Alpaca endpoint monitoring
│   └── web/                 # Web interface
│       ├── mod.rs
│       └── handlers.rs      # API handlers
├── static/                  # Web UI assets
│   ├── index.html
│   └── app.js
└── README.md
```

### Running Tests

```bash
cargo test
```

### Building for Release

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

## Troubleshooting

### MQTT Connection Issues

- Verify MQTT broker is running and accessible
- Check firewall rules
- Verify credentials (if authentication is enabled)
- Check MQTT topic subscriptions

### ASCOM Alpaca Monitor Issues

- Verify the remote Alpaca device is accessible
- Check device type and number are correct
- Verify property name is lowercase
- Check network connectivity

### Web Interface Not Loading

- Verify port 8080 is not in use
- Check firewall allows incoming connections
- Verify `static/` directory exists and contains `index.html` and `app.js`

### Monitor Showing Errors

- Check monitor configuration in web UI
- Review JSON path syntax for MQTT monitors
- Verify threshold values are numeric
- Check application logs with `RUST_LOG=debug`

## License

MIT License - See LICENSE file for details

## Contributing

Contributions welcome! Please submit issues and pull requests on GitHub.

## Credits

Built with:

- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [rumqttc](https://github.com/bytebeamio/rumqtt) - MQTT client
- [reqwest](https://github.com/seanmonstar/reqwest) - HTTP client
- [serde](https://github.com/serde-rs/serde) - Serialization
- [tokio](https://github.com/tokio-rs/tokio) - Async runtime

## ASCOM Alpaca Specification

This project implements the ASCOM Alpaca SafetyMonitor device specification:

- [ASCOM Alpaca API](https://ascom-standards.org/Developer/Alpaca.htm)
- [SafetyMonitor Interface](https://ascom-standards.org/Help/Developer/html/T_ASCOM_DeviceInterface_ISafetyMonitor.htm)
