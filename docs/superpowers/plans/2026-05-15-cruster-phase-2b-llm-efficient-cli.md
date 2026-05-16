# Cruster Phase 2B: LLM-Efficient CLI Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every cruster operation that exists as a TUI view also
works as a one-shot CLI invocation whose output is shaped for LLM
consumption. The same `cruster` binary dispatches: no args (or
`cruster tui`) launches the TUI; any verb (`cruster get pods`,
`cruster describe pod/nginx`, etc.) runs as a CLI. CLI output is
text by default in a TTY, NDJSON when piped or with `--llm`, with
aggressive field pruning, token-budgeted truncation, and a stable
versioned schema per verb.

This is the core deliverable of **Pillar 3** in the spec — the bet
that cruster's CLI is a drop-in `kubectl` replacement that an agent
already wishes you'd given them.

**Architecture:** New `cruster-cli` crate provides verb handlers,
output formatters, schemas, and the pruning library. `cruster-bin`'s
`main` dispatches based on `argv`. Verb handlers reuse
`cruster-kube` for apiserver calls (no parallel kube client). The
existing watch-based `StoreRegistry` is bypassed for CLI mode: CLI
verbs make one-shot list/get calls and exit, which is much cheaper
for a short-lived process.

**Tech stack additions:** `clap` 4.x with derive macros for arg
parsing; `serde_yaml` (already in workspace) for `--format yaml`;
`is-terminal` 0.4 for TTY detection (`std::io::IsTerminal` works too
but the crate gives us 1.65 compatibility).

**Out of scope (Phase 4):**
- `cruster bundle`, `cruster why-pending`, `cruster why-crashloop`,
  `cruster why-no-endpoints`, `cruster what-changed`, `cruster
  diagnose`, prompt actions, diagnostic .md export
- The agentskills.io skills + Claude Code plugin
- `~/.cache/cruster/selection.json` selection bridge

Those are agent-bridging layers built on top of the CLI mode this
phase delivers.

---

## File Structure

New files this phase adds:

```
app/crates/cruster-cli/
├── Cargo.toml
└── src/
    ├── lib.rs                  # pub fn run(args) -> exit code
    ├── args.rs                 # clap definitions
    ├── format.rs               # Formatter trait, text/json/ndjson/yaml impls
    ├── prune.rs                # field-pruning library (apiserver noise removal)
    ├── budget.rs               # token-budget trimming
    ├── schemas.rs              # registry of per-verb JSON schemas (compiled in)
    ├── output.rs               # stdout writer with TTY detection
    └── verbs/
        ├── mod.rs
        ├── get.rs              # cruster get <kind> [name] [--ns] [--full] …
        ├── describe.rs         # cruster describe <kind>/<name>
        ├── logs.rs             # cruster logs <pod> [--follow] [--tail] …
        ├── events.rs           # cruster events [--ns] [--resource]
        ├── schema.rs           # cruster schema <verb>
        └── help_json.rs        # cruster help --format json
app/crates/cruster-cli/schemas/
├── get-pod.schema.json
├── get-deployment.schema.json
├── get-service.schema.json
├── get-node.schema.json
├── get-event.schema.json
├── get-configmap.schema.json
├── get-secret.schema.json
├── get-namespace.schema.json
├── describe.schema.json
├── logs.schema.json
└── events.schema.json
```

Files modified:

```
app/Cargo.toml                  # add clap, is-terminal, cruster-cli path dep
app/crates/cruster-bin/Cargo.toml  # add cruster-cli dep
app/crates/cruster-bin/src/main.rs # dispatch TUI vs CLI based on argv
```

---

## Task 1: cruster-cli crate scaffold + main dispatcher

**Files:**
- Modify: `app/Cargo.toml` (add clap + is-terminal workspace deps, add cruster-cli path)
- Create: `app/crates/cruster-cli/Cargo.toml`
- Create: `app/crates/cruster-cli/src/lib.rs`
- Modify: `app/crates/cruster-bin/Cargo.toml`
- Modify: `app/crates/cruster-bin/src/main.rs`

- [ ] **Step 1: Add workspace deps**

Modify `app/Cargo.toml`:

```toml
[workspace.dependencies]
# ... existing ...
clap = { version = "4", features = ["derive", "env"] }
is-terminal = "0.4"

# Workspace crates
cruster-core = { path = "crates/cruster-core" }
cruster-kube = { path = "crates/cruster-kube" }
cruster-tui = { path = "crates/cruster-tui" }
cruster-cli = { path = "crates/cruster-cli" }
```

Add `crates/cruster-cli` to `members`:

```toml
members = [
    "crates/cruster-core",
    "crates/cruster-kube",
    "crates/cruster-tui",
    "crates/cruster-cli",
    "crates/cruster-bin",
]
```

- [ ] **Step 2: Create the crate manifest**

Create `app/crates/cruster-cli/Cargo.toml`:

```toml
[package]
name = "cruster-cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
cruster-core = { workspace = true }
cruster-kube = { workspace = true }
anyhow = { workspace = true }
clap = { workspace = true }
futures = { workspace = true }
is-terminal = { workspace = true }
k8s-openapi = { workspace = true }
kube = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "test-util"] }
```

- [ ] **Step 3: Stub the lib**

Create `app/crates/cruster-cli/src/lib.rs`:

```rust
//! Cruster command-line interface.
//!
//! Entry point for non-TUI invocations. The `cruster` binary calls
//! `run(args)` when invoked with subcommands; with no args it stays
//! in TUI mode.

use std::ffi::OsString;

pub mod args;
pub mod budget;
pub mod format;
pub mod output;
pub mod prune;
pub mod schemas;
pub mod verbs;

/// Run cruster in CLI mode. Returns a process exit code.
///
/// `argv` is the full process argv (including argv[0]). Caller is
/// responsible for routing this only when subcommands are present.
pub async fn run<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match args::Cli::try_parse(argv) {
        Ok(c) => c,
        Err(e) => {
            // clap prints the error/help itself; preserve its exit code.
            e.exit();
        }
    };

    match verbs::dispatch(cli).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cruster: {e:#}");
            1
        }
    }
}
```

- [ ] **Step 4: Stub the modules so the crate compiles**

Create empty module stubs (each replaces a `pub mod X;` line with
real content in later tasks):

`app/crates/cruster-cli/src/args.rs`:

```rust
//! Replaced in Task 2.

use std::ffi::OsString;

pub struct Cli;

impl Cli {
    pub fn try_parse<I, T>(_argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Ok(Self)
    }
}
```

`app/crates/cruster-cli/src/format.rs`:

```rust
//! Replaced in Task 2.
```

`app/crates/cruster-cli/src/budget.rs`:

```rust
//! Replaced in Task 15.
```

`app/crates/cruster-cli/src/output.rs`:

```rust
//! Replaced in Task 2.
```

`app/crates/cruster-cli/src/prune.rs`:

