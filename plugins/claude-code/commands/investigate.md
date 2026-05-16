---
description: Investigate a failing Kubernetes pod (or any resource) by bundling its manifest, recent events, and log tail into a markdown report.
allowed-tools: Bash, Read
---

# /cruster:investigate

Diagnose what's wrong with a Kubernetes resource using cruster's
`export` verb. One CLI call produces a self-contained markdown
bundle — read it and explain the root cause to the user.

## Usage

The user invoked: `/cruster:investigate $ARGUMENTS`

`$ARGUMENTS` should be a resource reference and optional namespace,
e.g. `pod/my-pod -n production` or `deploy/web`. If the user only
gave a name (no `kind/`), ask them what kind, then proceed.

## Steps

1. Run `cruster export $ARGUMENTS -o /tmp/cruster-investigate.md` to
   build the bundle.
2. Read `/tmp/cruster-investigate.md` to see the manifest, events,
   and (for pods) log tail.
3. Summarise in 2-4 sentences:
   - **Status**: what state is it in?
   - **Root cause**: what's breaking it? Quote the specific event or
     log line.
   - **Next step**: the single most-useful action the user can take.
4. If the cause is ambiguous, run `cruster events --resource
   $ARGUMENTS --format ndjson` for the structured event stream and
   re-summarise.

## Tip

Don't dump the whole markdown bundle to the user — they can read it
themselves at `/tmp/cruster-investigate.md`. Just give them the
synthesis.
