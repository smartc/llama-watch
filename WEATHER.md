# ASCOM Alpaca Observing Conditions Weather Monitor

This document describes the weather monitoring features added to llama-watch.

## Features

### ASCOM Alpaca ObservingConditions Device

The application now supports ASCOM Alpaca ObservingConditions devices that provide weather data from various sources:

- **Tempest Weather Station**: Receives UDP broadcasts from WeatherFlow Tempest weather stations
- **Weather Underground**: (Future enhancement) Polls Weather Underground API for weather data

### Supported Properties

The ObservingConditions device implements the following ASCOM properties:

| Property | Unit | Source | Notes |
|----------|------|--------|-------|
| Temperature | °C | Tempest | Ambient air temperature |
| Humidity | % | Tempest | Relative humidity |
| DewPoint | °C | Tempest | Calculated from temperature and humidity |
| Pressure | MB | Tempest | Atmospheric pressure |
| WindSpeed | m/s | Tempest | Average wind speed |
| WindGust | m/s | Tempest | Maximum wind gust |
| WindDirection | degrees | Tempest | Wind direction (0-360°) |
| RainRate | mm/hour | Tempest | Calculated from precipitation accumulation |
| SkyBrightness | mag/arcsec² | Tempest | Converted from illuminance (lux) |

### Weather Safety Monitoring

Weather devices can optionally integrate with the Safety Monitor to provide automated safe/unsafe determination based on configurable thresholds:

- **Temperature**: Monitor for freezing or extreme heat
- **Humidity**: Detect high humidity conditions
- **Sky Brightness**: Monitor light pollution or dawn/dusk
- **Wind Speed**: Alert on high winds
- **Wind Gust**: Alert on dangerous gusts
- **Rain Rate**: Detect precipitation

Each threshold is optional and configurable with:
- Comparison operator (greater than, less than, equal)
- Threshold value
- Safe/unsafe interpretation
- Hold time (prevent flapping)
- Timeout (mark unsafe if no data)

## Configuration

### Server Settings

```json
{
  "server_interface": "127.0.0.1",
  "server_port": 8080,
  "tempest_udp_port": 50222
}
```

- `server_interface`: IP address to bind the server (default: "127.0.0.1", use "0.0.0.0" for all interfaces)
- `server_port`: Port for HTTP server (default: 8080)
- `tempest_udp_port`: Port for Tempest UDP broadcasts (default: 50222)

### Weather Device Configuration

```json
{
  "weather_devices": {
    "0": {
      "device_number": 0,
      "name": "Tempest Weather Station",
      "description": "WeatherFlow Tempest all-in-one weather station",
      "source": {
        "type": "tempest",
        "serial_number": null
      },
      "enabled": true,
      "auto_connect": true,
      "safety_thresholds": {
        "enabled": true,
        "timeout_seconds": 300,
        "hold_time_seconds": 60,
        "wind_gust": {
          "threshold": 20.0,
          "operator": "greaterthan",
          "safe_when_true": false
        }
      }
    }
  }
}
```

**Fields:**
- `device_number`: ASCOM device number (0-based)
- `name`: Display name for the device
- `description`: Device description
- `source.type`: Data source type ("tempest" or "weatherunderground")
- `source.serial_number`: Specific Tempest serial number (null = accept any)
- `enabled`: Enable/disable the device
- `auto_connect`: Automatically set device to connected state on startup
- `safety_thresholds`: Optional safety monitoring configuration

### Safety Threshold Configuration

Each measurement can have an optional threshold:

```json
{
  "temperature": {
    "threshold": -10.0,
    "operator": "lessthan",
    "safe_when_true": false
  }
}
```

**Operators:**
- `"greaterthan"`: value > threshold
- `"lessthan"`: value < threshold
- `"equal"`: value == threshold

**Interpretation:**
- `safe_when_true: true`: Condition met = SAFE
- `safe_when_true: false`: Condition met = UNSAFE

**Example**: Wind gust > 20 m/s → UNSAFE
```json
{
  "wind_gust": {
    "threshold": 20.0,
    "operator": "greaterthan",
    "safe_when_true": false
  }
}
```

## ASCOM Alpaca API

### ObservingConditions Endpoints

```
GET  /api/v1/observingconditions/{device}/connected
PUT  /api/v1/observingconditions/{device}/connected
GET  /api/v1/observingconditions/{device}/temperature
GET  /api/v1/observingconditions/{device}/humidity
GET  /api/v1/observingconditions/{device}/dewpoint
GET  /api/v1/observingconditions/{device}/pressure
GET  /api/v1/observingconditions/{device}/windspeed
GET  /api/v1/observingconditions/{device}/windgust
GET  /api/v1/observingconditions/{device}/winddirection
GET  /api/v1/observingconditions/{device}/rainrate
GET  /api/v1/observingconditions/{device}/skybrightness
GET  /api/v1/observingconditions/{device}/sensordescription?SensorName={name}
GET  /api/v1/observingconditions/{device}/timesincelastupdate?SensorName={name}
PUT  /api/v1/observingconditions/{device}/refresh
```

### Management API

The management API now lists all configured devices:

```
GET /management/v1/configureddevices
```

Returns both SafetyMonitor and ObservingConditions devices.

## Tempest Weather Station Setup

1. Ensure your Tempest weather station is on the same network
2. Tempest Hub broadcasts UDP messages on port 50222
3. Configure a weather device with `"type": "tempest"`
4. Optionally specify a serial number to filter specific devices
5. Start the application - it will automatically listen for UDP broadcasts

## CONFORM Testing

The ObservingConditions device is designed to pass ASCOM CONFORM Universal testing:

- All required ASCOM common properties implemented
- Proper error codes for unsupported properties
- Transaction ID handling
- Connected/disconnected state management
- UDP discovery protocol support

## Integration with Safety Monitor

When safety thresholds are enabled on a weather device, the overall safety status includes weather conditions. The Safety Monitor `/api/v1/safetymonitor/0/issafe` endpoint will return:

- `false` if ANY weather threshold is exceeded
- Safety comments include which weather measurements are unsafe
- Timeout handling marks weather unsafe if no recent data
- Hold time prevents state flapping on borderline conditions

## Sources

- [ASCOM Alpaca ObservingConditions API](https://ascom-standards.org/alpyca/alpaca.observingconditions.html)
- [Tempest UDP API Documentation](https://weatherflow.github.io/Tempest/api/udp.html)
- [ASCOM Master Interfaces](https://ascom-standards.org/newdocs/)