```rust
//! Replaced in Task 3.
```

`app/crates/cruster-cli/src/schemas.rs`:

```rust
//! Replaced in Task 16.
```

`app/crates/cruster-cli/src/verbs/mod.rs`:

```rust
//! CLI verb handlers (one module per verb).

use crate::args::Cli;

pub async fn dispatch(_cli: Cli) -> anyhow::Result<()> {
    anyhow::bail!("no verbs registered yet (Task 4)")
}
```

- [ ] **Step 5: Wire bin dispatch**

Modify `app/crates/cruster-bin/Cargo.toml`, add to `[dependencies]`:

```toml
cruster-cli = { workspace = true }
```

Modify `app/crates/cruster-bin/src/main.rs`:

```rust
use anyhow::Context;
use cruster_cli;
use cruster_kube::{
    run_watcher, ConfigMaps, Deployments, Events, Namespaces, Nodes, Pods, ResourceKind,
    ResourceStore, Secrets, Services, StoreRegistry,
};
use cruster_tui::App;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let mut argv = std::env::args_os();
    let argv0 = argv.next();
    let first_arg = argv.next();

    // CLI mode: any argument other than nothing or `tui` dispatches to
    // the CLI. Subcommand parsing happens inside cruster-cli.
    let go_cli = match first_arg.as_deref().and_then(|s| s.to_str()) {
        None => false,           // no args → TUI
        Some("tui") => false,    // explicit TUI
        _ => true,
    };

    if go_cli {
        // Reconstruct full argv for clap.
        let mut full = vec![argv0.unwrap_or_default()];
        full.push(first_arg.unwrap());
        full.extend(argv);
        let code = cruster_cli::run(full).await;
        std::process::exit(code);
    }

    // Otherwise launch the TUI exactly as before.
    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let registry = StoreRegistry::new();

    spawn_watcher::<Pods>(client.clone(), registry.pods.clone());
    spawn_watcher::<Deployments>(client.clone(), registry.deployments.clone());
    spawn_watcher::<Services>(client.clone(), registry.services.clone());
    spawn_watcher::<Nodes>(client.clone(), registry.nodes.clone());
    spawn_watcher::<Events>(client.clone(), registry.events.clone());
    spawn_watcher::<ConfigMaps>(client.clone(), registry.configmaps.clone());
    spawn_watcher::<Secrets>(client.clone(), registry.secrets.clone());
    spawn_watcher::<Namespaces>(client.clone(), registry.namespaces.clone());

    let mut app = App::new(registry, Some(client));
    app.run().await
}

fn spawn_watcher<K: ResourceKind>(client: Client, store: ResourceStore<K::Object>) {
    tokio::spawn(async move {
        if let Err(e) = run_watcher::<K>(client, store).await {
            tracing::error!(kind = K::name(), error = %e, "watcher exited");
        }
    });
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
```

- [ ] **Step 6: Build + commit**

```bash
cd app && cargo build --workspace
cd app && cargo test --workspace
```
Expected: both pass (no new tests; existing tests stay green).

```bash
cd app && cargo clippy --workspace --all-targets -- -D warnings
```
Expected: clean (stub modules will have unused-import warnings; suppress with `#![allow(unused)]` at the top of each stub if needed).

```bash
git add app
git commit -m "feat(cli): scaffold cruster-cli crate and TUI/CLI dispatch"
```

Smoke-test from the running shell:
```bash
./target/debug/cruster nonexistent
```
Expected: exits 1 with `cruster: no verbs registered yet (Task 4)` on
stderr — confirms dispatch works.

---

## Task 2: Clap CLI definitions + Formatter trait + output mode detection

**Files:**
- Replace: `app/crates/cruster-cli/src/args.rs`
- Replace: `app/crates/cruster-cli/src/format.rs`
- Replace: `app/crates/cruster-cli/src/output.rs`

- [ ] **Step 1: Define CLI structure**

Replace `app/crates/cruster-cli/src/args.rs`:

```rust
//! Top-level CLI argument schema (clap derive).
//!
//! New verbs are added as variants of `Command`.

use std::ffi::OsString;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cruster", about = "Kubernetes TUI + CLI for humans and agents")]
pub struct Cli {
    /// Output format. Defaults to `text` in a TTY, `ndjson` otherwise.
    #[arg(long, short = 'o', global = true)]
    pub format: Option<Format>,

    /// Shorthand for `--format ndjson` plus aggressive field pruning.
    /// Auto-on when stdout is not a TTY.
    #[arg(long, global = true)]
    pub llm: bool,

    /// Disable field pruning (include managedFields, status timestamps,
    /// etc.). Off by default.
    #[arg(long, global = true)]
    pub full: bool,

    /// Hard cap on output size in approximate tokens. Trimmed at record
    /// boundaries with a `truncated: true` marker.
    #[arg(long, global = true)]
    pub budget: Option<usize>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn try_parse<I, T>(argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(argv)
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List resources of a given kind.
    Get(GetArgs),
    /// Show detailed information about a single resource.
    Describe(DescribeArgs),
    /// Print pod logs.
    Logs(LogsArgs),
    /// Print recent cluster events.
    Events(EventsArgs),
    /// Print the JSON schema of a verb's structured output.
    Schema(SchemaArgs),
    /// Print machine-readable help (same as `--help --format json`).
    #[command(name = "help-json")]
    HelpJson,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
pub enum Format {
    Text,
    Json,
    Ndjson,
    Yaml,
}

#[derive(Debug, Parser)]
pub struct GetArgs {
    /// Kind to list: pods, deployments, services, nodes, events,
    /// configmaps, secrets, namespaces. Aliases (po, deploy, svc, no,
    /// ev, cm, sec, ns) are accepted.
    pub kind: String,
    /// Optional resource name to filter to.
    pub name: Option<String>,
    /// Namespace. Defaults to all-namespaces.
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Label selector (e.g. `app=web,tier!=frontend`).
    #[arg(long, short = 'l')]
    pub selector: Option<String>,
}

#[derive(Debug, Parser)]
pub struct DescribeArgs {
    /// Resource reference: `kind/name`, e.g. `pod/nginx`.
    pub reference: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
}

#[derive(Debug, Parser)]
pub struct LogsArgs {
    /// Pod name (with optional `pod/` prefix).
    pub pod: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Container name within the pod. Required if the pod has multiple
    /// containers.
    #[arg(long, short = 'c')]
    pub container: Option<String>,
    /// Stream new log lines as they appear.
    #[arg(long, short = 'f')]
    pub follow: bool,
    /// Maximum lines to return.
    #[arg(long)]
    pub tail: Option<i64>,
    /// Only return lines newer than this duration (e.g. `5m`, `1h`).
    #[arg(long)]
    pub since: Option<String>,
    /// Only return lines containing this substring (case-sensitive).
    #[arg(long)]
    pub grep: Option<String>,
}

#[derive(Debug, Parser)]
pub struct EventsArgs {
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Filter to events involving the given resource: `kind/name`.
    #[arg(long)]
    pub resource: Option<String>,
    /// Max events to return.
    #[arg(long, default_value = "100")]
    pub limit: usize,
}

#[derive(Debug, Parser)]
pub struct SchemaArgs {
    /// Verb to print the schema for, e.g. `get-pod`, `describe`, `logs`.
    pub verb: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_pods() {
        let cli = Cli::try_parse(["cruster", "get", "pods"]).unwrap();
        let Command::Get(args) = cli.command else {
            panic!("expected Get")
        };
        assert_eq!(args.kind, "pods");
    }

    #[test]
    fn parses_logs_with_follow_and_grep() {
        let cli =
            Cli::try_parse(["cruster", "logs", "nginx", "-f", "--grep", "error"]).unwrap();
        let Command::Logs(args) = cli.command else {
            panic!("expected Logs")
        };
        assert_eq!(args.pod, "nginx");
        assert!(args.follow);
        assert_eq!(args.grep.as_deref(), Some("error"));
    }

    #[test]
    fn llm_flag_propagates() {
        let cli = Cli::try_parse(["cruster", "--llm", "get", "pods"]).unwrap();
        assert!(cli.llm);
    }

    #[test]
    fn format_explicit_overrides_auto() {
        let cli = Cli::try_parse(["cruster", "--format", "yaml", "get", "pods"]).unwrap();
        assert!(matches!(cli.format, Some(Format::Yaml)));
    }

    #[test]
    fn budget_parses_to_usize() {
        let cli = Cli::try_parse(["cruster", "--budget", "1500", "get", "pods"]).unwrap();
        assert_eq!(cli.budget, Some(1500));
    }
}
```

