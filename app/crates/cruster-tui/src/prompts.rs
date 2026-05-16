//! Saved prompt actions.
//!
//! A prompt is a Tera template that takes a `Snapshot` and produces a
//! string. The user binds prompts to a key chord (`P` then a single
//! char), and triggering a chord renders the template and copies the
//! result to the system clipboard. Designed for agentic workflows:
//! "I see a sus pod → press P d → paste into Claude Code".
//!
//! Templates live in two places:
//! - **Shipped defaults**: `prompts/*.tera` files bundled into the
//!   binary via `include_str!`. Always available.
//! - **User overrides + additions**: `~/.config/cruster/prompts/*.toml`,
//!   loaded at startup. A TOML file with the same `name` as a shipped
//!   default replaces it; new files extend the registry.

use std::path::PathBuf;

use cruster_core::context::Snapshot;
use serde::Deserialize;
use tera::{Context, Tera};

#[derive(Debug, Clone, Deserialize)]
pub struct PromptDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Single character pressed after the leader (`P`) to trigger
    /// this prompt. e.g. `"d"` → user presses `P d`.
    pub key: String,
    pub template: String,
}

impl PromptDef {
    /// First char of `key`, used as the trigger after the leader.
    /// Returns `None` if `key` is empty.
    pub fn trigger(&self) -> Option<char> {
        self.key.chars().next()
    }
}

/// Load shipped defaults, then layer user TOML files from
/// `~/.config/cruster/prompts/`. User files with the same `name` as a
/// shipped default replace it. Files that fail to parse are silently
/// skipped — invalid TOML in one file shouldn't break startup.
pub fn load_all() -> Vec<PromptDef> {
    let mut out = shipped_defaults();
    let Some(dir) = prompts_dir() else {
        return out;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(p) = toml::from_str::<PromptDef>(&body) else {
            continue;
        };
        if let Some(existing) = out.iter_mut().find(|d| d.name == p.name) {
            *existing = p;
        } else {
            out.push(p);
        }
    }
    out
}

/// Render `template` against `snapshot`. Each top-level snapshot
/// field is exposed under its own variable name so templates can
/// write `{{ resource.key.name }}`, `{{ events }}`, etc.
pub fn render(template: &str, snapshot: &Snapshot) -> anyhow::Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("prompt", template)?;
    let mut ctx = Context::new();
    ctx.insert("cluster", &snapshot.cluster);
    ctx.insert("resource", &snapshot.resource);
    ctx.insert("events", &snapshot.events);
    ctx.insert("logs", &snapshot.logs);
    ctx.insert("selection", &snapshot.selection);
    ctx.insert("pane", &snapshot.pane);
    Ok(tera.render("prompt", &ctx)?)
}

fn shipped_defaults() -> Vec<PromptDef> {
    vec![
        PromptDef {
            name: "diagnose".into(),
            description: "Diagnose what's wrong with the current resource".into(),
            key: "d".into(),
            template: include_str!("../prompts/diagnose.tera").into(),
        },
        PromptDef {
            name: "why-failing".into(),
            description: "Why is this pod failing?".into(),
            key: "w".into(),
            template: include_str!("../prompts/why-failing.tera").into(),
        },
        PromptDef {
            name: "summarize-events".into(),
            description: "Summarize recent events".into(),
            key: "s".into(),
            template: include_str!("../prompts/summarize-events.tera").into(),
        },
    ]
}

fn prompts_dir() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("prompts");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruster_core::context::{ClusterContext, EventSummary, ResourceContext};
    use cruster_core::ResourceKey;

    fn basic_snapshot() -> Snapshot {
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
                raw_yaml: String::new(),
            }),
            events: vec![EventSummary {
                time: "12:00".into(),
                type_: "Warning".into(),
                reason: "BackOff".into(),
                message: "stuck".into(),
            }],
            logs: Vec::new(),
            selection: None,
            pane: None,
        }
    }

    #[test]
    fn render_substitutes_resource_name() {
        let snap = basic_snapshot();
        let out = render("Pod is {{ resource.key.name }}", &snap).unwrap();
        assert_eq!(out, "Pod is nginx");
    }

    #[test]
    fn render_can_iterate_events_with_type_field() {
        let snap = basic_snapshot();
        // The serde rename of `type_` -> `type` matters here: a
        // template that writes `{{ e.type }}` should work without
        // needing the user to know about the trailing underscore.
        let tmpl = "{% for e in events %}{{ e.type }}-{{ e.reason }}{% endfor %}";
        let out = render(tmpl, &snap).unwrap();
        assert_eq!(out, "Warning-BackOff");
    }

    #[test]
    fn render_returns_error_on_bad_template() {
        let snap = basic_snapshot();
        assert!(render("{{ unterminated", &snap).is_err());
    }

    #[test]
    fn shipped_defaults_all_have_unique_keys_and_nonempty_templates() {
        let prompts = shipped_defaults();
        assert!(!prompts.is_empty());
        let mut seen = std::collections::HashSet::new();
        for p in &prompts {
            assert!(!p.template.is_empty(), "empty template for {}", p.name);
            assert!(p.trigger().is_some(), "missing key char for {}", p.name);
            assert!(
                seen.insert(p.trigger().unwrap()),
                "duplicate trigger {:?} in shipped defaults",
                p.trigger(),
            );
        }
    }

    #[test]
    fn shipped_defaults_all_render_against_full_snapshot() {
        let snap = basic_snapshot();
        for p in shipped_defaults() {
            render(&p.template, &snap)
                .unwrap_or_else(|e| panic!("shipped default {} failed to render: {e}", p.name));
        }
    }
}
