# Weather Sources and ObservingConditions Devices

llama-watch collects weather data from one or more weather sources and serves it as ASCOM Alpaca
**ObservingConditions** devices. It can also feed weather conditions into the Safety Monitor.

## Weather Sources

| Source | How data arrives | Notes |
|--------|------------------|-------|
| **Tempest** (WeatherFlow) | UDP broadcasts from the hub on port 50222 | Filter by serial number, or accept any Tempest on the network |
| **Ecowitt gateway** (GW1100/GW2000/GW3000) | Polls `http://<gateway>/get_livedata_info` | Units follow the gateway's display settings and are converted to metric. Station pressure comes from the gateway's built-in sensor |
| **ASCOM Alpaca ObservingConditions device** | Polls another Alpaca server | Use **Detect Properties** in the web UI to choose which properties to take. Uses the device's own `TimeSinceLastUpdate`, so a stuck sensor is noticed. Read-only: never changes the remote device's Connected state |
| **Weather Underground PWS** | Polls the PWS current observations API | Needs your station ID and a PWS API key (wunderground.com → My Profile → API Keys). Poll interval at least 60 s (1500 calls/day). See [Weather Underground pressure](#weather-underground-pressure) |

Sources are added, edited and removed on the **Weather Devices** tab of the web UI. Changes apply immediately, with no restart.

## ObservingConditions Devices

- **Device 0 is the aggregate device.** Each measurement is served from the best available source (see [Source selection](#source-selection)). It exists as soon as one source is enabled.
- **Devices 1, 2, …** are the individual sources. Each source keeps a fixed device number. Sources are only served as their own devices once **two or more** are enabled; with a single source, device 0 is enough. Untick *Serve as its own ASCOM device* to keep a source feeding the aggregate without exposing it.

### Supported properties

| Property | Unit | Tempest | Ecowitt | Alpaca | Weather Underground |
|----------|------|:-------:|:-------:|:------:|:-------------------:|
| Temperature | °C | ✓ | ✓ | ✓ | ✓ |
| Humidity | % | ✓ | ✓ | ✓ | ✓ |
| DewPoint | °C | ✓ (calculated) | ✓ | ✓ | ✓ |
| Pressure | hPa, at the station (not sea level) | ✓ | ✓ | ✓ | ✓ with altitude set |
| WindSpeed / WindGust | m/s | ✓ | ✓ | ✓ | ✓ |
| WindDirection | degrees | ✓ | ✓ | ✓ | ✓ |
| RainRate | mm/hour | ✓ | ✓ | ✓ | ✓ |
| SkyBrightness | lux | ✓ | | ✓ | |
| SkyQuality | mag/arcsec² | | | ✓ | |
| SkyTemperature | °C | | | ✓ | |
| CloudCover | % | | | ✓ | |

For an Alpaca source, only the properties you selected and the remote device implements are used. Solar radiation and UV index are collected from Tempest, Ecowitt and Weather Underground and shown in the web UI. ASCOM has no property for them.

A device reports `NotImplemented` for properties none of its sources provide. The set is fixed by configuration, not by which data has arrived, so clients see a stable list.

## Source Selection

Edit the aggregate device on the Weather Devices tab to control where each measurement comes from.

- **Default source order:** sources are used in the numbered order. Click a source to stop or start using it, and use ‹ › to move it.
- **Per-measurement order:** each measurement lists only the sources that provide it, with their current values. A row follows the default order until you change it; **Reset** returns it to the default.

For each measurement, the aggregate uses the first source in its order whose data is:

1. **fresh**: newer than **Stale After** (default 300 s), and
2. **steady**: reporting without gaps for **Return After** (default 300 s), or since llama-watch started.

So the aggregate stays on your preferred source while it keeps reporting. A single missed reading doesn't count as dropping out: the last value is kept and simply gets older until the source reports again. If that source drops out, the measurement falls back to the next one. When the preferred source recovers, it only takes over again once it has reported steadily for the Return After time, so an intermittent source can't make values bounce back and forth. If every source is stale, the most recent reading is still served so that safety timeouts apply to it.

Keep Stale After well above each source's report interval. Tempest reports every 60 s.

Every change of source is logged:

```
Aggregate Wind Speed: switched from Tempest Weather Station (#1) to Ecowitt GW3000 (#2); its data is 312s old
Aggregate Wind Speed: back to Tempest Weather Station (#1) (was Ecowitt GW3000 (#2))
```

The status page shows which source each aggregate measurement currently comes from.

## Weather Safety Monitoring

The aggregate device, and optionally individual sources, can have safety thresholds that feed the Safety Monitor. Put thresholds on the aggregate so a failed source falls back to another instead of making the system UNSAFE.

Available thresholds: temperature, humidity, wind speed, wind gust, rain rate, sky brightness, sky quality, sky temperature and cloud cover. Each has an operator (greater than, less than, equal), a value, and a *Safe when TRUE* setting. They share a timeout and a hold time, and each threshold can override the timeout.

- A configured threshold with **no data**, or data older than its timeout, is UNSAFE immediately, bypassing the hold time.
- Otherwise a state change must persist for the **hold time** before it takes effect.
- Weather monitors start UNSAFE and become SAFE once conditions have been safe for the hold time.
- `issafe` is false if any weather threshold is UNSAFE. The Safety Monitor's comments say which.

```json
"safety_thresholds": {
  "enabled": true,
  "timeout_seconds": 300,
  "hold_time_seconds": 60,
  "wind_gust": { "threshold": 10.0, "operator": "greaterthan", "safe_when_true": false },
  "cloud_cover": { "threshold": 50.0, "operator": "greaterthan", "safe_when_true": false }
}
```

## Weather Underground Pressure

Weather Underground only stores the **sea-level (relative)** pressure a station uploads, and its API has no absolute value. ASCOM Pressure is the pressure **at the station**. To use Weather Underground pressure, set **Station Altitude for Pressure** to the altitude your station uses to calculate its sea-level pressure (for an Ecowitt gateway, the altitude in its pressure calibration). llama-watch converts the value back using the current temperature, typically to within about 1 hPa. Leave it blank and Weather Underground provides no pressure.

## Configuration

Weather settings are edited in the web UI. In `config.json` they look like this (see `config.example.json` for a full example):

```json
{
  "tempest_udp_port": 50222,
  "weather_devices": {
    "1": {
      "device_number": 1,
      "name": "Tempest Weather Station",
      "description": "WeatherFlow Tempest",
      "source": { "type": "tempest", "serial_number": null },
      "enabled": true,
      "auto_connect": true,
      "expose_device": true
    },
    "2": {
      "device_number": 2,
      "name": "Ecowitt GW3000",
      "description": "",
      "source": { "type": "ecowitt", "host": "ecowitt.local", "poll_interval_seconds": 30 },
      "enabled": true
    },
    "3": {
      "device_number": 3,
      "name": "Sky Conditions",
      "description": "",
      "source": {
        "type": "alpaca", "host": "skycon.local", "port": 11111, "device_number": 0,
        "poll_interval_seconds": 30,
        "properties": ["temperature", "sky_brightness", "sky_quality", "sky_temperature", "cloud_cover"]
      },
      "enabled": true
    },
    "4": {
      "device_number": 4,
      "name": "Weather Underground",
      "description": "",
      "source": {
        "type": "weatherunderground", "station_id": "YOUR_STATION_ID", "api_key": "YOUR_PWS_API_KEY",
        "poll_interval_seconds": 120, "pressure_altitude_m": 1100
      },
      "enabled": true
    }
  },
  "weather_aggregate": {
    "name": "LLAMA Weather",
    "description": "",
    "auto_connect": true,
    "stale_after_seconds": 300,
    "return_after_seconds": 300,
    "source_priority": [],
    "property_sources": { "temperature": [3, 2, 1] },
    "safety_thresholds": null
  }
}
```

- `weather_devices`: the sources, numbered from 1. Device 0 is reserved for the aggregate.
- `source_priority`: default source order. Empty means all enabled sources in device-number order, and sources added later are included automatically.
- `property_sources`: per-measurement source order, overriding `source_priority` for that measurement.

**Upgrading:** configs from earlier versions (with the weather station as device 0) are converted automatically on startup. The old device 0 becomes source 1, and the new aggregate device 0 takes over its name and safety thresholds, so ASCOM clients and safety behaviour are unchanged. The original file is saved as `config.json.pre-weather-aggregate.bak`.

**Security:** the configuration, including any Weather Underground API key and MQTT passwords, is readable through the web UI and `/api/config`, which have no authentication. Keep `server_interface` on a trusted network.

## ASCOM Alpaca API

```
GET  /api/v1/observingconditions/{device}/{property}           e.g. temperature, skyquality, cloudcover
GET  /api/v1/observingconditions/{device}/sensordescription?SensorName={name}
GET  /api/v1/observingconditions/{device}/timesincelastupdate?SensorName={name}
PUT  /api/v1/observingconditions/{device}/refresh
GET  /management/v1/configureddevices
```

`SensorDescription` names the source a property comes from and its fallbacks. `TimeSinceLastUpdate` reports the age of the reading actually being served.

## Setup Notes

- **Tempest:** the hub broadcasts on UDP port 50222. Only one program on a machine can listen on that port.
- **Ecowitt:** give the gateway a fixed address (DHCP reservation or DNS name). No setup is needed on the gateway itself.
- **Alpaca:** enter the host, port (default 11111) and the remote device number, then click **Detect Properties**. Properties whose device gives no sensor description start unticked; check the value is real before using them.

## Conformance

Version 0.4.0 passes ASCOM ConformU 4.5.0 device and Alpaca protocol checks for the SafetyMonitor and all ObservingConditions devices, with no errors or issues.

## Sources

- [ASCOM Alpaca ObservingConditions API](https://ascom-standards.org/alpyca/alpaca.observingconditions.html)
- [Tempest UDP API Documentation](https://weatherflow.github.io/Tempest/api/udp.html)
- [ASCOM Conform Universal](https://github.com/ASCOMInitiative/ConformU)