- [ ] **Step 2: Output mode detection**

Replace `app/crates/cruster-cli/src/output.rs`:

```rust
//! Output mode: which format to use and where to write.
//!
//! `--format` is honoured exactly when set. With no `--format`:
//! - `--llm` forces `ndjson`
//! - otherwise: TTY stdout → `text`, non-TTY → `ndjson`
//!
//! This means a piped invocation (`cruster get pods | jq …`) gets
//! structured output by default, with no flag needed.

use is_terminal::IsTerminal;

use crate::args::Format;

/// Decide the effective format from flags + TTY state.
pub fn effective_format(explicit: Option<Format>, llm: bool, is_tty: bool) -> Format {
    if let Some(f) = explicit {
        return f;
    }
    if llm || !is_tty {
        Format::Ndjson
    } else {
        Format::Text
    }
}

/// Convenience: is stdout currently a TTY?
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_format_wins() {
        assert!(matches!(
            effective_format(Some(Format::Yaml), false, true),
            Format::Yaml
        ));
        assert!(matches!(
            effective_format(Some(Format::Text), true, false),
            Format::Text
        ));
    }

    #[test]
    fn llm_implies_ndjson() {
        assert!(matches!(effective_format(None, true, true), Format::Ndjson));
    }

    #[test]
    fn non_tty_implies_ndjson() {
        assert!(matches!(
            effective_format(None, false, false),
            Format::Ndjson
        ));
    }

    #[test]
    fn tty_without_llm_is_text() {
        assert!(matches!(effective_format(None, false, true), Format::Text));
    }
}
```

- [ ] **Step 3: Formatter trait**

Replace `app/crates/cruster-cli/src/format.rs`:

```rust
//! Output formatters.
//!
//! Each verb produces a `Vec<T>` (or a stream of `T`) and feeds it to
//! a `Formatter<T>`. Implementations: text (per-kind columns), ndjson
//! (one JSON object per line), json (single array), yaml.
//!
//! Text formatters are kind-specific because columns differ. JSON /
//! NDJSON / YAML are generic over any `Serialize` type.

use std::io::{self, Write};

use serde::Serialize;

use crate::args::Format;

/// Write each record of `records` to `out` per the chosen format.
///
/// `text_writer` is only consulted when format is `Text`; pass a no-op
/// closure if your verb doesn't have a text representation (then `Text`
/// falls through to NDJSON).
pub fn write_records<T, W, F>(
    out: &mut W,
    format: Format,
    records: &[T],
    mut text_writer: F,
) -> io::Result<()>
where
    T: Serialize,
    W: Write,
    F: FnMut(&mut W, &[T]) -> io::Result<()>,
{
    match format {
        Format::Text => text_writer(out, records),
        Format::Ndjson => {
            for r in records {
                let mut line = serde_json::to_vec(r).expect("serialize record");
                line.push(b'\n');
                out.write_all(&line)?;
            }
            Ok(())
        }
        Format::Json => {
            let buf = serde_json::to_vec_pretty(records).expect("serialize array");
            out.write_all(&buf)?;
            out.write_all(b"\n")?;
            Ok(())
        }
        Format::Yaml => {
            let buf = serde_yaml::to_string(records).expect("serialize yaml");
            out.write_all(buf.as_bytes())?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        name: String,
        n: i32,
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                name: "a".into(),
                n: 1,
            },
            Row {
                name: "b".into(),
                n: 2,
            },
        ]
    }

    #[test]
    fn ndjson_writes_one_line_per_record() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Ndjson, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"name\":\"a\""));
        assert!(lines[1].contains("\"name\":\"b\""));
    }

    #[test]
    fn json_writes_pretty_array() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Json, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn yaml_writes_yaml_array() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Yaml, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("- name: a"));
    }

    #[test]
    fn text_uses_custom_writer() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Text, &rows(), |w, rs| {
            for r in rs {
                writeln!(w, "{}\t{}", r.name, r.n)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a\t1\nb\t2\n");
    }
}
```

- [ ] **Step 4: Test + commit**

```bash
cd app && cargo test -p cruster-cli && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-cli
git commit -m "feat(cli): add clap arg schema, Formatter, output mode detection"
```

---

## Task 3: Field pruning library

**Files:**
- Replace: `app/crates/cruster-cli/src/prune.rs`

What gets pruned by default (when `--full` is not set):
- `metadata.managedFields` (always — huge, useless to humans + LLMs)
- `metadata.annotations["kubectl.kubernetes.io/last-applied-configuration"]`
- `status.conditions[*].lastTransitionTime` and other timestamps that
  don't help reasoning
- `metadata.resourceVersion`, `metadata.uid`, `metadata.generation`
- `metadata.creationTimestamp` if `--llm` (kept for human text output)

We operate on `serde_json::Value` so the pruner is kind-agnostic. The
trade-off: we walk the JSON tree once per record. Cheap.

- [ ] **Step 1: Write the pruner**

Replace `app/crates/cruster-cli/src/prune.rs`:

