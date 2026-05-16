# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

Phase 2B (LLM-efficient CLI) complete. The same `cruster` binary
launches the TUI when invoked with no args, and runs as a one-shot
CLI when invoked with a verb. CLI surface:

- `cruster get <kind> [name]` — list resources of all 8 supported kinds
- `cruster describe <kind>/<name>` — full pruned object for one resource
- `cruster logs <pod> [--follow] [--tail] [--since] [--grep]`
- `cruster events [--resource <kind/name>] [--limit N]`
- `cruster schema <verb>` — JSON schema of a verb's structured output
- `cruster help-json` — machine-readable command tree for agent discovery

Output auto-detects: text in a TTY, NDJSON when piped (or with `--llm`).
`--format text|json|ndjson|yaml` forces a specific format. `--full`
disables agent-friendly field pruning. `--budget N` caps NDJSON output
at ≈N tokens with a `{"truncated": true, "remaining": M}` marker.
Secrets are always redacted (`<redacted>`), even with `--full`.

Next: Phase 3 (incident-solving ergonomics + themes).

## Quick start

TUI:
```sh
cd app && cargo run --release -p cruster-bin
```

CLI (a few examples):
```sh
cd app
cargo build --release -p cruster-bin
./target/release/cruster get pods
./target/release/cruster get pods --format ndjson | jq .
./target/release/cruster describe pod/nginx -n default --format yaml
./target/release/cruster logs nginx -n default --tail 50 --grep error
./target/release/cruster events --limit 5
./target/release/cruster schema get-pod | jq .title
./target/release/cruster help-json | jq '.[].name'
```

Uses your active kubeconfig context. See `app/README.md` for the full
TUI keymap.

## Project layout

```
cruster/
├── app/
│   ├── crates/
│   │   ├── cruster-core/   # shared types
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, ResourceKind, StoreRegistry
│   │   ├── cruster-tui/    # ratatui app, ResourceView trait, kind views, action panes
│   │   ├── cruster-cli/    # clap CLI, Formatter, prune, budget, per-verb modules + schemas
│   │   └── cruster-bin/    # single `cruster` binary; dispatches TUI vs CLI
│   └── benches/
├── web/                    # marketing site (Phase 5)
└── docs/                   # specs and implementation plans
```
