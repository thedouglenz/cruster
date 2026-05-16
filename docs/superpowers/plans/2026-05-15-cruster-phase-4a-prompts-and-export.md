# Cruster Phase 4A: Prompt Actions + Diagnostic Export

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the two Pro-tier human-in-the-loop features that
bridge the TUI to the user's coding agent:
1. **Prompt actions** — keybound clipboard prompts. Tera templates
   in `~/.config/cruster/prompts/` (plus shipped defaults). Press a
   chord → context gets gathered → templated prompt copies to
   system clipboard. Paste into Claude Code / Cursor / etc.
2. **Diagnostic export** — `E d` (TUI) and `cruster export pod/foo`
   (CLI) produce a structured `.md` file with resource identity,
   owner chain, events, log tail, related resources. For
   incident tickets, postmortems, async agent handoffs.

Both are **Pro-gated**: shipped templates work in free tier, but
running them prompts the upgrade flow. (Tier is hardcoded Free in
v1; Phase 5 wires real license.)

This is sub-phase 1 of Phase 4. Sub-phase 4B follows: agentskills.io
skills + Claude Code plugin.

**Architecture:**
- `cruster-core` gains a `context.rs` module: `Snapshot` struct with
  resource, cluster, events, logs (tail), selection, focused-pane
  fields. Lazy: only the fields the template references get
  populated.
- New `cruster-tui::prompts` module: TOML loader (analogous to
  `workflows.rs`), Tera template engine, key-chord registration.
- App state: `prompts: Vec<Prompt>` loaded at startup, key-chord
  map for prompt actions.
- New `cruster-tui::export` module: builds a markdown document
  from a `Snapshot`.
- `cruster-cli` gains `cruster export` verb.

**Tech additions:**
- `tera` 1.x for template rendering.

---

## Task 1: Context snapshot in cruster-core

The shared snapshot type both prompts and export consume.

- [ ] Create `app/crates/cruster-core/src/context.rs`:

```rust
//! Context snapshot for prompts + diagnostic export.

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
    pub type_: String,
    pub reason: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionContext {
    pub lines: Vec<String>,
}
```

- [ ] Re-export from lib.rs. Test + commit.

---

## Task 2: Prompts loader + Tera engine

- [ ] Add `tera = "1"` to workspace deps + `cruster-tui` deps.
- [ ] Create `app/crates/cruster-tui/src/prompts.rs`:

```rust
//! Saved prompt actions. TOML files in
//! ~/.config/cruster/prompts/, plus shipped defaults.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use cruster_core::context::Snapshot;
use serde::Deserialize;
use tera::{Context, Tera};

#[derive(Debug, Clone, Deserialize)]
pub struct PromptDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Key chord — single char for now (`d`), or `letter+letter` for
    /// a leader chord. v1: just single char as the trigger after the
    /// leader `P`.
    pub key: String,
    pub template: String,
}

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
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(p) = toml::from_str::<PromptDef>(&body) {
                out.push(p);
            }
        }
    }
    out
}

/// Render a template against a snapshot.
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
    use cruster_core::context::{ClusterContext, ResourceContext};
    use cruster_core::ResourceKey;

    #[test]
    fn render_substitutes_resource_name() {
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
                raw_yaml: String::new(),
            }),
            ..Default::default()
        };
        let out = render("Pod is {{ resource.key.name }}", &snap).unwrap();
        assert_eq!(out, "Pod is nginx");
    }

    #[test]
    fn shipped_defaults_loadable() {
        let prompts = shipped_defaults();
        assert!(!prompts.is_empty());
        for p in &prompts {
            assert!(!p.template.is_empty(), "empty template for {}", p.name);
        }
    }
}
```

- [ ] Create three shipped template files (`prompts/diagnose.tera`,
  etc.) under `app/crates/cruster-tui/prompts/`. Each is a short
  Tera template using the snapshot fields.
- [ ] Test + commit.

---

## Task 3: Wire prompts into App (leader `P` + char)

- [ ] Add `prompts: Vec<PromptDef>` + `pending_prompt_key: Option<char>`
  to App. On startup: `prompts = prompts::load_all()`.
- [ ] Add semantic action `OpenPromptLeader` to `SemanticAction`.
  Bind `P` (capital, no modifier — clashes with no existing
  bindings) in the keymap.
- [ ] On `OpenPromptLeader`, set `pending_prompt_key = Some(true_marker)`.
  Next keystroke is consumed as the prompt's key char. If it matches
  a prompt's `key`, render + copy + toast. Else "no prompt bound to
  '<char>'".
- [ ] Gate: if `!self.tier.has_pro()`, show "Pro feature" toast.
- [ ] Building the Snapshot: pull from `self.context`,
  `self.current_view.selected_key()` + recent events (filter
  registry.events by involved_object), recent logs (if Pod and
  logs_pane is open, copy lines; else fetch is too async — leave as
  empty for v1, or restrict prompts to non-logs templates).
- [ ] Clipboard via existing `crate::kubectl::copy_to_clipboard`.
- [ ] Test + commit.

---

## Task 4: Diagnostic export — `E d` TUI keybind

- [ ] Add semantic action `ExportDiagnostic` + bind to `E` (capital).
- [ ] Add `export.rs` module that builds the markdown:

```rust
pub fn build_markdown(snapshot: &cruster_core::context::Snapshot) -> String {
    let mut s = String::new();
    if let Some(r) = &snapshot.resource {
        s.push_str(&format!(
            "# Diagnostic: {}/{}/{}\n",
            r.key.kind,
            r.key.namespace.as_deref().unwrap_or("-"),
            r.key.name
        ));
        s.push_str(&format!("**Cluster:** {}\n", snapshot.cluster.name));
        s.push_str(&format!("**Status:** {}\n", r.status_summary));
        s.push_str(&format!("**Age:** {}\n\n", r.age));
    }
    if !snapshot.events.is_empty() {
        s.push_str("## Recent events\n\n| Time | Type | Reason | Message |\n|---|---|---|---|\n");
        for e in &snapshot.events {
            s.push_str(&format!("| {} | {} | {} | {} |\n", e.time, e.type_, e.reason, e.message));
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
```

- [ ] On `ExportDiagnostic`: build snapshot, render markdown, write
  to `./<kind>-<name>-<timestamp>.md`. Toast the path.
- [ ] Gate: Pro feature.
- [ ] Test + commit.

---

## Task 5: `cruster export` CLI verb

- [ ] `cruster export <kind>/<name> -n <ns>` fetches the resource via
  kube, builds a snapshot, runs `build_markdown`, writes to stdout
  (default) or `-o <file>`.
- [ ] Add to args, dispatch, schema. Tests not needed beyond build
  (the export library is tested in tui).
- [ ] Commit.

---

## Task 6: Phase 4A exit verification

- [ ] fmt + clippy + test
- [ ] Manual k3d:
  - `P d` on a pod (Free tier → "Pro feature" toast)
  - `E d` on a pod (Free → "Pro feature")
  - `cruster export pod/nginx -n default` produces a sensible .md
- [ ] Update READMEs
- [ ] Tag `phase-4a-prompts-export`

When 4A ships, 4B (skills + plugin) follows.