```rust
//! Field pruning: remove apiserver noise from serialised k8s objects.
//!
//! Default (LLM/non-full mode) drops:
//! - `metadata.managedFields`
//! - `metadata.annotations["kubectl.kubernetes.io/last-applied-configuration"]`
//! - `metadata.resourceVersion`, `metadata.uid`, `metadata.generation`
//! - `metadata.creationTimestamp` (kept in `--full`)
//! - `status.conditions[*].lastTransitionTime`
//! - `status.conditions[*].lastHeartbeatTime` (Nodes specifically)

use serde_json::Value;

/// Prune in-place. No-op if `full` is true.
pub fn prune(value: &mut Value, full: bool) {
    if full {
        return;
    }
    prune_metadata(value);
    prune_status_conditions(value);
}

fn prune_metadata(value: &mut Value) {
    let Some(meta) = value.get_mut("metadata").and_then(|m| m.as_object_mut()) else {
        return;
    };
    meta.remove("managedFields");
    meta.remove("resourceVersion");
    meta.remove("uid");
    meta.remove("generation");
    meta.remove("creationTimestamp");
    if let Some(annotations) = meta
        .get_mut("annotations")
        .and_then(|a| a.as_object_mut())
    {
        annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
        if annotations.is_empty() {
            meta.remove("annotations");
        }
    }
}

fn prune_status_conditions(value: &mut Value) {
    let Some(conditions) = value
        .get_mut("status")
        .and_then(|s| s.get_mut("conditions"))
        .and_then(|c| c.as_array_mut())
    else {
        return;
    };
    for c in conditions {
        if let Some(obj) = c.as_object_mut() {
            obj.remove("lastTransitionTime");
            obj.remove("lastHeartbeatTime");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_mode_is_noop() {
        let mut v = json!({"metadata": {"managedFields": [1, 2, 3]}});
        prune(&mut v, true);
        assert!(v["metadata"]["managedFields"].is_array());
    }

    #[test]
    fn managed_fields_removed_by_default() {
        let mut v = json!({"metadata": {"name": "x", "managedFields": [1, 2, 3]}});
        prune(&mut v, false);
        assert!(v["metadata"].get("managedFields").is_none());
        assert_eq!(v["metadata"]["name"], "x");
    }

    #[test]
    fn last_applied_annotation_removed() {
        let mut v = json!({
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{...big blob...}",
                    "app.kubernetes.io/name": "nginx"
                }
            }
        });
        prune(&mut v, false);
        let ann = &v["metadata"]["annotations"];
        assert!(ann
            .get("kubectl.kubernetes.io/last-applied-configuration")
            .is_none());
        assert_eq!(ann["app.kubernetes.io/name"], "nginx");
    }

    #[test]
    fn empty_annotations_block_removed_entirely() {
        let mut v = json!({
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{...}"
                }
            }
        });
        prune(&mut v, false);
        assert!(v["metadata"].get("annotations").is_none());
    }

    #[test]
    fn status_condition_timestamps_removed() {
        let mut v = json!({
            "status": {
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "True",
                        "lastTransitionTime": "2026-01-01T00:00:00Z",
                        "lastHeartbeatTime": "2026-01-01T00:01:00Z"
                    }
                ]
            }
        });
        prune(&mut v, false);
        let cond = &v["status"]["conditions"][0];
        assert!(cond.get("lastTransitionTime").is_none());
        assert!(cond.get("lastHeartbeatTime").is_none());
        assert_eq!(cond["type"], "Ready");
        assert_eq!(cond["status"], "True");
    }

    #[test]
    fn missing_metadata_is_noop() {
        let mut v = json!({"foo": "bar"});
        prune(&mut v, false); // must not panic
        assert_eq!(v["foo"], "bar");
    }
}
```

- [ ] **Step 2: Test + commit**

```bash
cd app && cargo test -p cruster-cli && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-cli
git commit -m "feat(cli): add field-pruning library for agent-friendly output"
```

---

## Task 4: `cruster get pods` (canonical)

**Files:**
- Create: `app/crates/cruster-cli/src/verbs/get.rs`
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`
- Create: `app/crates/cruster-cli/schemas/get-pod.schema.json`

Establishes the pattern for all `cruster get` verbs. Each kind reuses
the same machinery; later tasks just plug in their k8s type.

- [ ] **Step 1: Write the get verb**

Create `app/crates/cruster-cli/src/verbs/get.rs`:

```rust
//! `cruster get <kind>` — list resources of a kind.
//!
//! Output:
//! - text: a per-kind column table (mirrors the TUI views)
//! - ndjson / json / yaml: pruned k8s objects (subject to `--full`)

use std::io::Write;

use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use serde_json::Value;

use crate::args::{Cli, Format, GetArgs};
use crate::format::write_records;
use crate::prune::prune;

/// Normalise user-typed kind/alias to canonical plural.
pub fn canonicalise_kind(raw: &str) -> Option<&'static str> {
    match raw {
        "po" | "pod" | "pods" => Some("pods"),
        "deploy" | "deployment" | "deployments" => Some("deployments"),
        "svc" | "service" | "services" => Some("services"),
        "no" | "node" | "nodes" => Some("nodes"),
        "ev" | "event" | "events" => Some("events"),
        "cm" | "configmap" | "configmaps" => Some("configmaps"),
        "sec" | "secret" | "secrets" => Some("secrets"),
        "ns" | "namespace" | "namespaces" => Some("namespaces"),
        _ => None,
    }
}

pub async fn run(cli: &Cli, args: &GetArgs) -> anyhow::Result<()> {
    let kind = canonicalise_kind(&args.kind)
        .ok_or_else(|| anyhow::anyhow!("unknown kind: {}", args.kind))?;
    match kind {
        "pods" => run_pods(cli, args).await,
        other => {
            anyhow::bail!("get {other}: not yet implemented (see Phase 2B plan)")
        }
    }
}

async fn run_pods(cli: &Cli, args: &GetArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let api: Api<Pod> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let list_params = build_list_params(args);
    let mut pods = api.list(&list_params).await?.items;

    // Optional name filter (server-side --field-selector would also work).
    if let Some(name) = &args.name {
        pods.retain(|p| p.metadata.name.as_deref() == Some(name));
    }

    // Prune each pod.
    let mut records: Vec<Value> = pods
        .iter()
        .map(|p| serde_json::to_value(p).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
    }

    let format = crate::output::effective_format(cli.format, cli.llm, crate::output::stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &records, |w, rs| {
        write_pods_text(w, rs, &pods)
    })?;
    Ok(())
}

fn build_list_params(args: &GetArgs) -> kube::api::ListParams {
    let mut params = kube::api::ListParams::default();
    if let Some(sel) = &args.selector {
        params = params.labels(sel);
    }
    params
}

