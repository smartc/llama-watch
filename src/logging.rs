use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveTime, Utc};
use parking_lot::RwLock;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

use crate::config::models::MonitorStatus;

const LOG_DIR: &str = "logs";
const LOG_ROTATION_HOUR: u32 = 16; // 4 PM
const LOG_RETENTION_DAYS: i64 = 7;

#[derive(Clone)]
pub struct DebugLogger {
    inner: Arc<RwLock<LoggerInner>>,
}

struct LoggerInner {
    enabled: bool,
    current_file: Option<BufWriter<File>>,
    current_file_date: Option<String>,
    last_overall_safe: Option<bool>,
    last_status_log: Option<DateTime<Utc>>,
    last_monitor_states: std::collections::HashMap<String, bool>,
}

impl DebugLogger {
    pub fn new(enabled: bool) -> Result<Self> {
        // Ensure log directory exists
        fs::create_dir_all(LOG_DIR).context("Failed to create logs directory")?;

        let logger = Self {
            inner: Arc::new(RwLock::new(LoggerInner {
                enabled,
                current_file: None,
                current_file_date: None,
                last_overall_safe: None,
                last_status_log: None,
                last_monitor_states: std::collections::HashMap::new(),
            })),
        };

        // Open initial log file if enabled
        if enabled {
            logger.ensure_log_file()?;
        }

        // Clean up old logs
        if let Err(e) = logger.cleanup_old_logs() {
            warn!("Failed to cleanup old logs: {}", e);
        }

        Ok(logger)
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.read().enabled
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        let mut inner = self.inner.write();
        inner.enabled = enabled;

        if enabled {
            drop(inner);
            self.ensure_log_file()?;
        } else {
            // Close current file when disabling
            if let Some(mut file) = inner.current_file.take() {
                let _ = file.flush();
            }
        }

        Ok(())
    }

    /// Get the current log file name based on rotation schedule
    fn get_log_filename(&self) -> String {
        let now = Local::now();
        let rotation_time = NaiveTime::from_hms_opt(LOG_ROTATION_HOUR, 0, 0).unwrap();

        // If before rotation time, use previous day's date
        let date = if now.time() < rotation_time {
            now.date_naive() - chrono::Duration::days(1)
        } else {
            now.date_naive()
        };

        format!("llama-watch-{}.log", date.format("%Y-%m-%d"))
    }

    /// Ensure we have the correct log file open (handles rotation)
    fn ensure_log_file(&self) -> Result<()> {
        let filename = self.get_log_filename();
        let mut inner = self.inner.write();

        if !inner.enabled {
            return Ok(());
        }

        // Check if we need to rotate
        let needs_new_file = match &inner.current_file_date {
            Some(current) => current != &filename,
            None => true,
        };

        if needs_new_file {
            // Close existing file
            if let Some(mut file) = inner.current_file.take() {
                let _ = file.flush();
            }

            // Open new file
            let path = Path::new(LOG_DIR).join(&filename);
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .context(format!("Failed to open log file: {:?}", path))?;

            inner.current_file = Some(BufWriter::new(file));
            inner.current_file_date = Some(filename);
        }

        Ok(())
    }

