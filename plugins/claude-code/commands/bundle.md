---
description: Build a diagnostic markdown bundle for any Kubernetes resource and hand the file path back to the user — for incident tickets, postmortems, or async handoffs.
allowed-tools: Bash
---

# /cruster:bundle

Produce a self-contained markdown report for a Kubernetes resource
and tell the user where to find it. Unlike `/cruster:investigate`,
this command does **not** read the bundle or summarise — the user
wants the artifact, not your interpretation.

## Usage

The user invoked: `/cruster:bundle $ARGUMENTS`

`$ARGUMENTS` should be a resource reference and optional namespace,
e.g. `pod/my-pod -n production` or `svc/api`.

## Steps

1. Choose an output path: `./cruster-bundle-<kind>-<name>-<ts>.md`
   where `<ts>` is the current Unix epoch. Use `date +%s` for the
   timestamp.
2. Run `cruster export $ARGUMENTS -o <output-path>`.
3. Tell the user the path and the file size (`wc -c` it). Don't
   describe the contents — just confirm it was written.

## When to use this vs. /cruster:investigate

| Want | Use |
|---|---|
| The agent to diagnose for me | `/cruster:investigate` |
| A markdown file I can paste in Slack / a ticket | `/cruster:bundle` |
