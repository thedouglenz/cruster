//! Subscription tier: gates Pro features. Phase 5 wires this to a
//! real license file; for Phase 3C it's hardcoded `Free` everywhere.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Free,
    Pro,
    Team,
    Enterprise,
}

impl Tier {
    /// Whether this tier unlocks features marked "Pro+".
    pub fn has_pro(self) -> bool {
        !matches!(self, Self::Free)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_default() {
        assert_eq!(Tier::default(), Tier::Free);
        assert!(!Tier::Free.has_pro());
    }

    #[test]
    fn paid_tiers_have_pro() {
        assert!(Tier::Pro.has_pro());
        assert!(Tier::Team.has_pro());
        assert!(Tier::Enterprise.has_pro());
    }
}