fn write_pods_text<W: Write>(out: &mut W, _values: &[Value], pods: &[Pod]) -> std::io::Result<()> {
    writeln!(out, "NAMESPACE\tNAME\tSTATUS\tREADY\tRESTARTS")?;
    for p in pods {
        let ns = p.metadata.namespace.as_deref().unwrap_or("-");
        let name = p.metadata.name.as_deref().unwrap_or("?");
        let status = pod_phase(p);
        let ready = pod_ready(p);
        let restarts = pod_restarts(p);
        writeln!(out, "{ns}\t{name}\t{status}\t{ready}\t{restarts}")?;
    }
    Ok(())
}

fn pod_phase(p: &Pod) -> String {
    p.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "?".into())
}

fn pod_ready(p: &Pod) -> String {
    let cs = p
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    let total = cs.len();
    let ready = cs.iter().filter(|c| c.ready).count();
    format!("{ready}/{total}")
}

fn pod_restarts(p: &Pod) -> i32 {
    p.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalise_recognises_aliases() {
        assert_eq!(canonicalise_kind("po"), Some("pods"));
        assert_eq!(canonicalise_kind("Pods"), None); // case-sensitive
        assert_eq!(canonicalise_kind("svc"), Some("services"));
        assert_eq!(canonicalise_kind("nope"), None);
    }
}
```

- [ ] **Step 2: Wire dispatch**

Replace `app/crates/cruster-cli/src/verbs/mod.rs`:

```rust
//! CLI verb handlers (one module per verb).

pub mod get;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(_) => anyhow::bail!("describe: not yet implemented (Task 12)"),
        Command::Logs(_) => anyhow::bail!("logs: not yet implemented (Task 13)"),
        Command::Events(_) => anyhow::bail!("events: not yet implemented (Task 14)"),
        Command::Schema(_) => anyhow::bail!("schema: not yet implemented (Task 16)"),
        Command::HelpJson => anyhow::bail!("help-json: not yet implemented (Task 16)"),
    }
}
```

- [ ] **Step 3: Schema file**

Create `app/crates/cruster-cli/schemas/get-pod.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft-07/schema",
  "$id": "https://cruster.dev/schemas/get-pod/v1.json",
  "title": "cruster get pod (NDJSON record)",
  "description": "One record per pod. Default mode emits pruned k8s Pod objects (managedFields, resourceVersion, last-applied annotation, status condition timestamps removed).",
  "type": "object",
  "required": ["metadata"],
  "properties": {
    "apiVersion": { "type": "string" },
    "kind": { "type": "string", "const": "Pod" },
    "metadata": {
      "type": "object",
      "required": ["name", "namespace"],
      "properties": {
        "name": { "type": "string" },
        "namespace": { "type": "string" },
        "labels": { "type": "object" },
        "annotations": { "type": "object" }
      }
    },
    "spec": { "type": "object" },
    "status": { "type": "object" }
  }
}
```

- [ ] **Step 4: Manual smoke test + commit**

```bash
cd app && cargo build -p cruster-bin
./target/debug/cruster get pods --format text
./target/debug/cruster get pods --format ndjson | head -1 | jq .
./target/debug/cruster get pods   # auto-detects: text in TTY, ndjson when piped
./target/debug/cruster get pods | jq .  # ndjson because pipe
```

Expected: text shows the same columns as the TUI; ndjson emits one
JSON object per line; piped invocation auto-switches to ndjson.

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app/crates/cruster-cli
git commit -m "feat(cli): add cruster get pods (text + ndjson + schema)"
```

---

## Tasks 5–11: `cruster get` for the other 7 kinds

For each kind, repeat the Task 4 pattern: add a `run_<kind>` branch in
`get::run`, write the per-kind text formatter, ship a schema file.
Each task is one small commit.

### Task 5: `cruster get deployments`

In `get.rs`, add an arm to the `match kind` in `run`:

```rust
"deployments" => run_deployments(cli, args).await,
```

