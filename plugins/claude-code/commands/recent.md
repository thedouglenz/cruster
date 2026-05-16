---
description: Show what's been happening recently in a Kubernetes namespace — events + pod state side by side, surfacing warnings and non-Ready workloads.
allowed-tools: Bash
---

# /cruster:recent

Triage a namespace when the user doesn't have a specific resource in
mind. Pulls recent events and the current pod listing, then
summarises what's worth attention.

## Usage

The user invoked: `/cruster:recent $ARGUMENTS`

`$ARGUMENTS` may be empty (means all namespaces) or `-n <ns>` to
scope to one namespace.

## Steps

1. Run both in parallel:
   ```bash
   cruster events $ARGUMENTS --format ndjson
   cruster get pods $ARGUMENTS --format ndjson
   ```
2. From events: identify Warning-type events. Group by `reason`.
   Highlight recurring reasons (>2 occurrences) or any
   `FailedScheduling` / `BackOff` / `Evicted` activity.
3. From pods: list any whose `status.phase` is not `Running`/`Succeeded`,
   or whose container is in `waiting` state.
4. Summarise: a 3-5 line snapshot of the namespace's health. If
   everything is healthy, say so plainly.
5. If one resource keeps showing up across both streams, offer:
   "Want me to dig into <resource> with /cruster:investigate?"

## Tip

K8s events typically age out after ~1 hour. An empty events stream
doesn't mean nothing happened — it means nothing happened recently
and is still in retention. Tell the user this if relevant.
