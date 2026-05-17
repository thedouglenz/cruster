//! Pulse-dashboard pin storage.
//!
//! Pins live in `~/.config/cruster/dashboard.toml` and identify the
//! resources the user wants surfaced on the launch screen. The
//! `DashboardView` reads this file at construction and again whenever
//! a pin is added or removed elsewhere in the app.

use std::path::PathBuf;

use cruster_core::ResourceKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Pin {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub name: String,
}

impl Pin {
    pub fn from_key(key: &ResourceKey) -> Self {
        Self {
            kind: key.kind.clone(),
            namespace: key.namespace.clone(),
            name: key.name.clone(),
        }
    }

    pub fn to_key(&self) -> ResourceKey {
        match &self.namespace {
            Some(ns) => ResourceKey::namespaced(&self.kind, ns, &self.name),
            None => ResourceKey::cluster_scoped(&self.kind, &self.name),
        }
    }

    pub fn matches(&self, key: &ResourceKey) -> bool {
        self.kind == key.kind && self.namespace == key.namespace && self.name == key.name
    }

    /// Short human label: `kind/name` (cluster-scoped) or
    /// `kind/ns/name` (namespaced). Used in the dashboard list.
    pub fn label(&self) -> String {
        match &self.namespace {
            Some(ns) => format!("{}/{}/{}", self.kind.to_lowercase(), ns, self.name),
            None => format!("{}/{}", self.kind.to_lowercase(), self.name),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DashboardConfig {
    #[serde(default)]
    pub pins: Vec<Pin>,
}

impl DashboardConfig {
    pub fn load_or_default() -> Self {
        let Some(path) = pins_path() else {
            return Self::default();
        };
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&body).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = pins_path() else {
            anyhow::bail!("no config dir available");
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(&path, body)?;
        Ok(())
    }

    /// Append a pin if it isn't already present. Returns true if added.
    pub fn add(&mut self, pin: Pin) -> bool {
        if self.pins.contains(&pin) {
            return false;
        }
        self.pins.push(pin);
        true
    }

    pub fn remove(&mut self, idx: usize) -> Option<Pin> {
        if idx >= self.pins.len() {
            return None;
        }
        Some(self.pins.remove(idx))
    }
}

pub fn pins_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("dashboard.toml");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_roundtrip_namespaced() {
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        let pin = Pin::from_key(&key);
        assert_eq!(pin.to_key(), key);
        assert!(pin.matches(&key));
    }

    #[test]
    fn pin_roundtrip_cluster_scoped() {
        let key = ResourceKey::cluster_scoped("Node", "node-1");
        let pin = Pin::from_key(&key);
        assert_eq!(pin.to_key(), key);
        assert_eq!(pin.label(), "node/node-1");
    }

    #[test]
    fn pin_label_namespaced() {
        let pin = Pin::from_key(&ResourceKey::namespaced(
            "Deployment",
            "kube-system",
            "coredns",
        ));
        assert_eq!(pin.label(), "deployment/kube-system/coredns");
    }

    #[test]
    fn add_dedupes() {
        let mut cfg = DashboardConfig::default();
        let pin = Pin::from_key(&ResourceKey::namespaced("Pod", "default", "p"));
        assert!(cfg.add(pin.clone()));
        assert!(!cfg.add(pin));
        assert_eq!(cfg.pins.len(), 1);
    }

    #[test]
    fn remove_out_of_range_returns_none() {
        let mut cfg = DashboardConfig::default();
        cfg.add(Pin::from_key(&ResourceKey::namespaced(
            "Pod", "default", "p",
        )));
        assert!(cfg.remove(5).is_none());
        assert!(cfg.remove(0).is_some());
        assert!(cfg.pins.is_empty());
    }

    #[test]
    fn toml_roundtrip() {
        let mut cfg = DashboardConfig::default();
        cfg.add(Pin::from_key(&ResourceKey::namespaced(
            "Deployment",
            "default",
            "web",
        )));
        cfg.add(Pin::from_key(&ResourceKey::cluster_scoped("Node", "n1")));
        let body = toml::to_string_pretty(&cfg).unwrap();
        let back: DashboardConfig = toml::from_str(&body).unwrap();
        assert_eq!(back.pins, cfg.pins);
        // Cluster-scoped serialisation omits the namespace key.
        assert!(!body.contains("namespace = \"\""));
    }
}