Write `run_deployments` analogous to `run_pods`, using `Api<Deployment>`
from `k8s_openapi::api::apps::v1::Deployment`. Text columns:
`NAMESPACE`, `NAME`, `READY`, `UP-TO-DATE`, `AVAILABLE` (mirror the
TUI's DeploymentsView).

Schema file: `schemas/get-deployment.schema.json` (copy
`get-pod.schema.json` and adjust `kind` const to `"Deployment"`).

Commit: `feat(cli): add cruster get deployments`.

### Task 6: `cruster get services`

Same pattern. `Api<Service>`. Text columns: `NAMESPACE`, `NAME`,
`TYPE`, `CLUSTER-IP`, `PORTS`.

Commit: `feat(cli): add cruster get services`.

### Task 7: `cruster get nodes`

Cluster-scoped — `Api::all` always; ignore `--namespace`. Text columns:
`NAME`, `STATUS`, `ROLES`, `VERSION`, `OS-IMAGE`.

Commit: `feat(cli): add cruster get nodes`.

### Task 8: `cruster get events`

Same pattern. Sort by `last_timestamp` desc before emitting. Text
columns: `NAMESPACE`, `LAST SEEN`, `TYPE`, `REASON`, `OBJECT`,
`MESSAGE`.

Commit: `feat(cli): add cruster get events`.

### Task 9: `cruster get configmaps`

Same pattern. Text columns: `NAMESPACE`, `NAME`, `DATA` (key count),
`AGE`. Do **not** emit data values in text output (the structured
output respects `--full` for whether to include them).

Commit: `feat(cli): add cruster get configmaps`.

### Task 10: `cruster get secrets` (with redaction)

Same pattern, but with a **hard invariant**: in any format, **never
emit secret data values**. The pruner pass must replace each value in
`data` and `stringData` with `"<redacted>"` (or the byte-string
equivalent for `data`) — even with `--full`.

In `prune.rs`, add a `prune_secret(value: &mut Value)` helper that's
called specifically for Secret records (independent of the `full`
flag — secrets are always redacted).

Add a unit test asserting that no fixture secret value appears in the
serialised output of `get secrets`.

Text columns: `NAMESPACE`, `NAME`, `TYPE`, `DATA` (key count), `AGE`.

Commit: `feat(cli): add cruster get secrets with hard redaction`.

### Task 11: `cruster get namespaces`

Cluster-scoped. Text columns: `NAME`, `STATUS`, `AGE`.

Commit: `feat(cli): add cruster get namespaces`.

---

## Task 12: `cruster describe <kind>/<name>`

**Files:**
- Create: `app/crates/cruster-cli/src/verbs/describe.rs`
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`
- Create: `app/crates/cruster-cli/schemas/describe.schema.json`

For v1, `describe` returns the full (pruned) object for the named
resource — same data as `cruster get <kind> <name>` would, but for
exactly one resource. The structured output is the canonical YAML/JSON
of the object; text output is a kubectl-describe-style summary
(headers + indented field/value lines).

A "structured describe" with pre-decomposed analysis fields is Phase
4 work — out of scope here.

- [ ] **Step 1: Parse `kind/name` references**

In `app/crates/cruster-cli/src/verbs/describe.rs`:

```rust
//! `cruster describe <kind>/<name>` — full info on one resource.

use crate::args::{Cli, DescribeArgs};

pub async fn run(cli: &Cli, args: &DescribeArgs) -> anyhow::Result<()> {
    let (kind, name) = parse_reference(&args.reference)?;
    // Reuse get logic with name filter set.
    let get_args = crate::args::GetArgs {
        kind: kind.into(),
        name: Some(name.into()),
        namespace: args.namespace.clone(),
        selector: None,
    };
    crate::verbs::get::run(cli, &get_args).await
}

pub fn parse_reference(s: &str) -> anyhow::Result<(&str, &str)> {
    let (kind, name) = s
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected kind/name (got '{s}')"))?;
    if kind.is_empty() || name.is_empty() {
        anyhow::bail!("expected kind/name (got '{s}')");
    }
    Ok((kind, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_reference() {
        assert_eq!(parse_reference("pod/nginx").unwrap(), ("pod", "nginx"));
        assert_eq!(parse_reference("deploy/web").unwrap(), ("deploy", "web"));
    }

    #[test]
    fn rejects_missing_slash() {
        assert!(parse_reference("pod-nginx").is_err());
    }

    #[test]
    fn rejects_empty_parts() {
        assert!(parse_reference("/nginx").is_err());
        assert!(parse_reference("pod/").is_err());
    }
}
```

- [ ] **Step 2: Wire dispatch + schema**

Update `verbs/mod.rs`:

```rust
pub mod describe;
pub mod get;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(args) => describe::run(&cli, args).await,
        Command::Logs(_) => anyhow::bail!("logs: not yet implemented (Task 13)"),
        Command::Events(_) => anyhow::bail!("events: not yet implemented (Task 14)"),
        Command::Schema(_) => anyhow::bail!("schema: not yet implemented (Task 16)"),
        Command::HelpJson => anyhow::bail!("help-json: not yet implemented (Task 16)"),
    }
}
```

Schema (`schemas/describe.schema.json`):

```json
{
  "$schema": "https://json-schema.org/draft-07/schema",
  "$id": "https://cruster.dev/schemas/describe/v1.json",
  "title": "cruster describe (NDJSON record)",
  "description": "Always one record: the named resource. Schema matches the kind's `get` schema.",
  "type": "object"
}
```

- [ ] **Step 3: Test + smoke test + commit**

```bash
cd app && cargo test -p cruster-cli && cargo build -p cruster-bin
./target/debug/cruster describe pod/<name> -n <ns> --format yaml
git add app/crates/cruster-cli
git commit -m "feat(cli): add cruster describe (delegates to get with name filter)"
```

---

## Task 13: `cruster logs <pod>` (with filters)

**Files:**
- Create: `app/crates/cruster-cli/src/verbs/logs.rs`
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`
- Create: `app/crates/cruster-cli/schemas/logs.schema.json`

Streams pod logs to stdout. In text mode: raw log lines. In structured
mode (ndjson): one record per line with `{timestamp?, line, pod, container}`.

`--follow` streams indefinitely (until Ctrl-C). `--tail`, `--since`,
`--grep` are filters. `--container` selects a specific container in a
multi-container pod.

- [ ] **Step 1: Write the verb**

```rust
//! `cruster logs <pod>` — print pod logs.

use std::io::Write;
use std::str::FromStr;

use anyhow::Context;
use chrono::Utc;
use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use serde::Serialize;

use crate::args::{Cli, Format, LogsArgs};
use crate::output::{effective_format, stdout_is_tty};

#[derive(Serialize)]
struct LogRecord<'a> {
    pod: &'a str,
    container: Option<&'a str>,
    line: &'a str,
}

pub async fn run(cli: &Cli, args: &LogsArgs) -> anyhow::Result<()> {
    let pod_name = args.pod.strip_prefix("pod/").unwrap_or(&args.pod);
    let ns = args
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--namespace is required for logs"))?;

    let client = Client::try_default().await?;
    let api: Api<Pod> = Api::namespaced(client, ns);

    let mut params = LogParams {
        follow: args.follow,
        container: args.container.clone(),
        ..Default::default()
    };
    params.tail_lines = args.tail;
    if let Some(since) = &args.since {
        params.since_seconds = Some(parse_duration_seconds(since)?);
    }

    let stream = api
        .log_stream(pod_name, &params)
        .await
        .context("opening log stream")?;
    let mut reader = stream.lines();

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();

    while let Some(line) = reader.try_next().await? {
        if let Some(grep) = &args.grep {
            if !line.contains(grep) {
                continue;
            }
        }
        match format {
            Format::Text => writeln!(stdout, "{line}")?,
            Format::Ndjson | Format::Json | Format::Yaml => {
                let rec = LogRecord {
                    pod: pod_name,
                    container: args.container.as_deref(),
                    line: &line,
                };
                let s = serde_json::to_string(&rec).expect("serialize");
                writeln!(stdout, "{s}")?;
            }
        }
        stdout.flush()?;
    }
    Ok(())
}

/// Parse a `5m`, `2h`, `30s`, `1d` duration into seconds.
fn parse_duration_seconds(s: &str) -> anyhow::Result<i64> {
    let (num, suffix) = s.split_at(s.len().saturating_sub(1));
    let n: i64 = i64::from_str(num).context("duration number")?;
    match suffix {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86400),
        _ => anyhow::bail!("unknown duration suffix: {suffix}"),
    }
}

// Touch unused imports to silence clippy; remove once real use lands.
#[allow(dead_code)]
fn _unused(_: Utc) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration_seconds("30s").unwrap(), 30);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
        assert_eq!(parse_duration_seconds("1d").unwrap(), 86400);
    }

    #[test]
    fn rejects_bogus_durations() {
        assert!(parse_duration_seconds("5x").is_err());
        assert!(parse_duration_seconds("abc").is_err());
    }
}
```

- [ ] **Step 2: Wire + schema + commit**

Update `verbs/mod.rs` to dispatch `Logs`. Schema file
`schemas/logs.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft-07/schema",
  "$id": "https://cruster.dev/schemas/logs/v1.json",
  "title": "cruster logs (NDJSON record)",
  "type": "object",
  "required": ["pod", "line"],
  "properties": {
    "pod": { "type": "string" },
    "container": { "type": ["string", "null"] },
    "line": { "type": "string" }
  }
}
```

Commit: `feat(cli): add cruster logs with --follow/--tail/--since/--grep`.

---

## Task 14: `cruster events`

**Files:**
- Create: `app/crates/cruster-cli/src/verbs/events.rs`
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`
- Create: `app/crates/cruster-cli/schemas/events.schema.json`

Same as `cruster get events` but with first-class flags for the most
common queries: `--namespace`, `--resource <kind/name>`, `--limit N`.

- [ ] **Step 1: Verb**

```rust
//! `cruster events` — recent cluster events.

use k8s_openapi::api::core::v1::Event;
use kube::{Api, Client};
use serde_json::Value;

use crate::args::{Cli, EventsArgs};
use crate::format::write_records;
use crate::output::{effective_format, stdout_is_tty};
use crate::prune::prune;

pub async fn run(cli: &Cli, args: &EventsArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let api: Api<Event> = match &args.namespace {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let mut events = api.list(&Default::default()).await?.items;

    if let Some(resource) = &args.resource {
        let (kind, name) = crate::verbs::describe::parse_reference(resource)?;
        events.retain(|e| {
            e.involved_object.kind.as_deref() == Some(kind)
                && e.involved_object.name.as_deref() == Some(name)
        });
    }

    events.sort_by(|a, b| {
        let at = a.last_timestamp.as_ref().map(|t| t.0);
        let bt = b.last_timestamp.as_ref().map(|t| t.0);
        bt.cmp(&at)
    });
    events.truncate(args.limit);

    let mut records: Vec<Value> = events
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize"))
        .collect();
    for r in &mut records {
        prune(r, cli.full);
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    write_records(&mut stdout, format, &records, |w, _vs| {
        use std::io::Write;
        writeln!(w, "NAMESPACE\tLAST_SEEN\tTYPE\tREASON\tOBJECT\tMESSAGE")?;
        for e in &events {
            let ns = e.metadata.namespace.as_deref().unwrap_or("-");
            let last = e
                .last_timestamp
                .as_ref()
                .map(|t| t.0.to_rfc3339())
                .unwrap_or_else(|| "?".into());
            let ty = e.type_.as_deref().unwrap_or("-");
            let reason = e.reason.as_deref().unwrap_or("-");
            let obj = format!(
                "{}/{}",
                e.involved_object.kind.as_deref().unwrap_or("?"),
                e.involved_object.name.as_deref().unwrap_or("?")
            );
            let msg = e.message.as_deref().unwrap_or("");
            writeln!(w, "{ns}\t{last}\t{ty}\t{reason}\t{obj}\t{msg}")?;
        }
        Ok(())
    })?;
    Ok(())
}
```

- [ ] **Step 2: Wire + schema + commit**

Update `verbs/mod.rs`. Schema `schemas/events.schema.json` analogous
to `get-event.schema.json`.

Commit: `feat(cli): add cruster events with --resource and --limit`.

---

## Task 15: Token budget trimming (`--budget`)

**Files:**
- Replace: `app/crates/cruster-cli/src/budget.rs`
- Modify: `app/crates/cruster-cli/src/format.rs`

`--budget N` caps output at approximately N tokens (1 token ≈ 4
characters as a coarse heuristic — agents use the env-aware exact
token count separately). Trimming happens at record boundaries:
emit records until the next one would push past the budget, then
emit a final `{"truncated": true, "remaining": M}` marker.

- [ ] **Step 1: Implement budget logic**

Replace `app/crates/cruster-cli/src/budget.rs`:

```rust
//! Token-budget trimming for streaming output.
//!
//! Heuristic: 1 token ≈ 4 characters. Coarse but stable. Records are
//! emitted whole; the first record that would push the running total
//! past the budget is dropped and a `truncated` marker is emitted in
//! its place.

pub const CHARS_PER_TOKEN: usize = 4;

pub struct Budget {
    cap_chars: usize,
    used_chars: usize,
}

impl Budget {
    /// `tokens` is the user-facing budget; converted to a char cap.
    pub fn new(tokens: usize) -> Self {
        Self {
            cap_chars: tokens.saturating_mul(CHARS_PER_TOKEN),
            used_chars: 0,
        }
    }

    /// `true` if `additional_chars` fits within the remaining budget.
    pub fn fits(&self, additional_chars: usize) -> bool {
        self.used_chars + additional_chars <= self.cap_chars
    }

    /// Commit the chars to the running total.
    pub fn consume(&mut self, additional_chars: usize) {
        self.used_chars = self.used_chars.saturating_add(additional_chars);
    }

    pub fn remaining_chars(&self) -> usize {
        self.cap_chars.saturating_sub(self.used_chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_within_budget() {
        let b = Budget::new(100); // 400 chars
        assert!(b.fits(200));
    }

    #[test]
    fn rejects_overflow() {
        let mut b = Budget::new(100);
        b.consume(300);
        assert!(!b.fits(200));
        assert!(b.fits(100));
    }
}
```

- [ ] **Step 2: Plumb into `write_records`**

Extend `format.rs` with a `write_records_budgeted` variant that the
verbs can opt into. The basic `write_records` stays unchanged for
small outputs that don't need budgeting. For `get`, switch to the
budgeted variant when `cli.budget.is_some()`.

Provide an integration test that writes 100 records with a tiny
budget and verifies the truncation marker is the final line.

Commit: `feat(cli): add --budget trimming for record-based output`.

---

## Task 16: Self-describing (`cruster schema <verb>`, `cruster help-json`)

**Files:**
- Replace: `app/crates/cruster-cli/src/schemas.rs`
- Create: `app/crates/cruster-cli/src/verbs/schema.rs`
- Create: `app/crates/cruster-cli/src/verbs/help_json.rs`
- Modify: `app/crates/cruster-cli/src/verbs/mod.rs`

`cruster schema <verb>` prints the JSON schema for that verb's
structured output. `cruster help-json` prints the full command tree as
JSON (verb names, descriptions, flags, types) — what agents read at
discovery time.

- [ ] **Step 1: Bundle schemas**

Replace `app/crates/cruster-cli/src/schemas.rs`:

```rust
//! Compiled-in schema registry.
//!
//! Each `cruster get <kind>` / `cruster describe` / etc. ships a JSON
//! schema. `cruster schema <verb>` looks up the schema by verb name.

const fn pair(name: &'static str, body: &'static str) -> (&'static str, &'static str) {
    (name, body)
}

pub const SCHEMAS: &[(&str, &str)] = &[
    pair("get-pod", include_str!("../schemas/get-pod.schema.json")),
    pair("get-deployment", include_str!("../schemas/get-deployment.schema.json")),
    pair("get-service", include_str!("../schemas/get-service.schema.json")),
    pair("get-node", include_str!("../schemas/get-node.schema.json")),
    pair("get-event", include_str!("../schemas/get-event.schema.json")),
    pair("get-configmap", include_str!("../schemas/get-configmap.schema.json")),
    pair("get-secret", include_str!("../schemas/get-secret.schema.json")),
    pair("get-namespace", include_str!("../schemas/get-namespace.schema.json")),
    pair("describe", include_str!("../schemas/describe.schema.json")),
    pair("logs", include_str!("../schemas/logs.schema.json")),
    pair("events", include_str!("../schemas/events.schema.json")),
];

pub fn lookup(verb: &str) -> Option<&'static str> {
    SCHEMAS
        .iter()
        .find_map(|(n, body)| (n == &verb).then_some(*body))
}

pub fn verb_names() -> Vec<&'static str> {
    SCHEMAS.iter().map(|(n, _)| *n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_pod_schema() {
        let s = lookup("get-pod").expect("present");
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(v["title"], "cruster get pod (NDJSON record)");
    }

    #[test]
    fn lookup_misses_unknown_verb() {
        assert!(lookup("get-quark").is_none());
    }

    #[test]
    fn every_listed_verb_has_valid_json() {
        for (name, body) in SCHEMAS {
            let _: serde_json::Value = serde_json::from_str(body)
                .unwrap_or_else(|e| panic!("schema {name} invalid: {e}"));
        }
    }
}
```

- [ ] **Step 2: Schema verb**

`app/crates/cruster-cli/src/verbs/schema.rs`:

```rust
//! `cruster schema <verb>` — print the verb's structured-output schema.

use crate::args::{Cli, SchemaArgs};
use crate::schemas;

pub async fn run(_cli: &Cli, args: &SchemaArgs) -> anyhow::Result<()> {
    let body = schemas::lookup(&args.verb).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown verb '{}'. Known verbs: {}",
            args.verb,
            schemas::verb_names().join(", ")
        )
    })?;
    println!("{body}");
    Ok(())
}
```

- [ ] **Step 3: help-json**

`app/crates/cruster-cli/src/verbs/help_json.rs`:

```rust
//! `cruster help-json` — machine-readable command tree.
//!
//! Agents call this at startup to discover the verb surface. The
//! format intentionally mirrors a subset of OpenAPI: a list of
//! commands, each with a name, description, and parameter list.

use clap::CommandFactory;
use serde::Serialize;

use crate::args::Cli;

#[derive(Serialize)]
struct CommandDoc {
    name: String,
    about: String,
    flags: Vec<FlagDoc>,
}

#[derive(Serialize)]
struct FlagDoc {
    name: String,
    short: Option<String>,
    long: Option<String>,
    description: String,
    required: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let cmd = Cli::command();
    let mut docs = Vec::new();
    for sub in cmd.get_subcommands() {
        let mut flags = Vec::new();
        for arg in sub.get_arguments() {
            flags.push(FlagDoc {
                name: arg.get_id().to_string(),
                short: arg.get_short().map(|c| c.to_string()),
                long: arg.get_long().map(|s| s.to_string()),
                description: arg.get_help().map(|h| h.to_string()).unwrap_or_default(),
                required: arg.is_required_set(),
            });
        }
        docs.push(CommandDoc {
            name: sub.get_name().to_string(),
            about: sub
                .get_about()
                .map(|a| a.to_string())
                .unwrap_or_default(),
            flags,
        });
    }
    println!("{}", serde_json::to_string_pretty(&docs)?);
    Ok(())
}
```

- [ ] **Step 4: Wire + commit**

Update `verbs/mod.rs`:

```rust
pub mod describe;
pub mod events;
pub mod get;
pub mod help_json;
pub mod logs;
pub mod schema;

use crate::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Get(args) => get::run(&cli, args).await,
        Command::Describe(args) => describe::run(&cli, args).await,
        Command::Logs(args) => logs::run(&cli, args).await,
        Command::Events(args) => events::run(&cli, args).await,
        Command::Schema(args) => schema::run(&cli, args).await,
        Command::HelpJson => help_json::run().await,
    }
}
```

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
./target/debug/cruster schema get-pod | jq .
./target/debug/cruster help-json | jq '.[] | .name'
git add app/crates/cruster-cli
git commit -m "feat(cli): add schema verb + help-json for agent discovery"
```

