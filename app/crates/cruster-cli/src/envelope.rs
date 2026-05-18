//! Shared NDJSON envelope primitives for the evidence-gathering verbs
//! (`logs --grep`, `timeline`, `changed`).
//!
//! Each of those verbs streams one JSON record per fact and ends with a
//! single `summary` record. The summary always carries the same shape:
//! a time window, source counts, truncation flags, and a list of
//! probes/kinds that came up empty so the calling agent doesn't waste a
//! turn re-checking them.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Closed-open time window `[from, to)` scanned by a verb.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

impl Window {
    pub fn new(from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        Self { from, to }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_round_trips_through_json() {
        let w = Window::new(
            "2026-05-18T13:22:01Z".parse().unwrap(),
            "2026-05-18T14:22:01Z".parse().unwrap(),
        );
        let j = serde_json::to_value(&w).unwrap();
        assert_eq!(j["from"], "2026-05-18T13:22:01Z");
        assert_eq!(j["to"], "2026-05-18T14:22:01Z");
    }
}
