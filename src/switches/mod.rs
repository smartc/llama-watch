//! Switch management - aggregates multiple monitor sources into explicit switches
//!
//! This module provides the SwitchManager which evaluates switch states based on
//! monitor conditions and aggregation logic (ANY, ALL, MIN_OF).

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::models::{
    AppConfig, SwitchConfig, SwitchLogic, SwitchSourceStatus, SwitchStatus,
};
use crate::monitors::MonitorState;

/// Internal state for tracking a single switch
#[derive(Debug, Clone)]
struct SwitchInternalState {
    config: SwitchConfig,
    /// Current triggered state
    is_triggered: bool,
    /// Pending state during hold time
    pending_is_triggered: Option<bool>,
    pending_since: Option<DateTime<Utc>>,
    /// Last evaluation time
    last_update: Option<DateTime<Utc>>,
    /// Error message if any
    error: Option<String>,
}

impl SwitchInternalState {
    fn new(config: SwitchConfig) -> Self {
        Self {
            config,
            is_triggered: false,
            pending_is_triggered: None,
            pending_since: None,
            last_update: None,
            error: None,
        }
    }
}

/// Manages aggregated switches that combine multiple monitor sources
pub struct SwitchManager {
    switches: HashMap<String, SwitchInternalState>,
    monitor_state: Arc<RwLock<MonitorState>>,
}

impl SwitchManager {
    /// Create a new SwitchManager
    pub fn new(monitor_state: Arc<RwLock<MonitorState>>) -> Self {
        Self {
            switches: HashMap::new(),
            monitor_state,
        }
    }

    /// Initialize switches from configuration
    pub fn initialize(&mut self, config: &AppConfig) {
        self.switches.clear();

        for (id, switch_config) in &config.switches {
            if switch_config.enabled {
                info!(
                    "Initializing switch '{}' with {} sources, logic: {:?}",
                    id,
                    switch_config.sources.len(),
                    switch_config.logic
                );
                self.switches
                    .insert(id.clone(), SwitchInternalState::new(switch_config.clone()));
            }
        }

        info!("Initialized {} switches", self.switches.len());
    }

    /// Reload switches from new configuration, preserving state where possible
    pub fn reload(&mut self, config: &AppConfig) {
        // Save current states
        let old_states: HashMap<String, (bool, Option<bool>, Option<DateTime<Utc>>)> = self
            .switches
            .iter()
            .map(|(id, state)| {
                (
                    id.clone(),
                    (
                        state.is_triggered,
                        state.pending_is_triggered,
                        state.pending_since,
                    ),
                )
            })
            .collect();

        // Re-initialize
        self.switches.clear();

        for (id, switch_config) in &config.switches {
            if switch_config.enabled {
                let mut state = SwitchInternalState::new(switch_config.clone());

                // Restore previous state if this switch existed before
                if let Some((is_triggered, pending, pending_since)) = old_states.get(id) {
                    state.is_triggered = *is_triggered;
                    state.pending_is_triggered = *pending;
                    state.pending_since = *pending_since;
                }

                self.switches.insert(id.clone(), state);
            }
        }

        info!("Reloaded {} switches", self.switches.len());
    }

    /// Evaluate all switches and update their states
    pub async fn evaluate_all(&mut self) -> Vec<SwitchStatus> {
        let monitor_state = self.monitor_state.read().await;
        let all_statuses = monitor_state.get_all_statuses();
        let now = Utc::now();

        let mut results = Vec::new();

        for (id, switch_state) in &mut self.switches {
            let status = Self::evaluate_switch(switch_state, &all_statuses, now);
            results.push(status);
        }

        results
    }