---

## Task 17: Phase 2B exit verification

- [ ] **Step 1: Full lint + test matrix**

```bash
cd app && cargo fmt --all -- --check
cd app && cargo clippy --workspace --all-targets -- -D warnings
cd app && cargo test --workspace --all-targets
```
All three clean.

- [ ] **Step 2: Smoke-test the CLI end-to-end against k3d**

With the active context pointed at the k3d cluster:

```bash
cd app && cargo build --release -p cruster-bin
PATH=./target/release:$PATH

# Get verbs (one per kind)
cruster get pods
cruster get pods -n agent-platform
cruster get pods --format ndjson | jq .
cruster get pods --format yaml | head -20
cruster get pods --full --format json | jq '.[0].metadata | keys'  # managedFields back

cruster get deployments
cruster get services
cruster get nodes
cruster get events --limit 5
cruster get configmaps
cruster get secrets --format ndjson | jq '.data'   # should all be redacted
cruster get namespaces

# Describe
cruster describe pod/<a real pod> -n <ns> --format yaml

# Logs
cruster logs <a real pod> -n <ns> --tail 50
cruster logs <a real pod> -n <ns> --tail 50 --grep error
cruster logs <a real pod> -n <ns> --format ndjson --tail 5 | jq .

# Events
cruster events --limit 5
cruster events --resource pod/<a real pod> -n <ns>

# Self-describing
cruster schema get-pod | jq .title
cruster help-json | jq '.[].name'

# TUI still works (no args)
cruster
```

