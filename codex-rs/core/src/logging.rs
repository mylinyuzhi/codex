//! Custom logging utilities for configurable tracing behavior

use crate::config::types::TimezoneConfig;
use std::fmt;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

/// A configurable timer that supports both local and UTC timezones.
///
/// This avoids the type system issue of having different timer types in match arms
/// by using a single type with runtime configuration.
#[derive(Debug, Clone)]
pub struct ConfigurableTimer {
    timezone: TimezoneConfig,
}

impl ConfigurableTimer {
    pub fn new(timezone: TimezoneConfig) -> Self {
        Self { timezone }
    }
}

impl FormatTime for ConfigurableTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        match self.timezone {
            TimezoneConfig::Local => {
                // Use chrono local timezone
                let now = chrono::Local::now();
                write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S%.3f"))
            }
            TimezoneConfig::Utc => {
                // Use chrono UTC timezone
                let now = chrono::Utc::now();
                write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S%.3fZ"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configurable_timer_creation() {
        let local_timer = ConfigurableTimer::new(TimezoneConfig::Local);
        let utc_timer = ConfigurableTimer::new(TimezoneConfig::Utc);

        // Just ensure they can be created without panic
        assert!(matches!(local_timer.timezone, TimezoneConfig::Local));
        assert!(matches!(utc_timer.timezone, TimezoneConfig::Utc));
    }
}
