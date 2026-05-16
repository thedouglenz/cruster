# cruster

A Kubernetes TUI for people who live in k9s all day. Built in Rust.
Optimized for speed, ergonomics, and Claude Code compatibility.

This repository is a monorepo:

- `app/` — the Rust TUI + CLI (cargo workspace, `cd app && cargo build`)
- `skills/` — agentskills.io-format skills for any compatible AI agent
- `plugins/claude-code/` — Claude Code plugin (skills + slash commands)
- `web/` — the marketing site (not yet built)
- `docs/` — specs and implementation plans

## Status

**Phase 4B complete.** The agent distribution surface is shipped:

- **`skills/`** — three agentskills.io-format skills
  (`cruster-investigate-pod`, `cruster-resource-bundle`,
  `cruster-recent-activity`) that drop into Claude Code, Cursor,
  Codex, Gemini CLI, Goose, OpenHands, and other agentskills.io-
  compatible agents.
- **`plugins/claude-code/`** — a Claude Code plugin bundling the
  same three skills plus three slash commands: `/cruster:investigate`,
  `/cruster:bundle`, `/cruster:recent`.
- **`install.sh`** — one-shot installer
  (`./install.sh --agent claude`) for any of the supported targets.

Phase 4A: prompt actions (`P` leader) and diagnostic export (`E` in
TUI, `cruster export` in CLI). Phase 4A quick wins: inline schemas
in `help-json` and a filter-empty sentinel in `events --resource`.

Earlier: Phase 1 skeleton, Phase 2A TUI parity, Phase 2B LLM-efficient
CLI, Phase 3A ergonomics, Phase 3B task-first navigation, Phase 3C
themes (Pro-gated), Phase 3D keymap presets + named layouts, Phase 3E
visual polish (thin top rule, `▎` selection accent, pane focus + Tab,
modal port-forward).

Next: Phase 5 (license + landing page + polish).

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

## Agent integration

Cruster's CLI is designed to be driven by an AI coding agent. Two
levels of integration:

### Level 1 — drop-in agentskills

For any agent that reads agentskills.io-format skill directories
(Claude Code, Cursor, Codex, Goose, OpenHands, Gemini CLI, ...):

```sh
./install.sh --agent claude      # → ~/.claude/skills/
./install.sh --agent cursor      # → ./.cursor/skills/
./install.sh --agent generic     # → ~/.agentskills/
```

`./install.sh --help` for the full flag list (`--dry-run`, `--force`).

### Level 2 — Claude Code plugin

Adds slash commands on top of the skills:

```sh
ln -s "$PWD/plugins/claude-code" ~/.claude/plugins/cruster
```

Slash commands:

- `/cruster:investigate <kind>/<name> [-n <ns>]` — diagnose & summarise
- `/cruster:bundle <kind>/<name> [-n <ns>]` — write a markdown report
- `/cruster:recent [-n <ns>]` — namespace triage

See `plugins/claude-code/README.md` for details.

## Project layout

```
cruster/
├── app/
│   ├── crates/
│   │   ├── cruster-core/   # shared types (ResourceKey, Environment, Snapshot)
│   │   ├── cruster-kube/   # kube-rs wrapper, watch streams, ResourceKind, StoreRegistry
│   │   ├── cruster-tui/    # ratatui app, ResourceView trait, views, actions, overlays, prompts, export, safety
│   │   ├── cruster-cli/    # clap CLI, Formatter, prune, budget, per-verb modules + schemas
│   │   └── cruster-bin/    # single `cruster` binary; dispatches TUI vs CLI
│   └── benches/
├── skills/                  # agentskills.io-format skills (drop into any compatible agent)
├── plugins/
│   └── claude-code/         # Claude Code plugin: skills + slash commands
├── install.sh               # one-shot skills installer (--agent claude|cursor|generic)
├── web/                     # marketing site (Phase 5)
└── docs/                    # specs and implementation plans
```
