use std::collections::HashMap;
use tokio::task::JoinHandle;

use crate::config::models::{WeatherDataSource, WeatherDeviceConfig};
use super::observation::SharedWeatherStore;
use super::{alpaca, ecowitt, weather_underground};

/// Runs one polling task per enabled polled weather source (Ecowitt, Alpaca, Weather Underground)
#[derive(Default)]
pub struct SourcePollers {
    tasks: HashMap<u32, (WeatherDataSource, JoinHandle<()>)>,
}

impl SourcePollers {
    /// Start, stop or restart polling tasks to match the configured devices
    pub fn sync(&mut self, devices: &HashMap<u32, WeatherDeviceConfig>, store: &SharedWeatherStore) {
        let wanted: HashMap<u32, &WeatherDataSource> = devices.iter()
            .filter(|(_, d)| d.enabled && is_polled(&d.source))
            .map(|(n, d)| (*n, &d.source))
            .collect();

        self.tasks.retain(|number, (source, task)| {
            let keep = wanted.get(number).is_some_and(|w| *w == source);
            if !keep {
                task.abort();
            }
            keep
        });

        for (number, source) in wanted {
            if self.tasks.contains_key(&number) {
                continue;
            }
            let task = match source {
                WeatherDataSource::Ecowitt { host, poll_interval_seconds } => tokio::spawn(
                    ecowitt::poll_gateway(host.clone(), *poll_interval_seconds, store.clone()),
                ),
                WeatherDataSource::Alpaca { host, port, device_number, poll_interval_seconds, properties } => tokio::spawn(
                    alpaca::poll_device(host.clone(), *port, *device_number, properties.clone(), *poll_interval_seconds, store.clone()),
                ),
                WeatherDataSource::WeatherUnderground { station_id, api_key, poll_interval_seconds } => tokio::spawn(
                    weather_underground::poll_station(station_id.clone(), api_key.clone(), *poll_interval_seconds, store.clone()),
                ),
                _ => continue,
            };
            self.tasks.insert(number, (source.clone(), task));
        }
    }
}

fn is_polled(source: &WeatherDataSource) -> bool {
    matches!(
        source,
        WeatherDataSource::Ecowitt { .. } | WeatherDataSource::Alpaca { .. } | WeatherDataSource::WeatherUnderground { .. }
    )
}
