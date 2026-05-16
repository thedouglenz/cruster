# Cruster Phase 4B: agentskills.io Skills + Claude Code Plugin

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ship the agent-facing distribution surface for cruster:
1. A `skills/` directory containing agentskills.io-format skills that
   any of the 30+ supporting agents can drop in.
2. A `plugins/claude-code/` directory with a Claude Code plugin
   manifest + slash commands that compose the skills.
3. An `install.sh` per surface, plus README updates that explain how
   to wire them up.

**Scope guardrail:** This phase ships skills and plugin glue around
**already-shipped verbs only** (`get`, `describe`, `logs`, `events`,
`export`, `diff`, `schema`, `help-json`). The richer per-failure-mode
verbs from the spec (`why-pending`, `why-crashloop`,
`why-no-endpoints`, `what-changed`) are deferred — they'd each need
their own analysis logic and aren't blocking 4B. Sticking to existing
verbs means the skills work the moment a user installs them.

**Out of scope (for this phase):**
- New CLI verbs.
- TUI selection.json cache (was speculative; the plugin works fine
  by taking a `pod/<name>` argument from the user).
- Per-agent installer auto-detection — ship the skills as a
  drop-in folder + one `install.sh` for `~/.claude/skills/`.
- License / paywall enforcement in skills (skills always work; they
  point users at the cruster binary, which itself enforces the gate).

**Architecture:**
- `skills/` — top-level directory, peer of `app/` and `docs/`.
  Mirrors the layout other agentskills consumers expect: each skill
  is its own folder containing `SKILL.md`.
- `plugins/claude-code/` — top-level. Contains `plugin.json`,
  `commands/*.md` (slash commands), and a symlink-friendly copy /
  re-reference of the same skills.

---

## Task 1: Three core skills

Each skill is a directory under `skills/`. The `SKILL.md` uses
agentskills.io frontmatter:

```yaml
---
name: <slug>
description: <one-line: when to invoke this skill>
---
```

Followed by markdown instructions the agent reads on activation.

Skills to ship:

1. **`skills/cruster-investigate-pod/SKILL.md`** — when the user
   reports a pod is failing / pending / crashing. Tells the agent
   to run `cruster export pod/<name> -n <ns>` first (one call, full
   manifest + events + logs in markdown), then `cruster events
   --resource pod/<name> -n <ns>` if it needs the structured event
   stream.

2. **`skills/cruster-resource-bundle/SKILL.md`** — when the user
   asks for context on any non-Pod resource (Deployment, Service,
   ConfigMap, etc.). Tells the agent to use `cruster export
   <kind>/<name> -n <ns>` for the full manifest + related events.

3. **`skills/cruster-recent-activity/SKILL.md`** — when the user
   asks "what's been happening" / "what's wrong" in a namespace
   without naming a resource. Tells the agent to run `cruster events
   -n <ns> --format ndjson` and `cruster get pods -n <ns> --llm` for
   parallel triage.

Each skill also includes:
- A short "Discovery" note pointing at `cruster help-json` for the
  full verb surface (one call now thanks to inline schemas).
- A "Tip" reminding the agent that cruster auto-prunes managedFields
  / status timestamps unless `--full` is passed.

- [ ] Create the three skill directories with SKILL.md files.
- [ ] Add `skills/README.md` explaining the layout, the install
  paths per agent (Claude Code: `~/.claude/skills/`; Cursor:
  `.cursor/skills/`; agentskills.io default: `~/.agentskills/`),
  and how to drop them in.
- [ ] Commit.

---

## Task 2: Claude Code plugin

Claude Code plugins are directories with a `plugin.json` manifest and
sub-directories for `skills/`, `commands/`, `agents/`, `hooks/`. For
v1 we ship skills + commands.

- [ ] Create `plugins/claude-code/plugin.json`:

```json
{
  "name": "cruster",
  "version": "0.1.0",
  "description": "Kubernetes TUI + CLI with agent-friendly diagnostics. Ships skills + slash commands for investigating workloads with Claude Code.",
  "author": "Doug Lenz <thedouglenz@gmail.com>",
  "homepage": "https://cruster.dev",
  "license": "Proprietary",
  "skills": ["skills/*"],
  "commands": ["commands/*"]
}
```

- [ ] Copy (or re-write thinner variants of) the three Task 1 skills
  into `plugins/claude-code/skills/`. They can be shorter here since
  they live alongside Claude Code's invocation environment.

- [ ] Create slash commands under `plugins/claude-code/commands/`:

  - `investigate.md` — `/cruster:investigate <kind>/<name> [-n <ns>]`.
    Body instructs Claude to run `cruster export` against the arg and
    summarise the result. Includes one fenced bash example.
  - `bundle.md` — `/cruster:bundle <kind>/<name> [-n <ns>]`. Same idea
    but explicitly leaves the markdown for the user to read.
  - `recent.md` — `/cruster:recent [-n <ns>]`. Calls `cruster events`
    + `cruster get pods` for a namespace overview.

  Each command file follows the Claude Code slash-command spec: YAML
  frontmatter with `name` + `description`, then markdown body that
  Claude treats as its instructions when the command fires.

- [ ] `plugins/claude-code/README.md` documenting install:
  `git clone … && ln -s $PWD/plugins/claude-code ~/.claude/plugins/cruster`
  (or `claude plugin install` once that exists).

- [ ] Commit.

---

## Task 3: Install script + top-level README

- [ ] `install.sh` at the repo root that, given an `--agent <claude|cursor|generic>`
  flag, copies the `skills/` directory to the right path:
  - `claude` → `~/.claude/skills/`
  - `cursor` → `.cursor/skills/` in cwd
  - `generic` (default) → `~/.agentskills/`
  Idempotent (skips existing skill directories or replaces them based
  on a `--force` flag).

- [ ] Update top-level README.md:
  - Add a "Agent integration" section that points at the three install
    paths above.
  - Bump status to Phase 4B and list the new artifacts.

- [ ] Commit.

---

## Task 4: Verify the skills end-to-end

This isn't a unit-test pass — it's a smoke test against the actual
agent invocation surface. Since I can't programmatically install into
Claude Code mid-session, the verification is:

- [ ] Run `cargo test --workspace` to confirm no regressions.
- [ ] `cat skills/cruster-investigate-pod/SKILL.md` — sanity check
  that the frontmatter parses (run it through `python3 -c "import
  yaml; print(yaml.safe_load(open('skills/cruster-investigate-pod/
  SKILL.md').read().split('---')[1]))"` or similar).
- [ ] `cat plugins/claude-code/plugin.json | jq .` — confirm valid
  JSON.
- [ ] Run `./install.sh --agent claude --dry-run` (dry-run flag the
  script should support) to confirm install paths resolve.
- [ ] Tag `phase-4b-skills-and-plugin`.

When 4B ships, Phase 5 (license + landing page + polish) follows.