    /// Evaluate a single switch and update its state
    fn evaluate_switch(
        switch_state: &mut SwitchInternalState,
        all_statuses: &[crate::config::models::MonitorStatus],
        now: DateTime<Utc>,
    ) -> SwitchStatus {
        let config = &switch_state.config;
        let mut source_states = Vec::new();
        let mut source_triggered_values = Vec::new();
        let mut errors = Vec::new();

        // Gather state from each source
        for source_id in &config.sources {
            if let Some(monitor_status) = all_statuses.iter().find(|s| &s.id == source_id) {
                // Check if the monitor has timed out
                let is_timed_out = if let Some(last_update) = monitor_status.last_update {
                    let elapsed = (now - last_update).num_seconds() as u64;
                    elapsed > config.timeout_seconds
                } else {
                    true // No data yet = timed out
                };

                // A source is "triggered" when it's in an unsafe condition
                // We use the raw condition evaluation, not the safety state
                // (since the monitor might not be included in safety but still useful for switches)
                let is_triggered = if is_timed_out {
                    errors.push(format!("Source '{}' timed out", source_id));
                    true // Timed out sources are considered triggered (unsafe)
                } else if !monitor_status.enabled {
                    false // Disabled sources don't contribute
                } else {
                    // Use the inverse of is_safe - if the monitor is unsafe, the condition is triggered
                    !monitor_status.is_safe
                };

                source_states.push(SwitchSourceStatus {
                    monitor_id: source_id.clone(),
                    monitor_name: monitor_status.name.clone(),
                    is_triggered,
                    current_value: monitor_status.current_value,
                    threshold: monitor_status.threshold,
                    last_update: monitor_status.last_update,
                    error: if is_timed_out {
                        Some("Timed out".to_string())
                    } else {
                        monitor_status.error.clone()
                    },
                });

                if monitor_status.enabled {
                    source_triggered_values.push(is_triggered);
                }
            } else {
                // Source not found
                errors.push(format!("Source '{}' not found", source_id));
                source_states.push(SwitchSourceStatus {
                    monitor_id: source_id.clone(),
                    monitor_name: source_id.clone(),
                    is_triggered: true, // Missing sources are considered triggered
                    current_value: None,
                    threshold: 0.0,
                    last_update: None,
                    error: Some("Monitor not found".to_string()),
                });
                source_triggered_values.push(true);
            }
        }

        // Apply the aggregation logic
        let new_triggered = config.logic.evaluate(&source_triggered_values);

        // Handle hold time for state transitions
        if new_triggered != switch_state.is_triggered {
            // State wants to change
            if switch_state.pending_is_triggered == Some(new_triggered) {
                // Already pending this change, check if hold time has elapsed
                if let Some(pending_since) = switch_state.pending_since {
                    let elapsed = (now - pending_since).num_seconds() as u64;
                    if elapsed >= config.hold_time_seconds {
                        // Hold time elapsed, commit the change
                        debug!(
                            "Switch '{}' state change committed: {} -> {}",
                            config.id, switch_state.is_triggered, new_triggered
                        );
                        switch_state.is_triggered = new_triggered;
                        switch_state.pending_is_triggered = None;
                        switch_state.pending_since = None;
                    }
                }
            } else {
                // Start pending the new state
                if config.hold_time_seconds > 0 {
                    debug!(
                        "Switch '{}' state change pending: {} -> {} (hold time: {}s)",
                        config.id, switch_state.is_triggered, new_triggered, config.hold_time_seconds
                    );
                    switch_state.pending_is_triggered = Some(new_triggered);
                    switch_state.pending_since = Some(now);
                } else {
                    // No hold time, change immediately
                    switch_state.is_triggered = new_triggered;
                }
            }
        } else {
            // State matches, clear any pending change
            switch_state.pending_is_triggered = None;
            switch_state.pending_since = None;
        }

        switch_state.last_update = Some(now);
        switch_state.error = if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        };

        SwitchStatus {
            id: config.id.clone(),
            name: config.name.clone(),
            description: config.description.clone(),
            is_triggered: switch_state.is_triggered,
            logic: config.logic.clone(),
            source_states,
            pending_is_triggered: switch_state.pending_is_triggered,
            pending_since: switch_state.pending_since,
            hold_time_seconds: config.hold_time_seconds,
            error: switch_state.error.clone(),
            enabled: config.enabled,
            last_update: switch_state.last_update,
        }
    }

    /// Get the count of configured switches
    pub fn get_switch_count(&self) -> usize {
        self.switches.len()
    }

    /// Get a switch status by index (for ASCOM Switch interface)
    pub async fn get_switch_by_index(&mut self, index: usize) -> Option<SwitchStatus> {
        let monitor_state = self.monitor_state.read().await;
        let all_statuses = monitor_state.get_all_statuses();
        let now = Utc::now();

        let switch_ids: Vec<String> = self.switches.keys().cloned().collect();
        if index >= switch_ids.len() {
            return None;
        }

        let id = &switch_ids[index];
        if let Some(switch_state) = self.switches.get_mut(id) {
            Some(Self::evaluate_switch(switch_state, &all_statuses, now))
        } else {
            None
        }
    }

    /// Get all switch statuses
    pub async fn get_all_statuses(&mut self) -> Vec<SwitchStatus> {
        self.evaluate_all().await
    }

    /// Get switch IDs in order (for consistent ASCOM indexing)
    pub fn get_switch_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.switches.keys().cloned().collect();
        ids.sort(); // Consistent ordering
        ids
    }
}

pub type SharedSwitchManager = Arc<RwLock<SwitchManager>>;