    /// Clean up log files older than retention period
    fn cleanup_old_logs(&self) -> Result<()> {
        let log_dir = Path::new(LOG_DIR);
        if !log_dir.exists() {
            return Ok(());
        }

        let cutoff = Local::now() - chrono::Duration::days(LOG_RETENTION_DAYS);

        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(ext) = path.extension() {
                if ext == "log" {
                    // Try to parse date from filename
                    if let Some(stem) = path.file_stem() {
                        let stem = stem.to_string_lossy();
                        if stem.starts_with("llama-watch-") {
                            let date_str = stem.strip_prefix("llama-watch-").unwrap_or("");
                            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                                let file_datetime = date.and_hms_opt(LOG_ROTATION_HOUR, 0, 0)
                                    .unwrap()
                                    .and_local_timezone(Local)
                                    .unwrap();

                                if file_datetime < cutoff {
                                    if let Err(e) = fs::remove_file(&path) {
                                        warn!("Failed to remove old log file {:?}: {}", path, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Write a message to the log
    fn write_log(&self, message: &str) -> Result<()> {
        self.ensure_log_file()?;

        let mut inner = self.inner.write();
        if !inner.enabled {
            return Ok(());
        }

        if let Some(ref mut file) = inner.current_file {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(file, "[{}] {}", timestamp, message)?;
            file.flush()?;
        }

        Ok(())
    }

    /// Log a monitor state change
    pub fn log_monitor_state_change(
        &self,
        monitor_name: &str,
        monitor_id: &str,
        old_safe: bool,
        new_safe: bool,
        value: Option<f64>,
        threshold: f64,
        raw_payload: Option<&str>,
    ) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let state_change = if old_safe && !new_safe {
            "SAFE -> UNSAFE"
        } else if !old_safe && new_safe {
            "UNSAFE -> SAFE"
        } else {
            return Ok(()); // No actual state change
        };

        let value_str = match value {
            Some(v) => format!("{:.4}", v),
            None => "N/A".to_string(),
        };

        let message = format!(
            "MONITOR STATE CHANGE: {} [{}] {} | value={}, threshold={:.4}{}",
            monitor_name,
            monitor_id,
            state_change,
            value_str,
            threshold,
            raw_payload.map(|p| format!(", raw={}", p)).unwrap_or_default()
        );

        self.write_log(&message)
    }

    /// Log overall system state change
    pub fn log_overall_state_change(&self, old_safe: bool, new_safe: bool, statuses: &[MonitorStatus]) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let state_change = if old_safe && !new_safe {
            "SAFE -> UNSAFE"
        } else if !old_safe && new_safe {
            "UNSAFE -> SAFE"
        } else {
            return Ok(());
        };

        self.write_log(&format!("=== OVERALL STATE CHANGE: {} ===", state_change))?;

        // Log each monitor's current state
        for status in statuses {
            let value_str = match status.current_value {
                Some(v) => format!("{:.4}", v),
                None => "N/A".to_string(),
            };

            let state_str = if !status.enabled {
                "DISABLED"
            } else if status.is_safe {
                "SAFE"
            } else {
                "UNSAFE"
            };

            let error_str = status.error.as_ref()
                .map(|e| format!(", error={}", e))
                .unwrap_or_default();

            let raw_str = status.raw_payload.as_ref()
                .map(|p| format!(", raw={}", p))
                .unwrap_or_default();

            self.write_log(&format!(
                "  {} [{}]: {} | value={}, threshold={:.4}{}{}",
                status.name,
                status.id,
                state_str,
                value_str,
                status.threshold,
                error_str,
                raw_str
            ))?;
        }

        self.write_log("=== END STATE CHANGE ===")?;

        Ok(())
    }

    /// Log periodic status (every 5 minutes if no state change)
    pub fn log_periodic_status(&self, is_safe: bool, statuses: &[MonitorStatus]) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let now = Utc::now();
        let mut inner = self.inner.write();

        // Check if 5 minutes have passed since last status log
        if let Some(last_log) = inner.last_status_log {
            let elapsed = now.signed_duration_since(last_log);
            if elapsed.num_seconds() < 300 {
                return Ok(());
            }
        }

        inner.last_status_log = Some(now);
        drop(inner);

        let overall_state = if is_safe { "SAFE" } else { "UNSAFE" };

        self.write_log(&format!("--- PERIODIC STATUS (Overall: {}) ---", overall_state))?;

        for status in statuses {
            let value_str = match status.current_value {
                Some(v) => format!("{:.4}", v),
                None => "N/A".to_string(),
            };

            let state_str = if !status.enabled {
                "DISABLED"
            } else if status.is_safe {
                "SAFE"
            } else {
                "UNSAFE"
            };

            let last_update_str = match status.last_update {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                None => "Never".to_string(),
            };

            let error_str = status.error.as_ref()
                .map(|e| format!(", error={}", e))
                .unwrap_or_default();

            let raw_str = status.raw_payload.as_ref()
                .map(|p| format!(", raw={}", p))
                .unwrap_or_default();

            let pending_str = match (status.pending_is_safe, status.pending_since) {
                (Some(pending), Some(since)) => {
                    format!(", pending={} since {}", pending, since.format("%H:%M:%S UTC"))
                }
                _ => String::new(),
            };

            self.write_log(&format!(
                "  {} [{}] ({}): {} | enabled={}, value={}, threshold={:.4}, last_update={}{}{}{}",
                status.name,
                status.id,
                status.monitor_type,
                state_str,
                status.enabled,
                value_str,
                status.threshold,
                last_update_str,
                pending_str,
                error_str,
                raw_str
            ))?;
        }

        self.write_log("--- END PERIODIC STATUS ---")?;

        Ok(())
    }

    /// Check and log individual monitor state changes
    pub fn check_monitor_state_changes(&self, statuses: &[MonitorStatus]) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        let mut inner = self.inner.write();

        for status in statuses {
            // Only track enabled monitors
            if !status.enabled {
                continue;
            }

            let current_safe = status.is_safe;
            let old_safe = inner.last_monitor_states.get(&status.id).copied();

            if old_safe != Some(current_safe) {
                // State changed - log it
                inner.last_monitor_states.insert(status.id.clone(), current_safe);

                // Need to drop the lock to call write_log
                let status_clone = status.clone();
                drop(inner);

                if let Some(was_safe) = old_safe {
                    // Log the state change
                    self.log_monitor_state_change(
                        &status_clone.name,
                        &status_clone.id,
                        was_safe,
                        current_safe,
                        status_clone.current_value,
                        status_clone.threshold,
                        status_clone.raw_payload.as_deref(),
                    )?;
                }

                // Re-acquire the lock for the next iteration
                inner = self.inner.write();
            }
        }

        Ok(())
    }

    /// Check and log overall state change if needed
    pub fn check_overall_state_change(&self, current_safe: bool, statuses: &[MonitorStatus]) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        // First check individual monitor state changes
        self.check_monitor_state_changes(statuses)?;

        let mut inner = self.inner.write();
        let old_safe = inner.last_overall_safe;

        if old_safe != Some(current_safe) {
            inner.last_overall_safe = Some(current_safe);
            drop(inner);

            if let Some(was_safe) = old_safe {
                self.log_overall_state_change(was_safe, current_safe, statuses)?;
            } else {
                // First time - log initial state
                let state = if current_safe { "SAFE" } else { "UNSAFE" };
                self.write_log(&format!("=== INITIAL STATE: {} ===", state))?;
                for status in statuses {
                    let value_str = match status.current_value {
                        Some(v) => format!("{:.4}", v),
                        None => "N/A".to_string(),
                    };
                    let state_str = if !status.enabled {
                        "DISABLED"
                    } else if status.is_safe {
                        "SAFE"
                    } else {
                        "UNSAFE"
                    };
                    self.write_log(&format!(
                        "  {} [{}]: {} | value={}, threshold={:.4}",
                        status.name,
                        status.id,
                        state_str,
                        value_str,
                        status.threshold
                    ))?;

                    // Initialize the state tracking for this monitor
                    let mut inner = self.inner.write();
                    if status.enabled {
                        inner.last_monitor_states.insert(status.id.clone(), status.is_safe);
                    }
                }
                self.write_log("=== END INITIAL STATE ===")?;
            }
        }

        Ok(())
    }

    /// Get list of available log files
    pub fn get_log_files(&self) -> Result<Vec<LogFileInfo>> {
        let log_dir = Path::new(LOG_DIR);
        if !log_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();

        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(ext) = path.extension() {
                if ext == "log" {
                    if let Ok(metadata) = entry.metadata() {
                        let filename = path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();

                        files.push(LogFileInfo {
                            filename,
                            size: metadata.len(),
                            modified: metadata.modified().ok().map(|t| {
                                let dt: DateTime<Utc> = t.into();
                                dt
                            }),
                        });
                    }
                }
            }
        }

        // Sort by filename descending (newest first)
        files.sort_by(|a, b| b.filename.cmp(&a.filename));

        Ok(files)
    }

    /// Get path to a specific log file
    pub fn get_log_file_path(&self, filename: &str) -> Option<PathBuf> {
        // Validate filename to prevent directory traversal
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            return None;
        }

        if !filename.starts_with("llama-watch-") || !filename.ends_with(".log") {
            return None;
        }

        let path = Path::new(LOG_DIR).join(filename);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Get the current log file path
    pub fn get_current_log_path(&self) -> PathBuf {
        Path::new(LOG_DIR).join(self.get_log_filename())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogFileInfo {
    pub filename: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

pub type SharedDebugLogger = Arc<DebugLogger>;
