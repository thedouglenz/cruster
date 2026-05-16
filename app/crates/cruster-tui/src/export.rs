//! Diagnostic export: render a `Snapshot` to markdown.
//!
//! Used by both the TUI's `E` keybind (writes `kind-name-ts.md` to
//! cwd) and the `cruster export` CLI verb (writes to stdout or `-o`).
//! Kept in `cruster-tui` because it depends on `cruster-core` types
//! and is consumed by both the TUI binary and the CLI.

use cruster_core::context::Snapshot;

/// Build a self-contained markdown diagnostic for the given snapshot.
///
/// Sections render only if their source data is non-empty, so the
/// output stays focused: a snapshot with no events skips the "Recent
/// events" header rather than emitting an empty table.
pub fn build_markdown(snapshot: &Snapshot) -> String {
    let mut s = String::new();
    if let Some(r) = &snapshot.resource {
        s.push_str(&format!(
            "# Diagnostic: {}/{}/{}\n\n",
            r.key.kind,
            r.key.namespace.as_deref().unwrap_or("-"),
            r.key.name,
        ));
        s.push_str(&format!("- **Cluster:** {}\n", snapshot.cluster.name));
        s.push_str(&format!(
            "- **Environment:** {}\n",
            snapshot.cluster.environment
        ));
        if !r.status_summary.is_empty() {
            s.push_str(&format!("- **Status:** {}\n", r.status_summary));
        }
        if !r.age.is_empty() {
            s.push_str(&format!("- **Age:** {}\n", r.age));
        }
        s.push('\n');
        if !r.raw_yaml.is_empty() {
            s.push_str("## Manifest\n\n```yaml\n");
            s.push_str(&r.raw_yaml);
            if !r.raw_yaml.ends_with('\n') {
                s.push('\n');
            }
            s.push_str("```\n\n");
        }
    }
    if !snapshot.events.is_empty() {
        s.push_str("## Recent events\n\n");
        s.push_str("| Time | Type | Reason | Message |\n");
        s.push_str("|---|---|---|---|\n");
        for e in &snapshot.events {
            // Escape pipes so messages don't break the table.
            let msg = e.message.replace('|', "\\|").replace('\n', " ");
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                e.time, e.type_, e.reason, msg
            ));
        }
        s.push('\n');
    }
    if !snapshot.logs.is_empty() {
        s.push_str("## Recent logs\n\n```\n");
        for line in &snapshot.logs {
            s.push_str(line);
            s.push('\n');
        }
        s.push_str("```\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruster_core::context::{ClusterContext, EventSummary, ResourceContext};
    use cruster_core::ResourceKey;

    fn populated() -> Snapshot {
        Snapshot {
            cluster: ClusterContext {
                name: "k3d-a8s-dev".into(),
                context: "k3d-a8s-dev".into(),
                environment: "local".into(),
            },
            resource: Some(ResourceContext {
                key: ResourceKey::namespaced("Pod", "default", "nginx"),
                status_summary: "Running".into(),
                age: "5m".into(),
                raw_yaml: "kind: Pod\nmetadata:\n  name: nginx\n".into(),
            }),
            events: vec![EventSummary {
                time: "2026-05-15T12:00:00Z".into(),
                type_: "Warning".into(),
                reason: "BackOff".into(),
                message: "Back-off restarting | failed container".into(),
            }],
            logs: vec!["info: hello".into(), "error: kaboom".into()],
            selection: None,
            pane: None,
        }
    }

    #[test]
    fn renders_header_with_kind_ns_name() {
        let md = build_markdown(&populated());
        assert!(md.starts_with("# Diagnostic: Pod/default/nginx"));
    }

    #[test]
    fn empty_snapshot_renders_nothing() {
        let md = build_markdown(&Snapshot::default());
        assert!(md.is_empty(), "got: {md}");
    }

    #[test]
    fn events_section_escapes_pipes_in_messages() {
        let md = build_markdown(&populated());
        assert!(md.contains("Back-off restarting \\| failed container"));
    }

    #[test]
    fn logs_section_renders_as_fenced_block() {
        let md = build_markdown(&populated());
        assert!(md.contains("## Recent logs\n\n```\ninfo: hello\nerror: kaboom\n```\n"));
    }

    #[test]
    fn cluster_resource_only_omits_events_and_logs_sections() {
        let mut snap = populated();
        snap.events.clear();
        snap.logs.clear();
        let md = build_markdown(&snap);
        assert!(!md.contains("## Recent events"));
        assert!(!md.contains("## Recent logs"));
        assert!(md.contains("## Manifest"));
    }
}
