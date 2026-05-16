# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

**Phase 4A complete.** Two Pro-tier agent-bridging features landed:

- **Prompt actions (`P` leader)**: press `P` then one of `d` / `w` /
  `s` to render a Tera template against the current selection +
  recent events + recent logs, and copy the result to the clipboard.
  Drop it into Claude Code / Cursor. User templates live in
  `~/.config/cruster/prompts/*.toml`.
- **Diagnostic export**: press `E` in the TUI or run `cruster export
  pod/<name> -n <ns>` from the CLI to produce a self-contained
  markdown bundle (manifest + events + log tail). One command, one
  artifact you can paste into an incident ticket or an async agent
  handoff.

Quick wins shipped alongside: `cruster help-json` now inlines every
verb's JSON schema (agent discovery is one call instead of six);
`cruster events --resource` emits a clear sentinel when the filter
drops all events.

Earlier: Phase 1 skeleton, Phase 2A TUI parity, Phase 2B LLM-efficient
CLI, Phase 3A ergonomics, Phase 3B task-first navigation, Phase 3C
themes (Pro-gated), Phase 3D keymap presets + named layouts, Phase 3E
visual polish (thin top rule, `▎` selection accent, pane focus + Tab,
modal port-forward).

Next: Phase 4B (agentskills.io skills + Claude Code plugin), then
Phase 5 (license + landing page + polish).

## Quick start

TUI:
```sh
cd app && cargo run --release -p cruster-bin
```

CLI:
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
./target/release/cruster export pod/nginx -n default -o report.md
```

See `app/README.md` for the full TUI keymap.

## Project layout

```
cruster/
├── app/
│   ├── crates/
│   │   ├── cruster-core/   # shared types (ResourceKey, Environment)
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, ResourceKind, StoreRegistry
│   │   ├── cruster-tui/    # ratatui app, ResourceView trait, views, actions, overlays, safety
│   │   ├── cruster-cli/    # clap CLI, Formatter, prune, budget, per-verb modules + schemas
│   │   └── cruster-bin/    # single `cruster` binary; dispatches TUI vs CLI
│   └── benches/
├── web/                    # marketing site (Phase 5)
└── docs/                   # specs and implementation plans
```
