//! Named multi-pane layouts.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Single pane: just the current view.
    #[default]
    Single,
    /// Top half: view. Bottom half: describe pane (auto-opened on
    /// selection).
    Triplet,
    /// Top: events. Middle: list. Bottom: logs of selected.
    Incident,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Triplet => "triplet",
            Self::Incident => "incident",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_single() {
        assert_eq!(Layout::default(), Layout::Single);
    }

    #[test]
    fn labels() {
        assert_eq!(Layout::Single.label(), "single");
        assert_eq!(Layout::Triplet.label(), "triplet");
        assert_eq!(Layout::Incident.label(), "incident");
    }
}
