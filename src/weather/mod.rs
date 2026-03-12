// TODO: Implement WeatherUnderground API integration as an alternative weather data source.
// The configuration structure already supports WeatherUnderground in WeatherDataSource enum
// (see config/models.rs). Implementation would involve:
// - Creating a weather_underground.rs module with API client
// - Handling station_id and api_key from WeatherDeviceConfig
// - Mapping WU API response fields to TempestObservation format (or a common WeatherObservation)
// - Adding periodic polling similar to how Alpaca monitors work

pub mod tempest;
pub mod observing_conditions;
pub mod weather_monitor;

pub use tempest::TempestListener;
pub use observing_conditions::{ObservingConditionsDevice, WeatherDataAccessor};
pub use weather_monitor::{WeatherMonitor, WeatherMonitorManager};
