# cruster agent skills

agentskills.io-format skills that teach an AI coding agent how to use
the `cruster` CLI for Kubernetes triage. Drop them into any agent
that supports the agentskills.io standard — Claude Code, Cursor,
Codex, Gemini CLI, Goose, OpenHands, GitHub Copilot, VS Code,
OpenCode, Amp, Roo Code, Junie, Kiro, and others.

## Skills

| Skill | When to activate |
|---|---|
| [`cruster-investigate-pod`](cruster-investigate-pod/SKILL.md) | A pod is failing / pending / crashing |
| [`cruster-resource-bundle`](cruster-resource-bundle/SKILL.md) | The user wants context on any non-Pod resource |
| [`cruster-recent-activity`](cruster-recent-activity/SKILL.md) | The user asks "what's happening in this namespace" |

All three lean on already-shipped cruster verbs: `export`, `events`,
`get`, `logs`, `help-json`. They don't require any plugin, daemon,
or API key — they're just text files telling the agent which CLI
calls to make.

## Install

### Claude Code

```sh
cp -R skills/cruster-* ~/.claude/skills/
```

Or use the convenience script at the repo root:

```sh
./install.sh --agent claude
```

### Cursor

```sh
mkdir -p .cursor/skills && cp -R skills/cruster-* .cursor/skills/
```

### agentskills.io default (Codex, Goose, generic)

```sh
mkdir -p ~/.agentskills && cp -R skills/cruster-* ~/.agentskills/
```

## Prerequisite

`cruster` binary on `$PATH`. Build from `app/` with `cd app && cargo
build --release && cp target/release/cruster ~/.local/bin/`.

## Authoring your own skill

Each skill is a directory containing a `SKILL.md` with YAML
frontmatter:

```markdown
---
name: my-skill-slug
description: One line describing when to activate this skill
---

# Markdown body the agent reads on activation

Tell the agent which commands to run, in what order, and what to
look for in the output.
```

Keep skill bodies short. Long instructions burn context every time
the skill activates.
