//! Context snapshot consumed by prompt actions and diagnostic export.
//!
//! A `Snapshot` is the union of everything a Tera template or markdown
//! export might want: cluster identity, the currently focused
//! resource, recent events, recent logs, and any selected text. It
//! lives in `cruster-core` because both `cruster-tui` (interactive)
//! and `cruster-cli` (`cruster export`) populate and consume it.

use serde::Serialize;

use crate::ResourceKey;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub cluster: ClusterContext,
    pub resource: Option<ResourceContext>,
    pub events: Vec<EventSummary>,
    pub logs: Vec<String>,
    pub selection: Option<SelectionContext>,
    pub pane: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ClusterContext {
    pub name: String,
    pub context: String,
    pub environment: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceContext {
    pub key: ResourceKey,
    pub status_summary: String,
    pub age: String,
    pub raw_yaml: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventSummary {
    pub time: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub reason: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionContext {
    pub lines: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snapshot_serializes_cleanly() {
        let snap = Snapshot::default();
        let v = serde_json::to_value(&snap).unwrap();
        assert!(v.get("cluster").is_some());
        assert!(v["resource"].is_null());
        assert_eq!(v["events"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn populated_snapshot_round_trips_through_json() {
        let snap = Snapshot {
            cluster: ClusterContext {
                name: "k3d-a8s-dev".into(),
                context: "k3d-a8s-dev".into(),
                environment: "local".into(),
            },
            resource: Some(ResourceContext {
                key: ResourceKey::namespaced("Pod", "default", "nginx"),
                status_summary: "Running".into(),
                age: "5m".into(),
                raw_yaml: "kind: Pod\n".into(),
            }),
            events: vec![EventSummary {
                time: "2026-05-15T12:00:00Z".into(),
                type_: "Warning".into(),
                reason: "BackOff".into(),
                message: "Back-off restarting failed container".into(),
            }],
            logs: vec!["line 1".into(), "line 2".into()],
            selection: None,
            pane: Some("view".into()),
        };
        let s = serde_json::to_string(&snap).unwrap();
        // Tera-friendly field name: events should expose `type`, not
        // `type_`, so templates can write `{{ event.type }}`.
        assert!(s.contains(r#""type":"Warning""#));
        assert!(s.contains(r#""reason":"BackOff""#));
    }
}