For each: confirm output structure matches expectations; no panics;
non-zero exits on bad input show clear stderr messages.

- [ ] **Step 3: Update top-level README**

Edit `README.md`:

```markdown
## Status

Phase 2B (LLM-efficient CLI) complete. `cruster get/describe/logs/events`
all available as one-shot CLI verbs with text + ndjson + json + yaml
output, field pruning, token budgeting, and per-verb JSON schemas
exposed via `cruster schema <verb>` and `cruster help-json`.

Next: Phase 3 (incident-solving ergonomics + themes).
```

Update the "Quick start" section to include a CLI example.

- [ ] **Step 4: Commit + tag**

```bash
git add README.md
git commit -m "docs: update README for phase 2B completion"
git tag -a phase-2b-llm-cli -m "Phase 2B: LLM-efficient CLI mode (get/describe/logs/events + schemas)"
```

---

## Phase 2B exit criteria

All must hold before moving on:

1. `cd app && cargo test --workspace` passes.
2. `cd app && cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cd app && cargo fmt --all -- --check` clean.
4. `cruster` with no args still launches the TUI.
5. Each of the 8 `cruster get <kind>` verbs returns sensible output in
   all four formats against the k3d cluster.
6. `cruster get secrets` never emits secret values in any format.
7. `cruster describe pod/<name>` returns the pod's full pruned YAML.
8. `cruster logs <pod> --tail 50 --grep error` filters as expected.
9. `cruster events --resource pod/<name>` filters to that pod.
10. `cruster schema get-pod` returns valid JSON; `cruster help-json`
    enumerates all verbs.
11. Piped invocations (`cruster get pods | jq .`) auto-detect non-TTY
    and emit NDJSON.
12. CI passes on a push to `main`.

When all hold, write the Phase 3 plan (incident-solving ergonomics +
themes: relationship-first navigation, faceted search, pinned/follow
panes, diff verb, inline action footer, safety badges, saved
workflows, theme engine).
