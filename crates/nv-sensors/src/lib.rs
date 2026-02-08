//! Device state monitoring: notifications, location, connectivity.
//!
//! Monitors device sensors and events to provide context to the agent.

/// Sensor monitor.
///
/// Watches device state (notifications, location, etc.) and publishes
/// events to the event bus.
pub struct SensorMonitor {
    // Placeholder - will be implemented in Horizon 1 Phase 4
}

impl SensorMonitor {
    /// Create a new sensor monitor.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SensorMonitor {
    fn default() -> Self {
        Self::new()
    }
}
