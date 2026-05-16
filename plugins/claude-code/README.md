# cruster — Claude Code plugin

A Claude Code plugin that teaches Claude how to use the `cruster`
CLI for Kubernetes triage. Bundles three agentskills.io skills and
three slash commands.

## Slash commands

| Command | What it does |
|---|---|
| `/cruster:investigate <kind>/<name> [-n <ns>]` | Bundle + summarise: Claude reads the report and tells you the root cause |
| `/cruster:bundle <kind>/<name> [-n <ns>]` | Just write the markdown; Claude hands you the file path |
| `/cruster:recent [-n <ns>]` | Parallel events + pods scan, prioritised summary |

## Bundled skills

Same three skills shipped at the repo root under `skills/`:

- `cruster-investigate-pod`
- `cruster-resource-bundle`
- `cruster-recent-activity`

These auto-activate based on what the user asks about; the slash
commands above are explicit invocations.

## Install

### As a Claude Code plugin

```sh
# clone the cruster repo somewhere
git clone https://github.com/thedouglenz/cruster.git ~/src/cruster

# link or copy the plugin into Claude Code's plugins dir
ln -s ~/src/cruster/plugins/claude-code ~/.claude/plugins/cruster
```

If you prefer a copy over a symlink (Windows, or you don't want the
plugin to update on `git pull`):

```sh
cp -R ~/src/cruster/plugins/claude-code ~/.claude/plugins/cruster
```

### Or just the skills

If you don't want the slash commands and only want the auto-activating
skills, drop them in directly:

```sh
cp -R ~/src/cruster/skills/cruster-* ~/.claude/skills/
```

## Prerequisites

The `cruster` binary needs to be on your `$PATH`. Build from source:

```sh
cd ~/src/cruster/app
cargo build --release
cp target/release/cruster ~/.local/bin/
```

Verify with `cruster help-json | jq '.[].name'`.
