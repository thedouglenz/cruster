---
name: cruster-investigate-pod
description: Investigate why a Kubernetes pod is failing, pending, crashing, or otherwise misbehaving. Activate when the user reports a problem with a specific pod (CrashLoopBackOff, ImagePullBackOff, OOMKilled, stuck Pending, exit code != 0, restart loop).
---

# Investigate a failing pod with cruster

Cruster ships a single command that bundles manifest, recent events,
and a log tail into one markdown report. Reach for this before
running individual `kubectl describe` / `kubectl logs` calls — it's
one round-trip instead of three.

## Primary call

```bash
cruster export pod/<name> -n <namespace>
```

Output: a self-contained markdown document with sections for the pod
manifest (YAML), recent events involving the pod, and the last 100
log lines. Write it to a file with `-o report.md` if you want to
attach it to a ticket.

If the pod has multiple containers and you want logs from a specific
one, use `--tail <N>` to widen the log window — then look for the
relevant container's lines in the bundle.

## Follow-up calls (only if the bundle isn't enough)

Structured event stream (NDJSON, easy to grep):

```bash
cruster events --resource pod/<name> -n <namespace> --format ndjson
```

If you get an empty result, look for the sentinel line `{"matched":
0, "filtered_from": N}` — that tells you the filter ate N events vs
"no events exist at all".

Streaming logs (only if you need lines newer than the export window):

```bash
cruster logs <name> -n <namespace> --tail 500
```

Multi-pattern log scan with context — the dense-evidence call. Use
this when you already know which symptoms to look for; one call
replaces several `kubectl logs | grep` follow-ups:

```bash
cruster logs <name> -n <namespace> \
  --grep auth='(?i)\b(401|403|unauthorized|invalid[\s_-]?api[\s_-]?key)\b' \
  --grep panic='panic:|fatal error:|Traceback' \
  --grep-literal netfail='connection refused' \
  -A 3 -B 3 --previous --all-containers
```

- `--grep` is regex; `--grep-literal` is a substring (auto-escaped).
  Both repeatable. Both accept `[name=]pattern` so each hit is tagged
  with the probe that fired.
- `-A`/`-B`/`--context` give grep-style context lines.
- `--previous` also scans the previous container's logs (the last
  crash) — usually where the actual crash output lives.
- `--all-containers` fans out across every container in the pod.

Output is NDJSON: one `{"hit": ...}` record per match (with `before` /
`after` context and a negative-from-end `line_offset`), followed by
one terminal `{"summary": ...}` record. Read the summary first — its
`patterns_unhit` field lists every probe that came up empty, so you
don't waste a follow-up call re-checking them.

Pod state in machine form:

```bash
cruster get pods <name> -n <namespace> --format ndjson | jq .
```

Merged chronological timeline (events + derived restarts + referenced
config rotations, in one ordered stream) — use when "what happened,
in order?" is the question:

```bash
cruster timeline pod/<name> -n <namespace> --since 1h
```

Output is NDJSON: one record per fact, then one terminal
`{"summary": ...}` carrying counts and `kinds_absent` (which probes
came up empty). `--include restarted,oom_killed` narrows a noisy
timeline. The `secret_rotated` / `configmap_changed` records are
pulled from `managedFields[].time` so you see when a Secret was
last updated even if k8s never emits an event for it — useful for
"was it the secret rotation that broke us?" hypotheses.

## Discovery

Run `cruster help-json` once and cache the result. It now inlines
every verb's JSON schema, so you don't need to follow up with `cruster
schema <verb>` per verb.

## Common diagnoses cruster surfaces clearly

- **Auth / config errors**: visible in logs in the export. Look for
  HTTP 401/403, "unauthorized", "invalid api key" near the end of
  the log tail.
- **Image pull failures**: surface as `Failed` events in the events
  table with reason `Failed` / `ErrImagePull`.
- **Resource limits**: `OOMKilled` shows up in
  `containerStatuses[*].lastState.terminated.reason` in the YAML.
- **Scheduling failures**: events with reason `FailedScheduling`
  explain why no node accepted the pod.

## Tip

By default cruster prunes `managedFields`, `status.conditions[*].lastProbeTime`,
and similar noise from output. Pass `--full` to disable pruning if
you need the wire-format manifest. Don't reach for `--full` reflexively
— pruned output saves tokens and is usually sufficient for diagnosis.
